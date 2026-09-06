//! Built-in converters. v1: plain text + CSV (the offline, dependency-free
//! set). DOCX/XLSX/PPTX/PDF ports follow.

pub mod csv;
pub mod plain_text;

pub use csv::CsvConverter;
pub use plain_text::PlainTextConverter;
