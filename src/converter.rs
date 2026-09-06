//! Core types: StreamInfo, converter result, and the DocumentConverter trait —
//! a direct port of markitdown's `_base_converter.py` and `_stream_info.py`.

use std::fmt;
use std::io::Cursor;

/// Metadata about the byte stream being converted. All fields optional,
/// exactly like markitdown's `StreamInfo` dataclass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamInfo {
    pub mimetype: Option<String>,
    pub extension: Option<String>,
    pub charset: Option<String>,
    /// From a local path, url, or Content-Disposition header.
    pub filename: Option<String>,
    /// Set when read from disk.
    pub local_path: Option<String>,
    /// Set when read from a url.
    pub url: Option<String>,
}

impl StreamInfo {
    /// `self.copy_and_update(other)`: other's non-None fields win.
    pub fn copy_and_update(&self, other: &StreamInfo) -> StreamInfo {
        StreamInfo {
            mimetype: other.mimetype.clone().or_else(|| self.mimetype.clone()),
            extension: other.extension.clone().or_else(|| self.extension.clone()),
            charset: other.charset.clone().or_else(|| self.charset.clone()),
            filename: other.filename.clone().or_else(|| self.filename.clone()),
            local_path: other.local_path.clone().or_else(|| self.local_path.clone()),
            url: other.url.clone().or_else(|| self.url.clone()),
        }
    }
}

/// The result of converting a document to Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentConverterResult {
    pub title: Option<String>,
    pub markdown: String,
}

impl DocumentConverterResult {
    pub fn new(markdown: impl Into<String>) -> Self {
        Self {
            title: None,
            markdown: markdown.into(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A converter failure with its identity, for aggregated error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedConversionAttempt {
    pub converter_name: String,
    pub error: String,
}

/// Errors from the top-level convert flow, mirroring markitdown's
/// UnhandledMimeTypeException / FileConversionException semantics.
#[derive(Debug, Clone, Default)]
pub struct MarkitdownError {
    pub message: String,
    pub attempts: Vec<FailedConversionAttempt>,
}

impl fmt::Display for MarkitdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        for a in &self.attempts {
            write!(f, "\n  [{}] {}", a.converter_name, a.error)?;
        }
        Ok(())
    }
}

impl std::error::Error for MarkitdownError {}

/// Per-converter failure used inside `convert` implementations.
#[derive(Debug, Clone, PartialEq)]
pub struct ConverterError(pub String);

impl fmt::Display for ConverterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConverterError {}

/// Abstract superclass of all document converters — port of
/// `DocumentConverter`. The stream is a seekable in-memory cursor; the
/// orchestrator rewinds it to the start before every `accepts`/`convert`
/// call (Python asserts converters preserve the position; rewinding makes
/// that contract trivially hold).
pub trait DocumentConverter: Send + Sync {
    /// Converter name for error reporting (e.g. "CsvConverter").
    fn name(&self) -> &'static str;

    /// Quick check (mimetype/extension based) whether this converter should
    /// attempt the document.
    fn accepts(&self, stream: &mut Cursor<Vec<u8>>, stream_info: &StreamInfo) -> bool;

    /// Convert the document to Markdown.
    fn convert(
        &self,
        stream: &mut Cursor<Vec<u8>>,
        stream_info: &StreamInfo,
    ) -> Result<DocumentConverterResult, ConverterError>;
}

/// Lower priority values are tried first (same constants as markitdown).
pub const PRIORITY_SPECIFIC_FILE_FORMAT: f64 = 0.0;
pub const PRIORITY_GENERIC_FILE_FORMAT: f64 = 10.0;
