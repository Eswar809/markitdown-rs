//! XLSX → Markdown converter — port of `_xlsx_converter.py`.
//!
//! Upstream pipeline: openpyxl/pandas (`pd.read_excel`) → `DataFrame.to_html`
//! → HtmlConverter. This port reads the OOXML parts directly (workbook,
//! sharedStrings, worksheets) and reproduces the pandas semantics that reach
//! the output: first row = header, "Unnamed: N" for missing headers, ".N"
//! suffixes for duplicate headers, integral numbers without decimals, empty
//! cells as NaN (rendered as ""), and `to_html(index=False)` table markup.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use super::super::converter::{
    ConverterError, DocumentConverter, DocumentConverterResult, StreamInfo,
};

const ACCEPTED_MIME_PREFIX: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const ACCEPTED_FILE_EXTENSIONS: [&str; 1] = [".xlsx"];

pub struct XlsxConverter;

impl Default for XlsxConverter {
    fn default() -> Self {
        Self
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// "A1" → 0, "AA2" → 26 (column index from the cell reference letters).
fn col_index(cell_ref: &str) -> usize {
    let mut idx = 0usize;
    for c in cell_ref.chars() {
        if c.is_ascii_uppercase() {
            idx = idx * 26 + (c as usize - 'A' as usize + 1);
        } else {
            break;
        }
    }
    idx.saturating_sub(1)
}

/// Cell value formatting matching openpyxl → pandas → to_html:
/// integral floats render without decimals (openpyxl gives ints), text as-is.
fn format_number(v: &str) -> String {
    if let Ok(i) = v.parse::<i64>() {
        return i.to_string();
    }
    match v.parse::<f64>() {
        Ok(f) => {
            if f.fract() == 0.0 && f.abs() < 1e16 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            }
        }
        Err(_) => v.to_string(),
    }
}

struct Sheet {
    name: String,
    target: String,
}

struct Cell {
    col: usize,
    value: Option<String>, // None = empty (NaN)
}

fn parse_sheet_xml(xml: &str, shared: &[String]) -> Vec<Vec<Cell>> {
    let Ok(tree) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let ns_main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    for row in tree.descendants().filter(|n| {
        n.tag_name().namespace() == Some(ns_main) && n.tag_name().name() == "row"
    }) {
        let mut cells: Vec<Cell> = Vec::new();
        for c in row.children().filter(|n| {
            n.tag_name().namespace() == Some(ns_main) && n.tag_name().name() == "c"
        }) {
            let col = c
                .attribute("r")
                .map(col_index)
                .unwrap_or_else(|| cells.len());
            let t = c.attribute("t").unwrap_or("n");
            let value = if t == "inlineStr" {
                let text: String = c
                    .descendants()
                    .filter(|n| {
                        n.tag_name().namespace() == Some(ns_main)
                            && n.tag_name().name() == "t"
                    })
                    .filter_map(|n| n.text())
                    .collect();
                Some(text)
            } else {
                let Some(v) = c
                    .children()
                    .find(|&n| {
                        n.tag_name().namespace() == Some(ns_main)
                            && n.tag_name().name() == "v"
                    })
                    .and_then(|n| n.text())
                else {
                    continue; // formula-only cell without a cached value
                };
                Some(match t {
                    "s" => match shared.get(v.parse::<usize>().unwrap_or(usize::MAX)) {
                        Some(s) => s.clone(),
                        None => continue,
                    },
                    "b" => {
                        if v == "1" {
                            "True".to_string()
                        } else {
                            "False".to_string()
                        }
                    }
                    "str" => v.to_string(), // formula string result
                    "e" => v.to_string(),   // error like #DIV/0!
                    _ => format_number(v),  // numeric
                })
            };
            cells.push(Cell { col, value });
        }
        rows.push(cells);
    }
    rows
}

/// Reproduces pandas column naming: missing headers become "Unnamed: {i}",
/// duplicates get ".1"/".2" suffixes (first occurrence keeps the bare name).
fn build_columns(header: &[Option<String>], width: usize) -> Vec<String> {
    let mut cols = Vec::with_capacity(width);
    for i in 0..width {
        let base = header
            .get(i)
            .and_then(|v| v.clone())
            .unwrap_or_else(|| format!("Unnamed: {}", i));
        cols.push(base);
    }
    // pandas mangle_dupe_cols
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for col in cols.iter_mut() {
        let count = seen.entry(col.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            let mut n = *count - 1;
            let mut candidate = format!("{}.{}", col, n);
            while used.contains(&candidate) {
                n += 1;
                candidate = format!("{}.{}", col, n);
            }
            *col = candidate;
        }
        used.insert(col.clone());
    }
    cols
}

impl DocumentConverter for XlsxConverter {
    fn name(&self) -> &'static str {
        "XlsxConverter"
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
        let mut archive = zip::ZipArchive::new(Cursor::new(&data[..]))
            .map_err(|e| ConverterError(format!("not a valid xlsx zip: {}", e)))?;

        // shared strings
        let mut shared: Vec<String> = Vec::new();
        if let Ok(mut f) = archive.by_name("xl/sharedStrings.xml") {
            let mut xml = String::new();
            let _ = f.read_to_string(&mut xml);
            if let Ok(tree) = roxmltree::Document::parse(&xml) {
                const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
                for si in tree.descendants().filter(|n| {
                    n.tag_name().namespace() == Some(NS) && n.tag_name().name() == "si"
                }) {
                    // concatenate all <t> descendants (rich text runs)
                    let text: String = si
                        .descendants()
                        .filter(|n| {
                            n.tag_name().namespace() == Some(NS)
                                && n.tag_name().name() == "t"
                        })
                        .filter_map(|n| n.text())
                        .collect();
                    shared.push(text);
                }
            }
        }

        // workbook sheet order + names
        let workbook_xml = {
            let mut f = archive
                .by_name("xl/workbook.xml")
                .map_err(|e| ConverterError(format!("workbook.xml: {}", e)))?;
            let mut xml = String::new();
            let _ = f.read_to_string(&mut xml);
            xml
        };
        let rels_xml = {
            let mut f = archive
                .by_name("xl/_rels/workbook.xml.rels")
                .map_err(|e| ConverterError(format!("workbook rels: {}", e)))?;
            let mut xml = String::new();
            let _ = f.read_to_string(&mut xml);
            xml
        };
        let wb_tree = roxmltree::Document::parse(&workbook_xml)
            .map_err(|e| ConverterError(format!("workbook.xml parse: {}", e)))?;
        let rel_tree = roxmltree::Document::parse(&rels_xml)
            .map_err(|e| ConverterError(format!("workbook rels parse: {}", e)))?;

        const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        const NS_R: &str =
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let mut sheets: Vec<Sheet> = Vec::new();
        for node in wb_tree.descendants() {
            if node.tag_name().namespace() == Some(NS_MAIN)
                && node.tag_name().name() == "sheet"
            {
                let name = node.attribute("name").unwrap_or("Sheet").to_string();
                if let Some(rid) = node.attribute((NS_R, "id")) {
                    sheets.push(Sheet {
                        name,
                        target: rid.to_string(),
                    });
                }
            }
        }
        let mut rid_to_target: HashMap<String, String> = HashMap::new();
        for node in rel_tree.descendants() {
            if node.tag_name().name() == "Relationship" {
                if let (Some(id), Some(target)) =
                    (node.attribute("Id"), node.attribute("Target"))
                {
                    rid_to_target.insert(id.to_string(), target.to_string());
                }
            }
        }

        // build markdown: "## {sheet}\n" + pandas-to_html table + blank line
        let mut md = String::new();
        for sheet in &sheets {
            let target = rid_to_target
                .get(&sheet.target)
                .map(|t| {
                    if let Some(stripped) = t.strip_prefix('/') {
                        stripped.to_string()
                    } else {
                        format!("xl/{}", t)
                    }
                })
                .ok_or_else(|| {
                    ConverterError(format!(
                        "missing worksheet target for {}",
                        sheet.name
                    ))
                })?;

            let sheet_xml = {
                let mut f = archive
                    .by_name(&target)
                    .map_err(|e| ConverterError(format!("{}: {}", target, e)))?;
                let mut xml = String::new();
                let _ = f.read_to_string(&mut xml);
                xml
            };
            let rows = parse_sheet_xml(&sheet_xml, &shared);
            if rows.is_empty() {
                continue;
            }

            let width = rows
                .iter()
                .flat_map(|r| r.iter().map(|c| c.col + 1))
                .max()
                .unwrap_or(0);

            // grid[row][col] with None for empty (NaN) cells
            let mut grid: Vec<Vec<Option<String>>> =
                vec![vec![None; width]; rows.len()];
            for (ri, row) in rows.iter().enumerate() {
                for cell in row {
                    if cell.col < width {
                        grid[ri][cell.col] = cell.value.clone();
                    }
                }
            }

            // pandas drops leading fully-empty rows before the header row
            while grid.first().map(|r| r.iter().all(Option::is_none)).unwrap_or(false) {
                grid.remove(0);
            }
            if grid.is_empty() {
                continue;
            }

            let header = &grid[0];
            let columns = build_columns(header, width);
            let data_rows = &grid[1..];

            // pandas to_html(index=False)
            let mut html = String::from(
                "<table border=\"1\" class=\"dataframe\">\n  <thead>\n    <tr style=\"text-align: right;\">\n",
            );
            for col in &columns {
                html.push_str(&format!("      <th>{}</th>\n", html_escape(col)));
            }
            html.push_str("    </tr>\n  </thead>\n  <tbody>\n");
            for row in data_rows {
                html.push_str("    <tr>\n");
                for i in 0..width {
                    let v = row
                        .get(i)
                        .and_then(|v| v.clone())
                        .unwrap_or_default(); // NaN → na_rep ""
                    html.push_str(&format!("      <td>{}</td>\n", html_escape(&v)));
                }
                html.push_str("    </tr>\n");
            }
            html.push_str("  </tbody>\n</table>");

            let table_md = super::html::convert_html_string(&html)?
                .markdown
                .trim()
                .to_string();
            md.push_str(&format!("## {}\n", sheet.name));
            md.push_str(&table_md);
            md.push_str("\n\n");
        }

        Ok(DocumentConverterResult::new(md.trim()))
    }
}
