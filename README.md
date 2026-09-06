# markitdown-rs ⚡

**A Rust port of [microsoft/markitdown](https://github.com/microsoft/markitdown)** (178k⭐) —
convert documents into Markdown for LLM and RAG pipelines, at native speed.

> Status: v0.5 — CSV, plain-text, **HTML**, **DOCX** (OMML math → LaTeX), **XLSX** and
> **PPTX** (charts, groups, speaker notes) converters, **byte-for-byte parity with Python
> verified**. PDF converter is being ported next.

## Benchmarks — Python vs Rust (in-process, best of N)

| Case | Python `markitdown` 0.1.8b1 | markitdown-rs | speedup |
|---|---:|---:|---:|
| CSV → MD (300k rows, 13 MB) | 1350–2013 ms | 195–325 ms | **~6.9x** |
| HTML → MD (3000 sections + tables + lists, 1.7 MB) | 5142 ms | **311 ms** | **16.5x** |
| DOCX → MD (AutoGen paper: headings, table, image) | 104 ms | **1.40 ms** | **74x** |
| XLSX → MD (2 sheets, tables) | 20.4 ms | **1.05 ms** | **19.5x** |
| PPTX → MD (6 slides: chart, table, picture, groups) | 22.2 ms | **2.00 ms** | **11.1x** |

<sub>Best of N in-process runs on i5-12500H. Reproduce with `examples/bench.rs` + the
Python converters from the upstream repo.</sub>

**Why DOCX is 74x:** upstream runs a full `pre_process_docx` (BeautifulSoup XML
re-serialization) + **mammoth** (docx → HTML) + markdownify. The Rust port parses the
OOXML parts directly (`roxmltree` + zip), generates the intermediate HTML in one pass —
including the OMML equation → LaTeX conversion — and feeds the same markdownify pipeline.
**All four upstream sample documents (including `equations.docx` math) convert
byte-identically.**

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
`javascript:` link removal, data-URI truncation, checkbox inputs, colspan tables, the
ATX heading style, DOCX heading styles (resolved via style NAME), embedded images with
alt text, OMML equations rendered as `$...$` / `$$...$$` LaTeX, and PPTX slide
comments with shape-order sorting, chart pipe-tables (`2000.0` float semantics) and
placeholder picture filenames.

## Parity methodology (rustdate playbook)

- 25 Rust unit tests covering the tricky corners of all converters
- **Differential parity suite** ([parity.py](parity.py)): 54 inputs (16 CSV + 32 HTML +
  4 real DOCX documents including the OMML math document + 1 XLSX workbook + 1 PPTX deck
  with chart/table/picture/groups — all from the upstream test suite) run through both
  the Python converters (from the upstream repo) and this crate —
  **all 54 outputs byte-identical**

## Roadmap

- [x] CSV converter (parity ✅)
- [x] Plain-text passthrough
- [x] HTML → Markdown (hand-rolled parser + full markdownify port, parity ✅)
- [x] DOCX → HTML → Markdown (OOXML + style-name headings + tables + images +
      OMML math → LaTeX, parity ✅ on all upstream samples)
- [x] XLSX → Markdown (hand-rolled OOXML parse + pandas `to_html`/column-naming
      semantics — "Unnamed: N" headers, ".N" duplicate suffixes, integral
      numbers, shared strings, parity ✅)
- [x] PPTX → Markdown (slide comments, (top,left) shape ordering with -inf
      quirk, titles, text frames, pictures with sanitized alt + placeholder
      filenames, table HTML round-trip, charts with Python float str
      semantics (`2000.0`), group recursion, speaker notes, parity ✅)
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
