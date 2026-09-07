//! PDF → Markdown converter — port of `_pdf_converter.py` at the
//! orchestration level. The upstream converter tries a pdfplumber
//! word-geometry form/table heuristic per page and falls back to
//! `pdfminer.high_level.extract_text`; this port uses the `pdf-extract`
//! crate (a partial Rust pdfminer port) as the text engine.
//!
//! NOTE on parity: byte-identical output would require a full pdfminer.six
//! port (text operators, CMap/ToUnicode decoding, LAParams layout analysis).
//! Parity for PDFs is therefore verified at the upstream test-assertion
//! level instead (per-line rstrip + substring includes — the upstream suite
//! itself never compares PDF output byte-for-byte).

use std::io::{Cursor, Read};

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_PREFIX: &str = "application/pdf";
const ACCEPTED_FILE_EXTENSIONS: [&str; 1] = [".pdf"];

pub struct PdfConverter;

impl Default for PdfConverter {
    fn default() -> Self {
        Self
    }
}

impl DocumentConverter for PdfConverter {
    fn name(&self) -> &'static str {
        "PdfConverter"
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
        let text = pdf_extract::extract_text_from_mem(&data)
            .map_err(|e| ConverterError(format!("pdf text extraction failed: {}", e)))?;
        // upstream appends nothing and post-processes only with a numbering
        // merge that is a no-op for prose documents; the trailing \x0c page
        // separators come from the extraction engine itself
        Ok(DocumentConverterResult::new(text))
    }
}
