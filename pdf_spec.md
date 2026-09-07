# markitdown PDF Converter — Behavioral Spec for Rust Port

Source of truth: `markitdown/packages/markitdown/src/markitdown/converters/_pdf_converter.py`
(upstream repo at `C:\Users\deevi\.zcode\workspace\default\markitdown`).
Dependencies used: `pdfplumber` 0.11.10 (which wraps `pdfminer.six` 20260107 for layout analysis).

## 1. Pipeline overview

```
accepts(ext/mime)
  └─ convert(bytes)
       ├─ 1. pdfplumber.open(BytesIO)            # pdfminer layout analysis under the hood
       ├─ 2. for each page:
       │      page_content = _extract_form_content_from_words(page)
       │      ├─ non-None  → "form page": append page_content (markdown w/ pipe tables)
       │      └─ None      → plain page: append page.extract_text().strip()
       │      page.close()
       ├─ 3. if form_page_count == 0:
       │      markdown = pdfminer.high_level.extract_text(pdf_bytes)   # WHOLE doc, one call
       │    else:
       │      markdown = "\n\n".join(markdown_chunks).strip()
       ├─ 4. on ANY pdfplumber exception → pdfminer.high_level.extract_text(pdf_bytes)
       ├─ 5. if markdown still empty      → pdfminer.high_level.extract_text(pdf_bytes)
       └─ 6. markdown = _merge_partial_numbering_lines(markdown)
             return DocumentConverterResult(markdown)   # plain markdown string, no wrapper
```

Key facts:

- **No HTML is ever built.** The converter emits markdown directly (strings + pipe tables).
  The Rust `convert_html_string` pipeline is NOT involved for PDFs.
- **Two extraction backends, both pdfminer-based:** pdfplumber (word-level API for the form
  heuristic + its own `extract_text`) and raw `pdfminer.high_level.extract_text` for the
  whole-document path.
- The converter itself adds **no page separators**. The `\x0c` (form feed) appearing in output
  comes from pdfminer's `extract_text`, which appends `\x0c` after **every** page (including the
  last one).
- The whole-doc pdfminer path is **not** `.strip()`ed; the pdfplumber per-page path strips each
  page's text and the joined result is `.strip()`ed.

## 2. Function-by-function behavior

### `accepts(file_stream, stream_info) -> bool`
True if `stream_info.extension` (lowercased) is `.pdf`, or `stream_info.mimetype` (lowercased)
starts with `application/pdf` or `application/x-pdf`.

### `convert(file_stream, stream_info) -> DocumentConverterResult`
1. Raises `MissingDependencyException` if pdfminer/pdfplumber failed to import at module load.
2. Copies the stream into `io.BytesIO` (pdfplumber requires a seekable stream).
3. Single pass over `pdfplumber.open(...).pages`:
   - Calls `_extract_form_content_from_words(page)`.
   - Non-None → count as form page; append its string if non-blank after `.strip()`.
   - None → plain page: `page.extract_text()` (pdfplumber default LAParams), append
     `text.strip()` if non-blank.
   - `page.close()` after each page (memory hygiene only, no output effect).
4. If **zero** pages were form-style → discard the chunks and re-run the whole document through
   `pdfminer.high_level.extract_text(pdf_bytes)`. This is the "prose" path — pdfminer's own text
   spacing beats pdfplumber's for prose.
5. Else join chunks with `"\n\n"` and `.strip()`.
6. Any exception anywhere in the pdfplumber block → whole-doc
   `pdfminer.high_level.extract_text` fallback.
7. Empty result → one more `pdfminer.high_level.extract_text` attempt.
8. Post-process via `_merge_partial_numbering_lines`.
9. Return the string as-is. **No trailing strip, no header/footer, no page markers added.**

### `_extract_form_content_from_words(page) -> str | None` (borderless form/table heuristic)
Operates purely on word geometry; never inspects rules/lines:
1. `words = page.extract_words(keep_blank_chars=True, x_tolerance=3, y_tolerance=3)` (pdfplumber).
   Empty → return None.
2. Row grouping: bucket words by `y_key = round(word["top"] / 5) * 5` (y_tolerance = 5).
3. Per row (sorted by y, words sorted by x0): record
   - `line_width = last_x1 - first_x0`, `combined_text = " ".join(word texts)`,
   - `x_groups`: greedy clustering of word x0 values, new group when gap > 50,
   - `is_paragraph = line_width > page.width * 0.55 and len(combined_text) > 60`,
   - `has_partial_numbering = PARTIAL_NUMBERING_PATTERN.match(first_word_text)` where the
     pattern is `^\.\d+$` (e.g. ".1", ".2", ".10").
4. Collect x0 positions from rows with `num_columns >= 3` and not paragraph. If none → **None**.
5. Adaptive column tolerance: sort all collected x positions; take gaps > 5; if >= 3 gaps use the
   70th-percentile gap clamped to [25, 50], else 35. Cluster into `global_columns` (gap >
   adaptive_tolerance starts a new column).
6. Guard rails (each returns None → plain-text page):
   - avg column width < 30 pt,
   - columns-per-inch = len(columns) / (content_width / 72) > 10,
   - len(global_columns) > max(15, int(20 * page_width / 612)),
   - fewer than 2 global columns.
7. Row classification: a row is a table row iff not paragraph, not partial-numbering, and its
   words align (|word.x0 − col_x| < 40) with **>= 2** global columns.
8. Consecutive table rows form regions; regions must cover >= 20% of rows, else **None**.
9. Output (markdown built directly, no HTML):
   - Table region → pipe table: `| ` + cells `.ljust(col_width)` joined by `" | "` + ` |`,
     first row = header, then `| ` + `-`*width separator row, then data rows. Column widths =
     max cell length per column over the region (min 3). Cell extraction: word assigned to
     column i if `word_x < global_columns[i+1] - 20`, else last column; words in same cell
     joined with a single space.
   - Non-table row → its `combined_text` line.
   Rows are joined with `\n` (tables and prose interleaved in y order).

### `_merge_partial_numbering_lines(text) -> str`
Split on `\n`. If a line's `.strip()` matches `^\.\d+$` exactly (a MasterFormat partial number
alone on a line), merge it with the **next non-empty** line as `".N <next_line>"` (skipping blank
lines in between); lines otherwise pass through unchanged. Applied to the final markdown for all
paths.

### Dead code (defined but never called — safe to skip entirely)
- `_to_markdown_table(table, include_separator)` — generic 2D list → aligned pipe table.
- `_extract_tables_from_words(page)` — older word-position table detector (x-cluster tolerance
  20, 3–10 columns, >= 2 non-empty cells per row, >= 3 rows, <= 30% long cells).
Verified: zero references anywhere in the package outside their definitions.

## 3. Which pdfminer/pdfplumber APIs are used

| API | Where | Notes |
|---|---|---|
| `pdfminer.high_level.extract_text(BytesIO)` | whole-doc path + all fallbacks | **No parameters**: default LAParams (char_margin=2.0, line_margin=0.5, word_margin=0.1, boxes_flow=0.5), no page_numbers, no laparams override, no codec, no maxpages |
| `pdfplumber.open(BytesIO)` | main path | `.pages` iteration, `page.width` |
| `page.extract_words(keep_blank_chars=True, x_tolerance=3, y_tolerance=3)` | form heuristic | word-level geometry: x0, x1, top, text |
| `page.extract_text()` | plain-page path (discarded unless a form page exists elsewhere in doc) | pdfplumber defaults |
| `page.close()` | after each page | no output effect |

NOT used: custom PDFDevice/interceptors, character-level (LTChar) access, pdfplumber
`extract_tables()` (line/bbox based), table settings, password handling, page ranges, margins.
Everything above the word layer is pdfminer's default layout analysis (LAParams clustering of
LTChar → LTTextLine → LTTextBox, reading order via boxes_flow=0.5).

## 4. Exact output format details

- **Whole-doc (prose) path — the common case:** output is exactly
  `pdfminer.high_level.extract_text(...)` (default LAParams) followed by the partial-numbering
  merge. Pages are separated by pdfminer with `\x0c` **appended after each page** — so a 1-page
  PDF ends with `\x0c`, and N-page docs have `\x0c` between pages (also after the last).
  Blank lines (`\n\n`) between blocks are pdfminer's textbox boundaries; single `\n` between
  lines within a text box. No markdown constructs are produced on this path — plain text.
- **Form-page path:** chunks joined by `\n\n`, whole result `.strip()`ed (so no trailing `\x0c`).
  Tables rendered as GitHub-style pipe tables with left-justified padding and `| --- |`-style
  separators (leading/trailing spaces inside cells: `| ` prefix, ` |` suffix, ` | ` between).
- **Never produced:** headings (`#`), emphasis, links, images. Heading-looking lines like
  "1\n\nIntroduction" stay plain text (see ground truth).

## 5. test.pdf — what it contains and what it exercises

- 1 page, 612 x 792 pt (US Letter). An excerpt of the Microsoft AutoGen multi-agent paper
  (arXiv 2308.08155): section "1 Introduction", two long prose paragraphs, numbered concepts
  1 and 2 (as plain lines "1" / "2" plus indented body), a footnote line
  "3We refer to Appendix A for a detailed discussion.", and the paper page number "2" at the
  bottom.
- Path taken: `_extract_form_content_from_words` returns **None** (no 3+ column rows) →
  `form_page_count == 0` → **whole-doc pdfminer path only**.
- Features exercised: pdfminer default-LAParams text extraction, unicode ligature/quote
  handling ("framework’s"), pdfminer page-trailing `\x0c`, and `_merge_partial_numbering_lines`
  (a no-op here — no `.N` lines exist).
- Features NOT exercised: form/pipe-table output, per-page pdfplumber `extract_text` chunks,
  exception fallbacks, multi-page joining, dead-code helpers.

## 6. Ground truth (measured)

File: `target/parity/py_test_pdf.md` (UTF-8, LF endings, written with `newline=''` so no CRLF
translation).

- 5195 characters. First 200 chars:
  `'1\n\nIntroduction\n\nLarge language models (LLMs) are becoming a crucial building block in developing powerful agents\nthat utilize LLMs for reasoning, tool usage, and adapting to new observations (Yao et '`
- Last bytes: `'(Section 2.2)\n\n3We refer to Appendix A for a detailed discussion.\n\n2\n\n\x0c'`
- vs `expected_outputs/test.md` (5194 chars): **differs by exactly one character** — the
  trailing `\x0c`. Expected file ends `"...2\n\n"` (no form feed). Everything before it is
  byte-identical. The trailing `\x0c` comes from the installed pdfminer.six (20260107)
  appending a page terminator; the checked-in expected file predates/lacks it. Upstream tests
  compare by substring (`assert string in text`), so this never fails upstream.

## 7. Recommended Rust implementation

Simplest byte-parity path for this class of PDF (plain prose, no tables):

1. **Extract text = port `pdfminer.high_level.extract_text` semantics.** The output IS
   pdfminer's output, so parity means reproducing its default pipeline:
   - PDF parser + text-operator interpreter (Tj/TJ/Tf/Tm/Td/TD/T*/BT-ET), matrix tracking,
     glyph → Unicode via font CMap/ToUnicode/Encoding (this is where "framework’s", ligatures,
     hyphens must round-trip identically).
   - Layout analysis with **default LAParams**: `char_margin=2.0, line_margin=0.5,
     word_margin=0.1, boxes_flow=0.5` — LTChar → LTTextLine (x-grouping) → LTTextBox, then
     vertical ordering. `boxes_flow=0.5` controls the reading-order sort of boxes.
   - Rendering rules that must match byte-for-byte: `\n` between lines inside a box; `\n\n`
     between text boxes; space insertion inside a line based on `word_margin * fontsize` gap
     heuristics; empty-box suppression; `\x0c` appended after every page.
2. **Then apply `_merge_partial_numbering_lines`** (~30 lines of Rust, exact port: regex
   `^\.\d+$` on stripped line, merge with next non-empty line).
3. **Form/table heuristic (only needed for parity on form-style PDFs, NOT for test.pdf):**
   port `_extract_form_content_from_words` on top of a word extractor replicating
   pdfplumber's `extract_words(x_tolerance=3, y_tolerance=3)` — that itself is pdfminer's
   LTChar grouping with those tolerances.

Crate choice:
- **`pdf-extract` (0.7/0.8)** — architecturally a partial pdfminer port (has CMap handling +
  LAParams-ish layout). Closest starting point, but its output is NOT byte-identical to
  pdfminer (different spacing/box rules, no per-page `\x0c` semantics). Use only if
  "close-enough" parity is acceptable; expect to fork/patch its line-spacing and
  blank-line logic.
- **`pdfium-render`** — needs the native pdfium binary; pdfium's text segmentation differs
  fundamentally from pdfminer's; not a parity path without heavy post-processing.
- **Recommended: hand-rolled port** of pdfminer.six's `extract_text` chain
  (`PDFPageInterpreter` + `PDFLayoutAnalyzer` + `LAParams` defaults + `LTTextLine.get_text`
  space logic + CMap decoding) in pure Rust, driven by a low-level parser such as
  `lopdf`/`pdf` crate for object/stream access. That is the only realistic route to
  byte-parity; for test.pdf specifically it reduces to: parse 1 page, decode text ops,
  run default-LAParams grouping, emit lines with pdfminer's exact separator rules + trailing
  `\x0c`.

Skip-without-parity-risk (unused by test.pdf, and partially dead code upstream):
`_to_markdown_table`, `_extract_tables_from_words` (dead), the entire
form-heuristic branch, pdfplumber `extract_text` chunking, and the exception fallbacks
(only reachable when pdfplumber itself errors).
