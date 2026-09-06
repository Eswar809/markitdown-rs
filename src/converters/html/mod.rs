//! HTML → Markdown converter — port of `_html_converter.py`.

mod markdownify;
mod parser;

use std::io::Cursor;

use crate::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};
use self::parser::{parse, Dom, NodeKind};

const ACCEPTED_MIME_TYPE_PREFIXES: [&str; 2] = ["text/html", "application/xhtml"];
const ACCEPTED_FILE_EXTENSIONS: [&str; 2] = [".html", ".htm"];

pub struct HtmlConverter;

impl Default for HtmlConverter {
    fn default() -> Self {
        Self
    }
}

impl DocumentConverter for HtmlConverter {
    fn name(&self) -> &'static str {
        "HtmlConverter"
    }

    fn accepts(&self, _stream: &mut Cursor<Vec<u8>>, stream_info: &StreamInfo) -> bool {
        let mimetype = stream_info.mimetype.as_deref().unwrap_or("").to_lowercase();
        let extension = stream_info.extension.as_deref().unwrap_or("").to_lowercase();
        if ACCEPTED_FILE_EXTENSIONS.contains(&extension.as_str()) {
            return true;
        }
        ACCEPTED_MIME_TYPE_PREFIXES
            .iter()
            .any(|prefix| mimetype.starts_with(prefix))
    }

    fn convert(
        &self,
        stream: &mut Cursor<Vec<u8>>,
        stream_info: &StreamInfo,
    ) -> Result<DocumentConverterResult, ConverterError> {
        // v1: UTF-8 (charset_normalizer-style detection is a TODO)
        let _ = &stream_info.charset;
        let html = String::from_utf8_lossy(stream.get_ref());
        let mut dom = parse(&html);

        // Remove javascript and style blocks (upstream extracts them from the
        // soup before conversion).
        prune_script_style(&mut dom, 0);

        // Prefer the body element, otherwise the whole document.
        let body = dom.find_all(0, &["body"]).first().copied().unwrap_or(0);
        let webpage_text = markdownify::convert(&dom, body).trim().to_string();

        let title = find_title(&dom).filter(|t| !t.is_empty());

        Ok(DocumentConverterResult {
            title,
            markdown: webpage_text,
        })
    }
}

/// Detaches <script>/<style> elements from the tree (equivalent of upstream's
/// `soup(["script", "style"])` extraction pass).
fn prune_script_style(dom: &mut Dom, idx: usize) {
    let keep: Vec<usize> = dom.nodes[idx]
        .children
        .iter()
        .copied()
        .filter(|&c| match dom.name(c) {
            Some(n) => n != "script" && n != "style",
            None => true,
        })
        .collect();
    dom.nodes[idx].children = keep;
    let remaining = dom.nodes[idx].children.clone();
    for child in remaining {
        prune_script_style(dom, child);
    }
}

/// Port of `soup.title.string` — the text inside the first <title> element.
fn find_title(dom: &Dom) -> Option<String> {
    let title = *dom.find_all(0, &["title"]).first()?;
    let mut text = String::new();
    for &child in &dom.nodes[title].children {
        if let NodeKind::Text(t) = &dom.nodes[child].kind {
            text.push_str(t);
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_html(html: &str) -> String {
        let info = StreamInfo {
            extension: Some(".html".into()),
            charset: Some("utf-8".into()),
            ..Default::default()
        };
        HtmlConverter
            .convert(&mut Cursor::new(html.as_bytes().to_vec()), &info)
            .unwrap()
            .markdown
    }


    #[test]
    fn headings_and_paragraph() {
        assert_eq!(
            convert_html("<h1>Title</h1><h2>Sub</h2><p>Body text</p>"),
            "# Title\n\n## Sub\n\nBody text"
        );
    }

    #[test]
    fn links() {
        assert_eq!(
            convert_html(r#"<p>see <a href="page 1.html">the page</a> now</p>"#),
            "see [the page](page%201.html) now"
        );
        assert_eq!(
            convert_html(r#"<p><a href="javascript:void(0)">bad</a></p>"#),
            "bad"
        );
        assert_eq!(
            convert_html(
                r#"<p><a href="https://example.com">https://example.com</a></p>"#
            ),
            "<https://example.com>"
        );
    }

    #[test]
    fn images() {
        assert_eq!(
            convert_html(r#"<p>hi <img src="x.png" alt="PIC"> there</p>"#),
            "hi ![PIC](x.png) there"
        );
        assert_eq!(
            convert_html(r#"<img src="data:image/png;base64,AAAA" alt="D">"#),
            "![D](data:image/png;base64...)"
        );
    }

    #[test]
    fn script_style_removed() {
        assert_eq!(
            convert_html(
                "<body><script>evil()</script><style>.x{}</style><p>kept</p></body>"
            ),
            "kept"
        );
    }

    #[test]
    fn checkbox() {
        assert_eq!(
            convert_html(
                r#"<input type="checkbox" checked>done<input type="checkbox">todo"#
            ),
            "[x] done[ ] todo"
        );
    }

    #[test]
    fn table_without_thead() {
        assert_eq!(
            convert_html(
                "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"
            ),
            "| A | B |\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn nested_lists() {
        assert_eq!(
            convert_html("<ul><li>one<ul><li>nested</li></ul></li><li>two</li></ul>"),
            "* one\n  + nested\n* two"
        );
    }

    #[test]
    fn lists_and_emphasis() {
        assert_eq!(
            convert_html(
                "<p><b>bold</b> and <em>em</em> and <u>u</u> and <s>del</s></p>"
            ),
            "**bold** and *em* and <u>u</u> and ~~del~~"
        );
        assert_eq!(
            convert_html("<ol start=\"3\"><li>x</li><li>y</li></ol>"),
            "3. x\n4. y"
        );
    }

    #[test]
    fn pre_and_code() {
        assert_eq!(
            convert_html("<pre>def f():\n    pass</pre>"),
            "```\ndef f():\n    pass\n```"
        );
        assert_eq!(
            convert_html("<p>use <code>x | y</code> here</p>"),
            "use `x | y` here"
        );
    }

    #[test]
    fn escapes_and_entities() {
        assert_eq!(convert_html("<p>3 * 4 + snake_case</p>"), "3 \\* 4 + snake\\_case");
        assert_eq!(
            convert_html("<p>A &amp; B &lt; C &quot;d&quot;</p>"),
            "A & B < C \"d\""
        );
    }

    #[test]
    fn blockquote_and_br() {
        assert_eq!(
            convert_html("<blockquote>quoted<b>bold</b></blockquote>"),
            "> quoted**bold**"
        );
        assert_eq!(convert_html("<p>one<br>two</p>"), "one  \ntwo");
    }

    #[test]
    fn title_extracted() {
        let info = StreamInfo {
            extension: Some(".html".into()),
            ..Default::default()
        };
        let res = HtmlConverter.convert(
            &mut Cursor::new(
                br#"<html><head><title>My Title</title></head><body><p>content</p></body></html>"#.to_vec(),
            ),
            &info,
        )
        .unwrap();
        assert_eq!(res.markdown, "content");
        assert_eq!(res.title.as_deref(), Some("My Title"));
    }
}
