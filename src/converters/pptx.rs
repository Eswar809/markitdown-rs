//! PPTX → Markdown converter — port of `_pptx_converter.py` (python-pptx
//! pipeline). Per slide: `\n\n<!-- Slide number: N -->\n`, shapes sorted by
//! (top, left) with falsy/missing offsets sorting first (-inf), pictures and
//! tables emitted via their own formats, tables round-tripped through the
//! HTML converter, charts rendered as pipe tables with Python float str
//! semantics, groups recursed. See pptx_spec.md for the full behavioral spec.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_PREFIX: &str =
    "application/vnd.openxmlformats-officedocument.presentationml";
const ACCEPTED_FILE_EXTENSIONS: [&str; 1] = [".pptx"];

const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
// c: namespace lives in the chart parts
const C_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";

pub struct PptxConverter;

impl Default for PptxConverter {
    fn default() -> Self {
        Self
    }
}

fn is_el(node: roxmltree::Node, ns: &str, local: &str) -> bool {
    node.tag_name().namespace() == Some(ns) && node.tag_name().name() == local
}

fn html_escape_cell(s: &str) -> String {
    // Python html.escape(s, quote=True)
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Python str() for floats: integral values keep a ".0" suffix.
fn python_float_str(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

/// alt text sanitizer: [\r\n\[\]] → " ", then \s+ → " ", then trim.
fn sanitize_alt(alt: &str) -> String {
    let mut out = String::with_capacity(alt.len());
    for c in alt.chars() {
        if matches!(c, '\r' | '\n' | '[' | ']') {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut in_space = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !in_space {
                collapsed.push(' ');
                in_space = true;
            }
        } else {
            collapsed.push(c);
            in_space = false;
        }
    }
    collapsed.trim().to_string()
}

/// placeholder filename: re.sub(r"\W", "", name) + ".jpg" (Unicode \W).
fn picture_filename(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|&c| c.is_alphanumeric() || c == '_')
        .collect();
    format!("{}.jpg", kept)
}

struct SlidePart {
    path: String,
}

struct PptxContext {
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
    // per-part relationship maps: "ppt/slides/_rels/slide1.xml.rels" → {rId → target}
    rels_cache: HashMap<String, HashMap<String, String>>,
}

impl PptxContext {
    fn open(data: &[u8]) -> Result<Self, ConverterError> {
        let archive = zip::ZipArchive::new(Cursor::new(data.to_vec()))
            .map_err(|e| ConverterError(format!("not a valid pptx zip: {}", e)))?;
        Ok(Self {
            archive,
            rels_cache: HashMap::new(),
        })
    }

    fn read_part(&mut self, path: &str) -> Option<String> {
        let mut f = self.archive.by_name(path).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s)
    }

    /// Relationships for a part (e.g. "ppt/slides/slide1.xml" →
    /// "ppt/slides/_rels/slide1.xml.rels"), cached; rId → target.
    fn rels_for(&mut self, part: &str) -> HashMap<String, String> {
        let dir = match part.rfind('/') {
            Some(pos) => &part[..pos],
            None => "",
        };
        let file = match part.rfind('/') {
            Some(pos) => &part[pos + 1..],
            None => part,
        };
        let rels_path = format!("{}/_rels/{}.rels", dir, file);
        if let Some(cached) = self.rels_cache.get(&rels_path) {
            return cached.clone();
        }
        let mut map = HashMap::new();
        if let Some(xml) = self.read_part(&rels_path) {
            if let Ok(tree) = roxmltree::Document::parse(&xml) {
                for node in tree.descendants() {
                    if node.tag_name().name() == "Relationship" {
                        if let (Some(id), Some(target)) =
                            (node.attribute("Id"), node.attribute("Target"))
                        {
                            map.insert(id.to_string(), target.to_string());
                        }
                    }
                }
            }
        }
        self.rels_cache.insert(rels_path, map.clone());
        map
    }
}

/// Text-frame semantics (python-pptx TextFrame.text): paragraphs joined by
/// "\n"; a paragraph concatenates run texts, field texts, and "\v" per break.
fn text_frame_text(tx_body: roxmltree::Node) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    for p in tx_body.children().filter(|&c| is_el(c, A_NS, "p")) {
        let mut para = String::new();
        for child in p.children() {
            if !child.is_element() {
                continue;
            }
            if is_el(child, A_NS, "r") {
                for t in child.children().filter(|&c| is_el(c, A_NS, "t")) {
                    para.push_str(t.text().unwrap_or(""));
                }
            } else if is_el(child, A_NS, "fld") {
                for t in child.children().filter(|&c| is_el(c, A_NS, "t")) {
                    para.push_str(t.text().unwrap_or(""));
                }
            } else if is_el(child, A_NS, "br") {
                para.push('\u{0b}');
            }
        }
        paragraphs.push(para);
    }
    paragraphs.join("\n")
}

/// Explicit (top, left) EMU offsets from a shape's own xfrm, if present.
fn shape_offset(dom: &roxmltree::Document, node: roxmltree::Node) -> Option<(i64, i64)> {
    let _ = dom;
    // sp/pic: p:spPr/a:xfrm/a:off ; graphicFrame: p:xfrm/a:off ; grpSp: p:grpSpPr/a:xfrm/a:off
    let xfrm = node
        .children()
        .find(|&c| {
            is_el(c, P_NS, "spPr") || is_el(c, P_NS, "xfrm") || is_el(c, P_NS, "grpSpPr")
        })
        .and_then(|prop| {
            prop.children()
                .find(|&x| is_el(x, A_NS, "xfrm") || is_el(x, P_NS, "xfrm"))
        })?;
    let off = xfrm.children().find(|&o| is_el(o, A_NS, "off"))?;
    let top: i64 = off.attribute("y")?.parse().ok()?;
    let left: i64 = off.attribute("x")?.parse().ok()?;
    Some((top, left))
}

/// Sort key: falsy (missing or 0) coordinates become -inf (port of the
/// `float("-inf") if not x.top else x.top` quirk).
fn sort_key(dom: &roxmltree::Document, node: roxmltree::Node) -> (f64, f64) {
    match shape_offset(dom, node) {
        Some((top, left)) => (
            if top == 0 { f64::NEG_INFINITY } else { top as f64 },
            if left == 0 { f64::NEG_INFINITY } else { left as f64 },
        ),
        None => (f64::NEG_INFINITY, f64::NEG_INFINITY),
    }
}

fn cNvPr<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> Option<roxmltree::Node<'a, 'input>> {
    // p:nvSpPr | p:nvPicPr | p:nvGraphicFramePr | p:nvGrpSpPr → p:cNvPr
    node.children()
        .find(|&c| {
            c.tag_name().namespace() == Some(P_NS)
                && c.tag_name().name().starts_with("nv")
        })
        .and_then(|nv| nv.children().find(|&c| is_el(c, P_NS, "cNvPr")))
}

fn has_ph_idx_zero(node: roxmltree::Node) -> bool {
    node.children()
        .find(|&c| is_el(c, P_NS, "nvSpPr"))
        .and_then(|nv| nv.children().find(|&c| is_el(c, P_NS, "nvPr")))
        .and_then(|nvpr| nvpr.children().find(|&c| is_el(c, P_NS, "ph")))
        .map(|ph| match ph.attribute("idx") {
            Some(idx) => idx == "0",
            None => true, // absent idx defaults to 0
        })
        .unwrap_or(false)
}

fn shape_children<'a, 'input>(sp_tree: roxmltree::Node<'a, 'input>) -> Vec<roxmltree::Node<'a, 'input>> {
    sp_tree
        .children()
        .filter(|&c| {
            c.is_element()
                && (is_el(c, P_NS, "sp")
                    || is_el(c, P_NS, "pic")
                    || is_el(c, P_NS, "graphicFrame")
                    || is_el(c, P_NS, "grpSp")
                    || is_el(c, P_NS, "cxnSp"))
        })
        .collect()
}

fn sorted_shapes<'a, 'input>(
    dom: &roxmltree::Document<'input>,
    shapes: Vec<roxmltree::Node<'a, 'input>>,
) -> Vec<roxmltree::Node<'a, 'input>> {
    let mut keyed: Vec<(f64, f64, usize, roxmltree::Node)> = shapes
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            let (t, l) = sort_key(dom, n);
            (t, l, i, n)
        })
        .collect();
    keyed.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            // python sorted() is stable → ties keep document order
            .then(a.2.cmp(&b.2))
    });
    keyed.into_iter().map(|(_, _, _, n)| n).collect()
}

struct PptxCtx<'input, 'a> {
    dom: &'a roxmltree::Document<'input>,
    ctx: &'a mut PptxContext,
    slide_part: String,
    title_elem: Option<usize>,
}

impl<'input, 'a> PptxCtx<'input, 'a> {
    fn is_title(&self, node: roxmltree::Node) -> bool {
        match self.title_elem {
            Some(t) => t == node.id().get_usize(),
            None => false,
        }
    }

    fn resolve_rel_target(&mut self, base_part: &str, rid: &str) -> Option<String> {
        let rels = self.ctx.rels_for(base_part);
        let target = rels.get(rid)?.clone();
        let base_dir = base_part.rfind('/').map(|p| &base_part[..p]).unwrap_or("");
        // resolve "../" segments relative to the base part's directory
        let mut parts: Vec<&str> = if target.starts_with('/') {
            return Some(target[1..].to_string());
        } else if base_dir.is_empty() {
            vec![]
        } else {
            base_dir.split('/').collect()
        };
        for seg in target.split('/') {
            match seg {
                "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        Some(parts.join("/"))
    }
}

fn emit_shape(node: roxmltree::Node, c: &mut PptxCtx, md: &mut String) {
    // --- picture ---
    if is_el(node, P_NS, "pic") {
        emit_picture(node, c, md);
    }

    // --- table ---
    let mut is_chart_frame = false;
    if is_el(node, P_NS, "graphicFrame") {
        if let Some(tbl) = node.descendants().find(|&d| is_el(d, A_NS, "tbl")) {
            emit_table(tbl, md);
        } else if let Some(graphic) = node
            .descendants()
            .find(|&d| is_el(d, A_NS, "graphicData"))
        {
            if graphic.attribute("uri").map(|u| u.contains("chart")) == Some(true) {
                is_chart_frame = true;
                let chart_md = emit_chart(node, c);
                md.push_str(&chart_md);
            }
        }
    }

    // --- text frame (p:sp), unless the graphicFrame already handled a chart ---
    if is_el(node, P_NS, "sp") && !is_chart_frame {
        if let Some(tx_body) = node.children().find(|&c| is_el(c, P_NS, "txBody")) {
            let text = text_frame_text(tx_body);
            if c.is_title(node) {
                md.push_str(&format!("# {}\n", text.trim_start()));
            } else {
                md.push_str(&format!("{}\n", text));
            }
        }
    }

    // --- group (separate if, recursive) ---
    if is_el(node, P_NS, "grpSp") {
        let children = shape_children(node);
        for child in sorted_shapes(c.dom, children) {
            emit_shape(child, c, md);
        }
    }
}

fn emit_picture(node: roxmltree::Node, c: &mut PptxCtx, md: &mut String) {
    let cnvpr = cNvPr(node);
    let alt_text = cnvpr
        .as_ref()
        .and_then(|n| n.attribute("descr"))
        .map(String::from)
        .unwrap_or_default();
    let shape_name = cnvpr
        .as_ref()
        .and_then(|n| n.attribute("name"))
        .unwrap_or("")
        .to_string();
    let alt_base = if alt_text.trim().is_empty() {
        shape_name.clone()
    } else {
        alt_text
    };
    let alt = sanitize_alt(&alt_base);
    let filename = picture_filename(&shape_name);
    md.push_str(&format!("\n![{}]({})\n", alt, filename));
}

fn emit_table(tbl: roxmltree::Node, md: &mut String) {
    let mut html = String::from("<html><body><table>");
    let mut first_row = true;
    for tr in tbl.children().filter(|&c| is_el(c, A_NS, "tr")) {
        html.push_str("<tr>");
        for tc in tr.children().filter(|&c| is_el(c, A_NS, "tc")) {
            let text = tc
                .children()
                .find(|&c| is_el(c, A_NS, "txBody"))
                .map(text_frame_text)
                .unwrap_or_default();
            let (open, close) = if first_row { ("<th>", "</th>") } else { ("<td>", "</td>") };
            html.push_str(&format!(
                "{}{}{}",
                open,
                html_escape_cell(&text),
                close
            ));
        }
        html.push_str("</tr>");
        first_row = false;
    }
    html.push_str("</table></body></html>");
    if let Ok(result) = super::html::convert_html_string(&html) {
        md.push_str(&format!("{}\n", result.markdown.trim()));
    }
}

/// Port of `chart_markdown`: title + categories × series pipe table with
/// Python float str semantics and an unpadded |---| separator.
fn emit_chart(frame: roxmltree::Node, c: &mut PptxCtx) -> String {
    let result = (|| -> Option<String> {
        let rid = frame
            .descendants()
            .find(|&d| d.tag_name().name() == "chart")
            .and_then(|d| d.attribute((R_NS, "id")))?;
        let chart_part = c.resolve_rel_target(&c.slide_part.clone(), &rid)?;
        let xml = c.ctx.read_part(&chart_part)?;
        let tree = roxmltree::Document::parse(&xml).ok()?;
        let chart = tree
            .descendants()
            .find(|&d| is_el(d, C_NS, "chart"))?;

        let mut md = String::from("\n\n### Chart");
        // title
        let has_title = chart
            .children()
            .find(|&d| is_el(d, C_NS, "title"))
            .map(|title| {
                let deleted = title
                    .children()
                    .find(|&d| is_el(d, C_NS, "autoTitleDeleted"))
                    .and_then(|d| d.attribute("val"))
                    .map(|v| v == "1")
                    .unwrap_or(false);
                !deleted
            })
            .unwrap_or(false);
        if has_title {
            let title_el = chart.children().find(|&d| is_el(d, C_NS, "title"))?;
            let text: String = title_el
                .descendants()
                .filter(|&d| is_el(d, A_NS, "t"))
                .filter_map(|d| d.text())
                .collect();
            md.push_str(&format!(": {}", text));
        }
        md.push_str("\n\n");

        // plot area series + categories
        let plot = chart.children().find(|&d| is_el(d, C_NS, "plotArea"))?;
        let plot_type = plot
            .children()
            .find(|&d| {
                d.is_element()
                    && d.tag_name().namespace() == Some(C_NS)
                    && matches!(
                        d.tag_name().name(),
                        "barChart"
                            | "lineChart"
                            | "areaChart"
                            | "pieChart"
                            | "doughnutChart"
                            | "scatterChart"
                            | "radarChart"
                            | "surfaceChart"
                            | "bubbleChart"
                    )
            })?;
        if matches!(
            plot_type.tag_name().name(),
            "scatterChart" | "bubbleChart" | "surfaceChart"
        ) {
            return None; // python-pptx raises unsupported → [unsupported chart]
        }

        let mut series_names: Vec<String> = Vec::new();
        let mut series_values: Vec<Vec<String>> = Vec::new();
        let mut categories: Option<Vec<String>> = None;

        let sers: Vec<roxmltree::Node> = plot_type
            .children()
            .filter(|&d| is_el(d, C_NS, "ser"))
            .collect();
        if sers.is_empty() {
            return None;
        }
        for ser in &sers {
            // series name
            let name = ser
                .children()
                .find(|&d| is_el(d, C_NS, "tx"))
                .and_then(|tx| {
                    tx.descendants()
                        .filter(|&d| is_el(d, C_NS, "v"))
                        .filter_map(|d| d.text())
                        .next()
                })
                .unwrap_or_default()
                .to_string();
            series_names.push(name);

            // categories (from the first series only, like python-pptx)
            if categories.is_none() {
                if let Some(cat) = ser.children().find(|&d| is_el(d, C_NS, "cat")) {
                    let pts: Vec<String> = cat
                        .descendants()
                        .filter(|&d| is_el(d, C_NS, "pt"))
                        .map(|pt| {
                            pt.descendants()
                                .find(|&d| is_el(d, C_NS, "v"))
                                .and_then(|d| d.text())
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect();
                    categories = Some(pts);
                }
            }

            // values by point index; missing points → None
            if let Some(val) = ser.children().find(|&d| is_el(d, C_NS, "val")) {
                let mut values: HashMap<usize, String> = HashMap::new();
                let mut max_idx = 0usize;
                for pt in val
                    .descendants()
                    .filter(|&d| is_el(d, C_NS, "pt"))
                {
                    let idx: usize = pt.attribute("idx").unwrap_or("0").parse().unwrap_or(0);
                    if let Some(v) = pt.descendants().find(|&d| is_el(d, C_NS, "v")) {
                        let raw = v.text().unwrap_or("");
                        let rendered = raw
                            .parse::<f64>()
                            .map(|f| python_float_str(f))
                            .unwrap_or_else(|_| raw.to_string());
                        values.insert(idx, rendered);
                    } else {
                        values.insert(idx, "None".to_string());
                    }
                    max_idx = max_idx.max(idx + 1);
                }
                let mut row = Vec::with_capacity(max_idx);
                for i in 0..max_idx {
                    row.push(values.remove(&i).unwrap_or_else(|| "None".to_string()));
                }
                series_values.push(row);
            } else {
                series_values.push(Vec::new());
            }
        }

        let cats = categories.unwrap_or_default();
        let row_count = cats.len().max(series_values.iter().map(|v| v.len()).sum::<usize>().max(0));

        let mut header: Vec<String> = vec!["Category".to_string()];
        header.extend(series_names);
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("| {} |", header.join(" | ")));
        lines.push(format!("|{}|", vec!["---"; header.len()].join("|")));
        for i in 0..row_count {
            let cat = cats.get(i).cloned().unwrap_or_else(|| "None".to_string());
            let mut cells = vec![cat];
            for vals in &series_values {
                cells.push(vals.get(i).cloned().unwrap_or_else(|| "None".to_string()));
            }
            lines.push(format!("| {} |", cells.join(" | ")));
        }
        Some(md + &lines.join("\n"))
    })();
    match result {
        Some(s) => s,
        None => "\n\n[unsupported chart]\n\n".to_string(),
    }
}

/// Notes text: the notes slide's body-placeholder text frame.
fn notes_text(notes_part: &str, ctx: &mut PptxContext) -> String {
    let Some(xml) = ctx.read_part(notes_part) else {
        return String::new();
    };
    let Ok(tree) = roxmltree::Document::parse(&xml) else {
        return String::new();
    };
    // body placeholder: p:sp whose p:ph type="body"
    for sp in tree.descendants().filter(|&d| is_el(d, P_NS, "sp")) {
        let is_body = sp
            .children()
            .find(|&c| is_el(c, P_NS, "nvSpPr"))
            .and_then(|nv| nv.children().find(|&c| is_el(c, P_NS, "nvPr")))
            .and_then(|nvpr| nvpr.children().find(|&c| is_el(c, P_NS, "ph")))
            .and_then(|ph| ph.attribute("type"))
            .map(|t| t == "body")
            .unwrap_or(false);
        if is_body {
            if let Some(tx) = sp.children().find(|&c| is_el(c, P_NS, "txBody")) {
                return text_frame_text(tx);
            }
        }
    }
    String::new()
}

impl DocumentConverter for PptxConverter {
    fn name(&self) -> &'static str {
        "PptxConverter"
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
        let mut ctx = PptxContext::open(&data)?;

        // presentation.xml → sldIdLst order → rels → slide parts
        let pres_rels = ctx.rels_for("ppt/presentation.xml");
        let pres_xml = ctx
            .read_part("ppt/presentation.xml")
            .ok_or_else(|| ConverterError("ppt/presentation.xml missing".into()))?;
        let pres_tree = roxmltree::Document::parse(&pres_xml)
            .map_err(|e| ConverterError(format!("presentation.xml parse: {}", e)))?;
        let mut slide_parts: Vec<String> = Vec::new();
        for sld_id in pres_tree
            .descendants()
            .filter(|&d| is_el(d, P_NS, "sldId"))
        {
            if let Some(rid) = sld_id.attribute((R_NS, "id")) {
                if let Some(target) = pres_rels.get(rid) {
                    let path = if let Some(stripped) = target.strip_prefix('/') {
                        stripped.to_string()
                    } else {
                        format!("ppt/{}", target)
                    };
                    // normalize "../" segments
                    let mut parts: Vec<&str> = Vec::new();
                    for seg in path.split('/') {
                        match seg {
                            "." => {}
                            ".." => {
                                parts.pop();
                            }
                            s => parts.push(s),
                        }
                    }
                    slide_parts.push(parts.join("/"));
                }
            }
        }

        let mut md = String::new();
        for (slide_num, slide_part) in slide_parts.iter().enumerate() {
            md.push_str(&format!("\n\n<!-- Slide number: {} -->\n", slide_num + 1));
            // NOTE: Python strips AFTER the slide's shapes are emitted — the
            // trailing newline of the comment itself survives until then.

            let Some(slide_xml) = ctx.read_part(slide_part) else {
                continue;
            };
            let Ok(slide_tree) = roxmltree::Document::parse(&slide_xml) else {
                continue;
            };
            let Some(sp_tree) = slide_tree
                .descendants()
                .find(|&d| is_el(d, P_NS, "spTree"))
            else {
                continue;
            };

            // title element: first p:sp in document order with ph idx absent/0
            let title_elem: Option<usize> = slide_tree
                .descendants()
                .find(|&d| is_el(d, P_NS, "sp") && has_ph_idx_zero(d))
                .map(|n| n.id().get_usize());

            let shapes = shape_children(sp_tree);
            let mut c = PptxCtx {
                dom: &slide_tree,
                ctx: &mut ctx,
                slide_part: slide_part.clone(),
                title_elem,
            };
            for shape in sorted_shapes(&slide_tree, shapes) {
                emit_shape(shape, &mut c, &mut md);
            }
            md = md.trim().to_string();

            // speaker notes (only if a notesSlide rel exists — never create)
            let rels = ctx.rels_for(slide_part);
            let notes_part = rels
                .values()
                .find(|t| t.contains("notesSlide"))
                .map(|t| {
                    if let Some(stripped) = t.strip_prefix('/') {
                        stripped.to_string()
                    } else {
                        let base_dir =
                            slide_part.rfind('/').map(|p| &slide_part[..p]).unwrap_or("");
                        let mut parts: Vec<&str> =
                            base_dir.split('/').collect();
                        for seg in t.split('/') {
                            match seg {
                                "." => {}
                                ".." => {
                                    parts.pop();
                                }
                                s => parts.push(s),
                            }
                        }
                        parts.join("/")
                    }
                });
            if let Some(notes_part) = notes_part {
                md.push_str("\n\n### Notes:\n");
                let text = notes_text(&notes_part, &mut ctx);
                md.push_str(&text);
                md = md.trim().to_string();
            }
        }

        Ok(DocumentConverterResult::new(md.trim()))
    }
}
