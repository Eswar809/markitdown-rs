//! Benchmark helper: convert <file> through the full converter chain N times
//! in-process, print best time in ms. Usage: bench <file> <extension>

use std::io::Cursor;

use markitdown_rs::{DocumentConverter, MarkItDown, StreamInfo};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("read failed");
    let ext = args.get(2).cloned().unwrap_or_else(|| ".csv".into());
    let info = StreamInfo {
        extension: Some(ext),
        charset: Some("utf-8".into()),
        ..Default::default()
    };
    let md = MarkItDown::new();
    let mut cursor = Cursor::new(data);
    let mut best = f64::MAX;
    let mut sink = 0usize;
    for _ in 0..10 {
        cursor.set_position(0);
        let t = std::time::Instant::now();
        let res = md.convert_stream_cursor(&mut cursor, &[info.clone()]).expect("convert failed");
        sink = sink.max(res.markdown.len());
        let d = t.elapsed().as_secs_f64();
        if d < best {
            best = d;
        }
    }
    eprintln!("sink {} bytes", sink);
    println!("{:.2}", best * 1000.0);
}
