//! DOCX → HTML → Markdown converter — port of `_docx_converter.py`.
//!
//! Upstream pipeline: pre_process_docx → mammoth (docx → semantic HTML) →
//! HtmlConverter. This port replicates the mammoth HTML subset that the
//! converters exercise (paragraphs, headings via style NAME, runs with
//! bold/italic/underline/strikethrough, tables, embedded images, breaks),
//! then feeds the HTML through the same in-crate markdownify pipeline.

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_PREFIX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const ACCEPTED_FILE_EXTENSIONS: [&str; 1] = [".docx"];

use super::docx_math::M_NS;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

pub struct DocxConverter;

impl Default for DocxConverter {
    fn default() -> Self {
        Self
    }
}

fn is_el<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    ns: &str,
    local: &str,
) -> bool {
    node.tag_name().namespace() == Some(ns) && node.tag_name().name() == local
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn image_mime(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        "image/tiff"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".emf") {
        "image/x-emf"
    } else if lower.ends_with(".wmf") {
        "image/x-wmf"
    } else {
        "application/octet-stream"
    }
}

struct DocxContext<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    style_names: HashMap<String, String>, // styleId -> style name
    rel_targets: HashMap<String, String>, // rId -> target
}

impl<'a> DocxContext<'a> {
    fn open(data: &'a [u8]) -> Result<Self, ConverterError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(data))
            .map_err(|e| ConverterError(format!("not a valid docx zip: {}", e)))?;

        // styles: styleId -> name ("Heading 1" etc.) — heading detection uses
        // the style NAME because styleIds are often opaque ("1", "a3", ...)
        let mut style_names = HashMap::new();
        if let Ok(mut f) = archive.by_name("word/styles.xml") {
            let mut xml = String::new();
            f.read_to_string(&mut xml)
                .map_err(|e| ConverterError(format!("styles.xml: {}", e)))?;
            if let Ok(tree) = roxmltree::Document::parse(&xml) {
                for node in tree.descendants() {
                    if is_el(node, W_NS, "style") {
                        if let (Some(id), Some(name_node)) = (
                            node.attribute((W_NS, "styleId")),
                            node.descendants()
                                .find(|&d| is_el(d, W_NS, "name"))
                                .and_then(|n| n.attribute((W_NS, "val"))),
                        ) {
                            style_names.insert(id.to_string(), name_node.to_string());
                        }
                    }
                }
            }
        }

        // relationships: rId -> target
        let mut rel_targets = HashMap::new();
        if let Ok(mut f) = archive.by_name("word/_rels/document.xml.rels") {
            let mut xml = String::new();
            f.read_to_string(&mut xml)
                .map_err(|e| ConverterError(format!("document.xml.rels: {}", e)))?;
            if let Ok(tree) = roxmltree::Document::parse(&xml) {
                for node in tree.descendants() {
                    if node.tag_name().name() == "Relationship" {
                        if let (Some(id), Some(target)) =
                            (node.attribute("Id"), node.attribute("Target"))
                        {
                            rel_targets.insert(id.to_string(), target.to_string());
                        }
                    }
                }
            }
        }

        Ok(Self {
            archive,
            style_names,
            rel_targets,
        })
    }

    fn read_zip_file(&mut self, name: &str) -> Option<Vec<u8>> {
        let mut f = self.archive.by_name(name).ok()?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        Some(buf)
    }
}

/// Runs a closure over the parsed XML of a zip member.
fn with_xml<T>(
    ctx: &mut DocxContext,
    name: &str,
    f: impl FnOnce(&roxmltree::Document) -> Option<T>,
) -> Option<T> {
    let bytes = ctx.read_zip_file(name)?;
    let text = String::from_utf8_lossy(&bytes);
    let tree = roxmltree::Document::parse(&text).ok()?;
    f(&tree)
}

fn style_heading_level(ctx: &DocxContext, style_id: &str) -> Option<usize> {
    let name = ctx.style_names.get(style_id)?;
    let lower = name.to_lowercase();
    let rest = lower.strip_prefix("heading ")?;
    rest.trim().parse::<usize>().ok()
}

struct RunFormat {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

fn run_format(r: roxmltree::Node) -> RunFormat {
    let mut fmt = RunFormat {
        bold: false,
        italic: false,
        underline: false,
        strike: false,
    };
    if let Some(rpr) = r.children().find(|&c| is_el(c, W_NS, "rPr")) {
        for child in rpr.children() {
            if !child.is_element() {
                continue;
            }
            let local = child.tag_name().name();
            let val = child.attribute((W_NS, "val"));
            let on = match val {
                None => true,
                Some(v) => !v.eq_ignore_ascii_case("false")
                    && !v.eq_ignore_ascii_case("0")
                    && !v.eq_ignore_ascii_case("none"),
            };
            match local {
                "b" | "bCs" => fmt.bold = on,
                "i" | "iCs" => fmt.italic = on,
                "u" => fmt.underline = on,
                "strike" | "dstrike" => fmt.strike = on,
                _ => {}
            }
        }
    }
    fmt
}

/// Renders runs/links/images inside a paragraph or table cell into HTML.
fn render_content(dom: roxmltree::Node, ctx: &mut DocxContext, out: &mut String) {
    for child in dom.children() {
        if !child.is_element() {
            continue;
        }
        if is_el(child, W_NS, "r") {
            let fmt = run_format(child);
            let mut inner = String::new();
            for rc in child.children() {
                if !rc.is_element() {
                    continue;
                }
                if is_el(rc, W_NS, "t") {
                    inner.push_str(&html_escape(rc.text().unwrap_or("")));
                } else if is_el(rc, W_NS, "tab") {
                    inner.push('\t');
                } else if is_el(rc, W_NS, "br")
                    || is_el(rc, W_NS, "cr")
                {
                    inner.push_str("<br />");
                } else if is_el(rc, W_NS, "drawing") || is_el(rc, W_NS, "pict") {
                    // images live inside runs in Word documents
                    render_image(rc, ctx, &mut inner);
                }
            }
            if inner.is_empty() {
                continue;
            }
            if fmt.bold {
                out.push_str("<strong>");
            }
            if fmt.italic {
                out.push_str("<em>");
            }
            if fmt.underline {
                out.push_str("<u>");
            }
            if fmt.strike {
                out.push_str("<s>");
            }
            out.push_str(&inner);
            if fmt.strike {
                out.push_str("</s>");
            }
            if fmt.underline {
                out.push_str("</u>");
            }
            if fmt.italic {
                out.push_str("</em>");
            }
            if fmt.bold {
                out.push_str("</strong>");
            }
        } else if is_el(child, W_NS, "hyperlink") {
            let target = child
                .attribute((R_NS, "id"))
                .and_then(|id| ctx.rel_targets.get(id).cloned());
            let mut inner = String::new();
            render_content(child, ctx, &mut inner);
            if inner.is_empty() {
                continue;
            }
            match target {
                Some(t) if !t.is_empty() => {
                    let href = if t.starts_with("http") { t } else { t };
                    out.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        html_escape(&href),
                        inner
                    ));
                }
                _ => out.push_str(&inner),
            }
        } else if is_el(child, W_NS, "drawing")
            || is_el(child, W_NS, "pict")
        {
            render_image(child, ctx, out);
        } else if is_el(child, M_NS, "oMathPara") {
            // block equation: every child oMath becomes a $$...$$ block
            for om in child.children().filter(|&c| is_el(c, M_NS, "oMath")) {
                let latex = super::docx_math::omath_to_latex(om);
                out.push_str(&html_escape(&format!("$${}$$", latex)));
            }
        } else if is_el(child, M_NS, "oMath") {
            let latex = super::docx_math::omath_to_latex(child);
            out.push_str(&html_escape(&format!("${}$", latex)));
        } else {
            // w:ins, w:smartTag, sdt content, ... — recurse transparently
            render_content(child, ctx, out);
        }
    }
}

fn render_image(node: roxmltree::Node, ctx: &mut DocxContext, out: &mut String) {
    // find docPr (alt text) and a:blip (image ref) anywhere in the drawing
    let mut alt: Option<String> = None;
    let mut embed: Option<String> = None;
    for d in node.descendants() {
        if alt.is_none() && is_el(d, WP_NS, "docPr") {
            alt = d
                .attribute("descr")
                .or_else(|| d.attribute("name"))
                .map(String::from);
        }
        if embed.is_none() && d.tag_name().name() == "blip" {
            embed = d.attribute((R_NS, "embed")).map(String::from);
        }
    }
    let Some(alt) = alt else { return };
    let Some(embed) = embed else { return };
    let Some(target) = ctx.rel_targets.get(&embed).cloned() else {
        return; // external/linked images are dropped, like mammoth
    };
    // embedded media: only the mime reaches the output (markdownify truncates
    // data URIs), so the real base64 payload is not needed
    let mime = image_mime(&target).to_string();
    let _ = ctx.read_zip_file(&format!("word/{}", target));
    out.push_str(&format!(
        "<img alt=\"{}\" src=\"data:{};base64,\" />",
        html_escape(&alt),
        mime
    ));
}

fn collect_text(node: roxmltree::Node) -> String {
    let mut out = String::new();
    for d in node.descendants() {
        if is_el(d, W_NS, "t") {
            out.push_str(d.text().unwrap_or(""));
        }
    }
    out
}

/// Renders a w:p (paragraph) — headings are resolved via the style NAME.
fn render_paragraph(p: roxmltree::Node, ctx: &mut DocxContext, out: &mut String) {
    let style_id = p
        .children()
        .find(|&c| is_el(c, W_NS, "pPr"))
        .and_then(|ppr| ppr.children().find(|&c| is_el(c, W_NS, "pStyle")))
        .and_then(|ps| ps.attribute((W_NS, "val")))
        .map(String::from);

    let mut inner = String::new();
    render_content(p, ctx, &mut inner);

    let heading_level = style_id.as_deref().and_then(|id| style_heading_level(ctx, id));
    if inner.is_empty() {
        return; // empty paragraphs produce nothing, like mammoth
    }
    match heading_level {
        Some(n) => {
            let n = n.clamp(1, 6);
            out.push_str(&format!("<h{}>{}</h{}>", n, inner, n));
        }
        None => out.push_str(&format!("<p>{}</p>", inner)),
    }
}

/// Renders a w:tbl — rows and cells mirror mammoth's <table>/<tr>/<td>.
fn render_table(tbl: roxmltree::Node, ctx: &mut DocxContext, out: &mut String) {
    out.push_str("<table>");
    for row in tbl.children().filter(|&c| is_el(c, W_NS, "tr")) {
        out.push_str("<tr>");
        for cell in row.children().filter(|&c| is_el(c, W_NS, "tc")) {
            let colspan = cell
                .children()
                .find(|&c| is_el(c, W_NS, "tcPr"))
                .and_then(|tcpr| tcpr.children().find(|&c| is_el(c, W_NS, "gridSpan")))
                .and_then(|gs| gs.attribute((W_NS, "val")))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let mut inner = String::new();
            render_content(cell, ctx, &mut inner);
            if inner.is_empty() {
                inner.push_str("<p></p>");
            }
            if colspan > 1 {
                out.push_str(&format!("<td colspan=\"{}\">{}</td>", colspan, inner));
            } else {
                out.push_str(&format!("<td>{}</td>", inner));
            }
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
}

impl DocumentConverter for DocxConverter {
    fn name(&self) -> &'static str {
        "DocxConverter"
    }

    fn accepts(&self, _stream: &mut Cursor<Vec<u8>>, stream_info: &StreamInfo) -> bool {
        let mimetype = stream_info.mimetype.as_deref().unwrap_or("").to_lowercase();
        let extension = stream_info.extension.as_deref().unwrap_or("").to_lowercase();
        if ACCEPTED_FILE_EXTENSIONS.contains(&extension.as_str()) {
            return true;
        }
        mimetype.starts_with(ACCEPTED_MIME_PREFIX)
    }

    fn convert(
        &self,
        stream: &mut Cursor<Vec<u8>>,
        _stream_info: &StreamInfo,
    ) -> Result<DocumentConverterResult, ConverterError> {
        let data = stream.get_ref().clone();
        let mut ctx = DocxContext::open(&data)?;

        // parse document.xml separately so the tree doesn't hold the ctx borrow
        let doc_bytes = ctx
            .read_zip_file("word/document.xml")
            .ok_or_else(|| ConverterError("word/document.xml missing".into()))?;
        let doc_text = String::from_utf8_lossy(&doc_bytes).to_string();
        let tree = roxmltree::Document::parse(&doc_text)
            .map_err(|e| ConverterError(format!("could not parse word/document.xml: {}", e)))?;

        let html = {
            let body = tree
                .descendants()
                .find(|&n| is_el(n, W_NS, "body"))
                .ok_or_else(|| ConverterError("word/document.xml has no body".into()))?;
            let mut out = String::new();
            for child in body.children() {
                if !child.is_element() {
                    continue;
                }
                if is_el(child, W_NS, "p") {
                    render_paragraph(child, &mut ctx, &mut out);
                } else if is_el(child, W_NS, "tbl") {
                    render_table(child, &mut ctx, &mut out);
                } else if is_el(child, W_NS, "sdt") {
                    render_content(child, &mut ctx, &mut out);
                }
                // sectPr, bookmarkStart/End, proofErr... are ignored
            }
            out
        };

        // reuse the in-crate HTML → markdownify pipeline
        let result = super::html::convert_html_string(&html)?;
        Ok(result)
    }
}

