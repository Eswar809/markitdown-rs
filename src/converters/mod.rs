//! Built-in converters. Ported so far: plain text, CSV, HTML, DOCX.
//! XLSX/PPTX/PDF ports follow.

pub mod csv;
pub mod docx;
pub(crate) mod docx_math;
pub mod html;
pub mod plain_text;
pub mod pptx;
pub mod xlsx;

pub use csv::CsvConverter;
pub use docx::DocxConverter;
pub use html::HtmlConverter;
pub use plain_text::PlainTextConverter;
pub use pptx::PptxConverter;
pub use xlsx::XlsxConverter;
