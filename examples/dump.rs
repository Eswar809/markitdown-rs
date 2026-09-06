//! Debug helper: convert a file with a specific converter and print Markdown.
//! Usage: dump <file> <extension> [charset]
//!   dump data.csv .csv utf-8

use std::io::Cursor;

use markitdown_rs::{CsvConverter, DocumentConverter, StreamInfo};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump <file> <extension> [charset]");
        std::process::exit(2);
    }
    let data = std::fs::read(&args[1]).expect("read failed");
    let info = StreamInfo {
        extension: Some(args[2].clone()),
        charset: args.get(3).cloned(),
        ..Default::default()
    };
    let result = CsvConverter
        .convert(&mut Cursor::new(data), &info)
        .expect("conversion failed");
    print!("{}", result.markdown);
}
