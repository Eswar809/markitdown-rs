//! markitdown-rs — a Rust port of [microsoft/markitdown](https://github.com/microsoft/markitdown):
//! convert documents (CSV, plain text today; DOCX/XLSX/PPTX/PDF next) into
//! Markdown for LLM and RAG pipelines.
//!
//! ```no_run
//! use markitdown_rs::{MarkItDown, StreamInfo};
//!
//! let md = MarkItDown::new()
//!     .convert_local("report.csv")
//!     .unwrap();
//! println!("{}", md.markdown);
//! ```

mod converter;
mod converters;

use std::io::{Cursor, Seek, SeekFrom};
use std::path::Path;

pub use converter::{
    DocumentConverter, DocumentConverterResult, FailedConversionAttempt, MarkitdownError,
    StreamInfo, PRIORITY_GENERIC_FILE_FORMAT, PRIORITY_SPECIFIC_FILE_FORMAT,
};
pub use converters::{CsvConverter, DocxConverter, HtmlConverter, PlainTextConverter, PptxConverter, XlsxConverter};

struct Registration {
    priority: f64,
    /// Registration order — the sort by priority is stable, so same-priority
    /// converters stay in registration order (later registrations tried
    /// first among equals), exactly like markitdown.
    order: usize,
    name: String,
    converter: Box<dyn DocumentConverter>,
}

/// The orchestrator: holds the converter chain and routes streams to the
/// first accepting converter — port of markitdown's `MarkItDown` class.
pub struct MarkItDown {
    converters: Vec<Registration>,
    next_order: usize,
}

impl Default for MarkItDown {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkItDown {
    /// Creates an instance with the built-in converter chain:
    /// specific formats (CSV, …) at SPECIFIC priority, catch-alls
    /// (PlainText) at GENERIC priority.
    pub fn new() -> Self {
        let mut md = Self {
            converters: Vec::new(),
            next_order: 0,
        };
        // Same registration order as markitdown's `_register_builtins`:
        // generic catch-alls first, then specific formats. Upstream's
        // register_converter inserts at index 0, so among equal priorities the
        // LATEST registration is tried first (HtmlConverter beats
        // PlainTextConverter); the sort below reproduces that.
        md.register(
            PRIORITY_GENERIC_FILE_FORMAT,
            Box::new(PlainTextConverter),
        );
        md.register(PRIORITY_GENERIC_FILE_FORMAT, Box::new(HtmlConverter));
        // specific formats — same set/order as upstream's builtin registration
        md.register(PRIORITY_SPECIFIC_FILE_FORMAT, Box::new(DocxConverter));
        md.register(PRIORITY_SPECIFIC_FILE_FORMAT, Box::new(XlsxConverter));
        md.register(PRIORITY_SPECIFIC_FILE_FORMAT, Box::new(PptxConverter));
        md.register(PRIORITY_SPECIFIC_FILE_FORMAT, Box::new(CsvConverter));
        md
    }

    /// Registers a converter. Lower priority values are tried first; ties
    /// break toward later registrations (stable sort, like markitdown).
    pub fn register(&mut self, priority: f64, converter: Box<dyn DocumentConverter>) {
        let name = converter.name().to_string();
        self.converters.push(Registration {
            priority,
            order: self.next_order,
            name,
            converter,
        });
        self.next_order += 1;
    }

    /// Converts an in-memory byte stream. `guesses` are tried in order, and
    /// an empty `StreamInfo` is appended as the last resort — mirroring
    /// markitdown's `_convert`.
    pub fn convert_stream(
        &self,
        data: Vec<u8>,
        guesses: &[StreamInfo],
    ) -> Result<DocumentConverterResult, MarkitdownError> {
        let mut cursor = Cursor::new(data);
        self.convert_stream_cursor(&mut cursor, guesses)
    }

    /// Same as [`convert_stream`] over an existing cursor (the position is
    /// rewound before each converter attempt).
    pub fn convert_stream_cursor(
        &self,
        mut cursor: &mut Cursor<Vec<u8>>,
        guesses: &[StreamInfo],
    ) -> Result<DocumentConverterResult, MarkitdownError> {
        let mut registrations = self.converters.iter().collect::<Vec<_>>();
        registrations.sort_by(|a, b| {
            a.priority
                .partial_cmp(&b.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
                // stable sort, but later registrations come first among
                // equals (upstream inserts at index 0)
                .then(b.order.cmp(&a.order))
        });

        let mut attempts: Vec<FailedConversionAttempt> = Vec::new();

        let all_guesses: Vec<StreamInfo> = guesses
            .iter()
            .cloned()
            .chain(std::iter::once(StreamInfo::default()))
            .collect();

        for stream_info in &all_guesses {
            for reg in &registrations {
                cursor.seek(SeekFrom::Start(0)).ok();

                let accepts = reg.converter.accepts(&mut cursor, stream_info);
                if accepts {
                    cursor.seek(SeekFrom::Start(0)).ok();
                    match reg.converter.convert(&mut cursor, stream_info) {
                        Ok(result) => return Ok(result),
                        Err(e) => attempts.push(FailedConversionAttempt {
                            converter_name: reg.name.clone(),
                            error: e.to_string(),
                        }),
                    }
                }
            }
        }

        Err(MarkitdownError {
            message: if attempts.is_empty() {
                "No converter accepted the input stream.".to_string()
            } else {
                "Accepted converters failed to convert the input stream.".to_string()
            },
            attempts,
        })
    }

    /// Converts a local file, guessing mimetype/extension from the path —
    /// port of `convert_local` (v1: extension-based mime guessing, no
    /// Magika-style content sniffing yet).
    pub fn convert_local(&self, path: impl AsRef<Path>) -> Result<DocumentConverterResult, MarkitdownError> {
        let path = path.as_ref();
        let data = std::fs::read(path).map_err(|e| MarkitdownError {
            message: format!("failed to read {}: {}", path.display(), e),
            attempts: Vec::new(),
        })?;

        let extension = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()));
        let mimetype = extension.as_deref().and_then(guess_mimetype);

        let info = StreamInfo {
            mimetype: mimetype.map(|m| m.to_string()),
            extension,
            charset: Some("utf-8".into()),
            filename: path.file_name().map(|f| f.to_string_lossy().to_string()),
            local_path: Some(path.to_string_lossy().to_string()),
            url: None,
        };
        self.convert_stream(data, &[info])
    }
}

/// Minimal mimetype map for local files (v1).
fn guess_mimetype(ext: &str) -> Option<&'static str> {
    Some(match ext {
        ".csv" => "text/csv",
        ".txt" | ".text" => "text/plain",
        ".md" | ".markdown" => "text/markdown",
        ".json" => "application/json",
        ".jsonl" => "application/jsonl",
        ".html" | ".htm" => "text/html",
        ".docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ".xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ".pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ".pdf" => "application/pdf",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_file_converts_via_local() {
        let dir = std::env::temp_dir().join("markitdown-rs-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.csv");
        std::fs::write(&path, b"name,age\nalice,30").unwrap();

        let result = MarkItDown::new().convert_local(&path).unwrap();
        assert_eq!(result.markdown, "| name | age |\n| --- | --- |\n| alice | 30 |");
    }

    #[test]
    fn plain_text_file_passes_through() {
        let dir = std::env::temp_dir().join("markitdown-rs-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.md");
        std::fs::write(&path, b"# Title\nbody").unwrap();

        let result = MarkItDown::new().convert_local(&path).unwrap();
        assert_eq!(result.markdown, "# Title\nbody");
    }

    #[test]
    fn unknown_binary_fails_with_attempts() {
        let md = MarkItDown::new();
        // no extension, random binary: plain text accepts (charset None but
        // empty StreamInfo guess has no charset either -> not accepted unless
        // extension/mime match). Empty info: no charset/ext/mime -> nothing
        // accepts -> Unhandled-style error.
        let err = md
            .convert_stream(vec![0xff, 0xfe, 0x00, 0x01], &[StreamInfo::default()])
            .unwrap_err();
        assert!(err.message.contains("No converter accepted"));
    }
}
