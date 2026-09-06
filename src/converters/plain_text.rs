//! Plain-text converter — port of `_plain_text_converter.py`.
//! The near catch-all: anything with a charset, a text-ish extension, or a
//! text-ish mimetype passes through unchanged. Registered at GENERIC priority
//! so specific converters get first dibs.

use std::io::Cursor;

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_TYPE_PREFIXES: [&str; 3] = ["text/", "application/json", "application/markdown"];
const ACCEPTED_FILE_EXTENSIONS: [&str; 6] = [".txt", ".text", ".md", ".markdown", ".json", ".jsonl"];

pub struct PlainTextConverter;

impl Default for PlainTextConverter {
    fn default() -> Self {
        Self
    }
}

impl DocumentConverter for PlainTextConverter {
    fn name(&self) -> &'static str {
        "PlainTextConverter"
    }

    fn accepts(&self, _stream: &mut Cursor<Vec<u8>>, stream_info: &StreamInfo) -> bool {
        // If we have a charset, we can safely assume it's text
        if stream_info.charset.is_some() {
            return true;
        }
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
        // v1: UTF-8 lossy (charset_normalizer-style detection is a TODO)
        let _ = stream_info.charset;
        let text = String::from_utf8_lossy(stream.get_ref());
        Ok(DocumentConverterResult::new(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_implies_accept() {
        let mut cursor = Cursor::new(Vec::new());
        let info = StreamInfo {
            charset: Some("utf-8".into()),
            ..Default::default()
        };
        assert!(PlainTextConverter.accepts(&mut cursor, &info));
    }

    #[test]
    fn passes_text_through() {
        let info = StreamInfo {
            extension: Some(".md".into()),
            ..Default::default()
        };
        let res = PlainTextConverter
            .convert(&mut Cursor::new("# Hello\nworld".as_bytes().to_vec()), &info)
            .unwrap();
        assert_eq!(res.markdown, "# Hello\nworld");
        assert_eq!(res.title, None);
    }
}
