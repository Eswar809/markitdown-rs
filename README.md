# markitdown-rs ⚡

**A Rust port of [microsoft/markitdown](https://github.com/microsoft/markitdown)** (178k⭐) —
convert documents into Markdown for LLM and RAG pipelines, at native speed.

> Status: v0.2 — CSV, plain-text and **HTML** converters, **byte-for-byte parity with Python
> verified**. DOCX / XLSX / PPTX / PDF converters are being ported next.

## Benchmarks — Python vs Rust (in-process, best of N)

| Case | Python `markitdown` 0.1.8b1 | markitdown-rs | speedup |
|---|---:|---:|---:|
| CSV → MD (300k rows, 13 MB) | 1350–2013 ms | 195–325 ms | **~6.9x** |
| HTML → MD (3000 sections + tables + lists, 1.7 MB) | 5142 ms | **311 ms** | **16.5x** |

<sub>Best of N in-process runs on i5-12500H. Reproduce with `examples/bench.rs` + the
Python converters from the upstream repo.</sub>

**Why HTML is 16x:** the Python path is a BeautifulSoup DOM walk plus the pure-Python
`markdownify` transformer. The Rust port uses a hand-rolled lenient parser (modeled on
`html.parser` + BeautifulSoup semantics — no implicit `<tbody>`, same whitespace-text
sibling chain) and a direct port of the markdownify converter table, so output stays
identical while running at native speed.

## Why

markitdown is the standard "make any document LLM-ready" tool — but it's pure Python and
pulls a large dependency tree. This port keeps the exact same output (verified with a
differential parity suite against the Python implementation) while running faster with a
single small dependency.

## Usage

```rust
use markitdown_rs::MarkItDown;

let result = MarkItDown::new().convert_local("report.csv")?;   // or page.html
println!("{} ({:?})", result.markdown, result.title);
```

Output is identical to Python markitdown — including markdownify's whitespace/newline
collapsing, pipe-escaping rules, blank-row trimming, BOM stripping, autolink shortcuts,
`javascript:` link removal, data-URI truncation, checkbox inputs, colspan tables, and the
ATX heading style.

## Parity methodology (rustdate playbook)

- 25 Rust unit tests covering the tricky corners of all three converters
- **Differential parity suite** ([parity.py](parity.py)): 48 adversarial inputs (16 CSV +
  32 HTML) run through both the Python converters (from the upstream repo) and this crate —
  **all 48 outputs byte-identical**

## Roadmap

- [x] CSV converter (parity ✅)
- [x] Plain-text passthrough
- [x] HTML → Markdown (hand-rolled parser + full markdownify port, parity ✅)
- [ ] DOCX (zip + OOXML parsing, like upstream's hand-rolled `converter_utils/docx`)
- [ ] XLSX (spreadsheet → tables)
- [ ] PPTX (slides → sections)
- [ ] PDF (largest upstream converter — via pdfium bindings)
- [ ] Online converters (YouTube/Wikipedia/Bing) and MCP server — later

## Dev

```bash
cargo test                                   # unit tests
python parity.py <path-to-markitdown-clone>  # differential parity vs Python
cargo build --release --example bench
./target/release/examples/bench.exe big.csv .csv
```

The upstream repo is expected cloned next to this one (or pass its path to `parity.py`)
so the Python converters can be imported for comparison.

## License & attribution

MIT. A port of the MIT-licensed
[microsoft/markitdown](https://github.com/microsoft/markitdown) — all credit for the
design and conversion semantics belongs to the upstream project (and to the `markdownify`
library). Not affiliated with Microsoft.
