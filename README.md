# markitdown-rs ⚡

**A Rust port of [microsoft/markitdown](https://github.com/microsoft/markitdown)** (178k⭐) —
convert documents into Markdown for LLM and RAG pipelines, at native speed.

> Status: v0.1 — CSV + plain-text converters, **byte-for-byte parity with Python verified**.
> DOCX / XLSX / PPTX / HTML / PDF converters are being ported next.

## Benchmark — Python vs Rust (CSV → Markdown)

| | Python `markitdown` 0.1.8b1 | markitdown-rs | speedup |
|---|---:|---:|---:|
| 300k-row CSV (13 MB, quoted/piped/unicode fields) | 1350 ms | **195 ms** | **6.9x** |

<sub>Best of N in-process runs on i5-12500H. Reproduce: `python bench_csv.py` (or parity.py + examples/bench).</sub>

## Why

markitdown is the standard "make any document LLM-ready" tool — but it's pure Python and
pulls a large dependency tree. This port keeps the exact same output (verified with a
differential parity suite against the Python implementation) while running ~7x faster with
a single small dependency.

## Usage

```rust
use markitdown_rs::MarkItDown;

let result = MarkItDown::new().convert_local("report.csv")?;
println!("{}", result.markdown);
```

Output for a CSV is a Markdown table, identical to Python markitdown — including the
pipe-escaping rules (`|` → `\|`, backslash runs doubled), blank-row trimming, BOM
stripping, and ragged-row padding/truncation.

## Parity methodology (rustdate playbook)

- 13 Rust unit tests covering the tricky corners: pipe/backslash escaping, newlines in
  fields, blank-line trimming, BOM, ragged rows, quoted fields
- **Differential parity suite** ([parity.py](parity.py)): 16 adversarial CSVs run through
  both the Python `CsvConverter` (from the upstream repo) and this crate —
  **all 16 outputs byte-identical**

## Roadmap

- [x] CSV converter (parity ✅)
- [x] Plain-text passthrough
- [ ] HTML → Markdown (`_html_converter` + `_markdownify` port)
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
./target/release/examples/bench.exe big.csv  # 13MB CSV timing
```

The upstream repo is expected cloned next to this one (or pass its path to `parity.py`)
so the Python converter can be imported for comparison.

## License & attribution

MIT. A clean-room-style port of the MIT-licensed
[microsoft/markitdown](https://github.com/microsoft/markitdown) — all credit for the
design and conversion semantics belongs to the upstream project. Not affiliated with Microsoft.
