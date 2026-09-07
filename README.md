# markitdown-rs ⚡

**A Rust port of [microsoft/markitdown](https://github.com/microsoft/markitdown)** (178k⭐) —
convert documents into Markdown for LLM and RAG pipelines, at native speed.

> Status: v0.6 — CSV, plain-text, **HTML**, **DOCX** (OMML math → LaTeX), **XLSX** and
> **PPTX** (charts, groups, speaker notes) converters, **byte-for-byte parity with Python
> verified**. PDF converter included at **assertion-level parity** (see PDF section).

## Benchmarks — Python vs Rust (single session, in-process, best of N)

| Case | Python `markitdown` 0.1.8b1 | markitdown-rs | speedup |
|---|---:|---:|---:|
| CSV → MD (300k rows, 13 MB) | 2199 ms | 348 ms | **6.3x** |
| HTML → MD (3000 sections + tables + lists, 1.7 MB) | 6002 ms | **297 ms** | **20.2x** |
| DOCX → MD (AutoGen paper: headings, table, image) | 66 ms | **1.65 ms** | **40x** |
| XLSX → MD (2 sheets, tables) | 19.4 ms | **0.94 ms** | **20.6x** |
| PPTX → MD (6 slides: chart, table, picture, groups) | 23.9 ms | **1.33 ms** | **18.0x** |
| PDF → text (test.pdf, prose)* | ~35 ms | ~25 ms | engine-dependent* |

<sub>All five rows measured back-to-back in a single session on i5-12500H (best of N
in-process). Regenerate with `python plot_bench.py`. *PDF uses a different extraction
engine — see the PDF section below.</sub>

![Python markitdown vs markitdown-rs benchmark](benchmark.png)

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

## PDF — assertion-level parity (honest notes)

Upstream extracts PDF text with **pdfminer.six** (with a pdfplumber word-geometry
borderless-table heuristic before it). Byte-identical output requires porting pdfminer's
entire text engine (text operators, CMap/ToUnicode decoding, LAParams layout grouping) —
out of scope here, so the port uses the **pdf-extract** crate (a partial Rust pdfminer
port). Parity is therefore measured in the upstream test's own style (per-line `rstrip`
+ ±2 line tolerance):

| Sample | result |
|---|---|
| MEDRPT (scanned, no text layer) | ✅ strict (both empty) |
| RECEIPT (retail purchase) | CLOSE — same line count, sim 1.00, minor intra-line spacing |
| test.pdf (prose paper) | CLOSE — sim 1.00, ±1 line (engine merges two header boxes) |
| REPAIR (multipage) | PARTIAL — pdf-extract line grouping differs (sim 0.58) |
| SPARSE (borderless table) | PARTIAL — upstream emits pipe tables via its form heuristic (not ported) |

Upstream's own PDF tests are not byte-exact either: they use substring `must_include`
checks and per-line-rstrip full-output comparison with a ±2 line tolerance — under those
rules 4/5 samples pass today (REPAIR/SPARSE need the borderless-table heuristic ported).

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
- [~] PDF → text (**assertion-level parity**, see below; byte parity needs a
      full pdfminer.six port)
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
