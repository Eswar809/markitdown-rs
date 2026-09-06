//! Benchmark helper: convert <file> 10x in-process, print best time in ms.
//! Usage: bench <file>

use std::io::Cursor;

use markitdown_rs::{CsvConverter, DocumentConverter, StreamInfo};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("read failed");
    let info = StreamInfo {
        extension: Some(".csv".into()),
        charset: Some("utf-8".into()),
        ..Default::default()
    };
    let mut cursor = Cursor::new(data);
    let mut best = f64::MAX;
    let mut sink = 0usize;
    for _ in 0..10 {
        cursor.set_position(0);
        let t = std::time::Instant::now();
        let res = CsvConverter.convert(&mut cursor, &info).expect("convert failed");
        sink = sink.max(res.markdown.len());
        let d = t.elapsed().as_secs_f64();
        if d < best {
            best = d;
        }
    }
    eprintln!("sink {} bytes", sink);
    println!("{:.2}", best * 1000.0);
}
