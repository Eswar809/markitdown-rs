//! CSV → Markdown table converter — port of `_csv_converter.py`.

use std::io::Cursor;

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_TYPE_PREFIXES: [&str; 2] = ["text/csv", "application/csv"];
const ACCEPTED_FILE_EXTENSIONS: [&str; 1] = [".csv"];

/// Streaming version of [`escape_table_cell`] — writes the escaped cell
/// directly into `out` without an intermediate allocation. Handles pipe
/// escaping (with backslash-run doubling) and newline collapsing (\r\n -> " ")
/// in one pass, matching Python's two sequential transforms.
fn push_escaped(out: &mut String, value: &str) {
    let mut backslashes = 0usize;
    let mut after_cr = false;
    for c in value.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                after_cr = false;
            }
            '|' => {
                for _ in 0..backslashes * 2 {
                    out.push('\\');
                }
                backslashes = 0;
                out.push('\\');
                out.push('|');
                after_cr = false;
            }
            '\r' => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(' ');
                after_cr = true;
            }
            '\n' => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                if !after_cr {
                    out.push(' ');
                }
                after_cr = false;
            }
            other => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(other);
                after_cr = false;
            }
        }
    }
    for _ in 0..backslashes {
        out.push('\\');
    }
}

/// Escapes newlines only (cells already pipe-escaped) — the tail of Python's
/// `_escape_table_cell`.
fn collapse_newlines(value: String) -> String {
    value
        .replace("\r\n", " ")
        .replace('\n', " ")
        .replace('\r', " ")
}

/// Escapes a cell so it is safe inside a Markdown table cell.
pub fn escape_table_cell(value: &str) -> String {
    collapse_newlines(push_escaped_into(value))
}

fn push_escaped_into(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    push_escaped(&mut out, value);
    out
}

/// Remove empty rows from the beginning and end, and immediately after the
/// header. An "empty row" is a row with zero cells (a blank CSV line), which
/// is exactly Python's falsy-list check.
fn trim_outer_blank_rows(rows: &mut Vec<Vec<String>>) {
    while rows.first().map(|r| r.is_empty()).unwrap_or(false) {
        rows.remove(0);
    }
    while rows.len() > 1 && rows[1].is_empty() {
        rows.remove(1);
    }
    while rows.last().map(|r| r.is_empty()).unwrap_or(false) {
        rows.pop();
    }
}

/// Decodes bytes with the given charset (v1: UTF-8 only, lossy) and strips a
/// leading BOM — Excel and other tools prepend a UTF-8 BOM to CSV exports, so
/// it must not end up inside the first header cell.
fn decode_and_strip_bom(bytes: &[u8], charset: Option<&str>) -> String {
    let _ = charset; // v1: only UTF-8; charset_normalizer-style detection is a TODO
    let text = String::from_utf8_lossy(bytes);
    text.trim_start_matches('\u{feff}').to_string()
}

/// Converts CSV files to Markdown tables.
pub struct CsvConverter;

impl Default for CsvConverter {
    fn default() -> Self {
        Self
    }
}

impl DocumentConverter for CsvConverter {
    fn name(&self) -> &'static str {
        "CsvConverter"
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
        let content = decode_and_strip_bom(stream.get_ref(), stream_info.charset.as_deref());

        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(content.as_bytes());
        for record in reader.records() {
            let record = record.map_err(|e| ConverterError(format!("csv parse error: {}", e)))?;
            rows.push(record.iter().map(|s| s.to_string()).collect());
        }
        trim_outer_blank_rows(&mut rows);

        if rows.is_empty() {
            return Ok(DocumentConverterResult::new(""));
        }

        let width = rows[0].len();
        // Single output buffer, preallocated — avoids the Vec<String>+join
        // churn on multi-MB tables.
        let mut out = String::with_capacity(content.len() / 2 + 1024);

        out.push_str("| ");
        for (i, cell) in rows[0].iter().enumerate() {
            if i > 0 {
                out.push_str(" | ");
            }
            push_escaped(&mut out, cell);
        }
        out.push_str(" |\n");
        out.push_str("| ");
        for i in 0..width {
            if i > 0 {
                out.push_str(" | ");
            }
            out.push_str("---");
        }
        out.push_str(" |\n");

        for row in &rows[1..] {
            out.push_str("| ");
            let mut written = 0usize;
            // rows longer than the header are truncated to header width
            for cell in row.iter().take(width) {
                if written > 0 {
                    out.push_str(" | ");
                }
                push_escaped(&mut out, cell);
                written += 1;
            }
            while written < width {
                out.push_str(" | ");
                written += 1;
            }
            out.push_str(" |\n");
        }
        out.pop(); // trailing newline

        Ok(DocumentConverterResult::new(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_csv(input: &str) -> String {
        let info = StreamInfo {
            extension: Some(".csv".into()),
            ..Default::default()
        };
        CsvConverter
            .convert(
                &mut Cursor::new(input.as_bytes().to_vec()),
                &info,
            )
            .unwrap()
            .markdown
    }

    #[test]
    fn basic_table() {
        assert_eq!(
            convert_csv("name,age\nalice,30\nbob,25"),
            "| name | age |\n| --- | --- |\n| alice | 30 |\n| bob | 25 |"
        );
    }

    #[test]
    fn escapes_pipes_and_newlines() {
        assert_eq!(escape_table_cell("a|b"), r"a\|b");
        // backslash-run doubled, then the pipe escaped (Python regex semantics)
        assert_eq!(escape_table_cell(r"a\|b"), r"a\\\|b");
        assert_eq!(escape_table_cell("line1\nline2"), "line1 line2");
        assert_eq!(escape_table_cell("crlf\r\nx"), "crlf x");
    }

    #[test]
    fn blank_row_trimming() {
        // leading blank line, blank line after header, trailing blank lines
        assert_eq!(
            convert_csv("\n\na,b\n\n1,2\n\n\n"),
            "| a | b |\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn bom_stripped() {
        assert_eq!(
            convert_csv("\u{feff}a,b\n1,2"),
            "| a | b |\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn pads_and_truncates_to_header_width() {
        assert_eq!(
            convert_csv("a,b,c\n1\n1,2,3,4"),
            "| a | b | c |\n| --- | --- | --- |\n| 1 |  |  |\n| 1 | 2 | 3 |"
        );
    }

    #[test]
    fn empty_input_is_empty_markdown() {
        assert_eq!(convert_csv(""), "");
        assert_eq!(convert_csv("\n\n\n"), "");
    }

    #[test]
    fn quoted_fields() {
        assert_eq!(
            convert_csv("\"hello, world\",2\n\"say \"\"hi\"\"\",3"),
            "| hello, world | 2 |\n| --- | --- |\n| say \"hi\" | 3 |"
        );
    }

    #[test]
    fn accepts_checks() {
        let mut cursor = Cursor::new(Vec::new());
        let csv_info = StreamInfo {
            extension: Some(".CSV".into()),
            ..Default::default()
        };
        assert!(CsvConverter.accepts(&mut cursor, &csv_info));
        let mime_info = StreamInfo {
            mimetype: Some("text/csv; charset=utf-8".into()),
            ..Default::default()
        };
        assert!(CsvConverter.accepts(&mut cursor, &mime_info));
        let no_info = StreamInfo::default();
        assert!(!CsvConverter.accepts(&mut cursor, &no_info));
    }
}
