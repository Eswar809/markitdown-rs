# PPTX converter — behavioral spec for the Rust port

Source of truth: `markitdown/packages/markitdown/src/markitdown/converters/_pptx_converter.py`
(upstream microsoft/markitdown, current HEAD as of 2026-09-06). All examples below were
produced by actually running `PptxConverter` on
`packages/markitdown/tests/test_files/test.pptx` and verified byte-for-byte against
`markitdown-rs/target/parity/py_test_pptx.md`.

Python reference implementation uses **python-pptx 1.0.2** (the only converter-specific
dependency; `llm_caption` is inert unless an LLM client is passed). No whole-document
intermediate HTML is generated — only table shapes go through a small HTML round-trip.

---

## 1. Pipeline overview

```
accepts():  extension == ".pptx"  OR  mimetype starts with
            "application/vnd.openxmlformats-officedocument.presentationml"

convert():
    presentation = pptx.Presentation(stream)          # parse the OPC zip
    md = ""
    for slide_num, slide in enumerate(presentation.slides, 1):   # sldIdLst order
        md += f"\n\n<!-- Slide number: {slide_num} -->\n"
        title = slide.shapes.title                    # may be None
        shapes = sorted(slide.shapes, key=(top_quirk, left_quirk))   # see 1.1
        for shape in shapes:
            emit_shape(shape, is_title = shape element is title element)
        md = md.strip()                               # strips WHOLE buffer
        if slide.has_notes_slide:
            md += "\n\n### Notes:\n"
            if slide.notes_slide.notes_text_frame is not None:
                md += slide.notes_slide.notes_text_frame.text or ""
            md = md.strip()
    return DocumentConverterResult(markdown=md.strip())   # .title is never set (None)
```

`emit_shape(shape)` (recursive for groups), in this exact order of checks:

```
if is_picture(shape):        # PICTURE type, or PLACEHOLDER whose .image resolves
                                # (or PLACEHOLDER carrying an <asvg:svgBlip>)
    md += picture_markdown(shape)                # see 2.3
if is_table(shape):          # shape_type == MSO_SHAPE_TYPE.TABLE
    md += table_html_roundtrip(shape.table)      # see 2.4
if shape.has_chart:          # graphicFrame with chart part
    md += chart_markdown(shape.chart)            # see 2.5
elif shape.has_text_frame:   # p:sp with txBody (incl. title, autoshapes, textboxes)
    text = shape.text or ""                      # None-tolerant
    if shape element is the title element (identity/equality):
        md += "# " + text.lstrip() + "\n"
    else:
        md += text + "\n"
if shape.shape_type == GROUP:                    # separate if, not elif
    for subshape in sorted(shape.shapes, key=(top_quirk, left_quirk)):
        emit_shape(subshape)                     # recursion; group members inherit nothing
```

Key structural points a port must replicate:

- The three feature checks are **not** mutually exclusive: picture/table are independent
  `if`s; chart/text are `if/elif` (a chart shape has no text frame in practice, so the
  `elif` just prevents double emission).
- **Groups recurse**; a group's subshapes are re-sorted with the same key. Group members
  keep their group-local `a:off` coordinates (python-pptx does not transform them), so
  the sort inside a group uses those raw child offsets.
- Title detection is by **element identity**, not shape name: python-pptx
  `BaseShape.__eq__` compares the underlying XML element. In Rust, mark the title
  element (first placeholder with `p:ph` idx attr absent or `idx="0"` in `p:spTree`
  document order — `CT_Placeholder.idx` defaults to 0) and compare by that marker.
- An empty/None text is tolerated: `shape.text or ""` still emits `"\n"` (stripped later).

### 1.1 Shape ordering (the `top/left` quirk)

Sort key: `(top, left)` where each coordinate becomes **`-inf` when falsy**
(`None`, `0`, or missing):

```python
key = lambda x: (float("-inf") if not x.top else x.top,
                 float("-inf") if not x.left else x.left)
```

- `-inf` sorts BEFORE every finite value, including negative coordinates. So shapes with
  missing/zero offsets always come first (among themselves, ordered by the other key).
- `top`/`left` are EMU ints. For placeholders without an explicit `a:xfrm`
  (very common for title placeholders — true for **all six title shapes in test.pptx**),
  python-pptx **inherits geometry from the matching layout placeholder (by idx), then
  the master**. Port options:
  1. Faithful: implement the inheritance lookup (layout `p:sp` with same `p:ph` idx,
     fall back to master).
  2. Pragmatic: missing xfrm → treat as 0 → `-inf` → shape sorts first. This reproduces
     markitdown's output on test.pptx and on most decks (titles have the smallest
     inherited top), but can mis-order when another shape sits above an inherited
     placeholder position.
  Recommendation: option 1 if cheap, option 2 as fallback; document the divergence.

### 1.2 Slide join semantics (exact)

- Each slide emits `\n\n<!-- Slide number: N -->\n` first (comment on its own line,
  content starts on the next line).
- After all shapes of a slide, `md = md.strip()` — trailing whitespace (including the
  `\n` after the last shape and any empty trailing shape texts) is removed.
- Notes (if any) are then appended as `\n\n### Notes:\n` + raw notes text and stripped
  again.
- Because of the per-slide strip, consecutive slides are always separated by exactly
  one blank line + the comment: `"...slide N content\n\n<!-- Slide number: N+1 -->\n..."`.
- **Empty slide** (no shapes, no notes): leaves a lone comment line; two consecutive
  empty slides yield `<!-- Slide number: 2 -->\n\n<!-- Slide number: 3 -->` (verified on
  a synthetic deck).
- The final `DocumentConverterResult.markdown = md.strip()` removes the leading `\n\n`
  of slide 1, so output **starts with `<!-- Slide number: 1 -->`** and has **no trailing
  newline**. `.title` is `None` for pptx.

---

## 2. Exact markdown per shape type

### 2.1 Titles

`"# " + text.lstrip() + "\n"` — only leading whitespace is stripped; internal whitespace
(including non-breaking spaces) is preserved verbatim, and no markdown escaping happens.

test.pptx slide 4 title is `"A chart\xa0to test parsing:"` (NBSP between "chart" and "to"):

```python
'# A chart\xa0to test parsing:\n'
```

Title placeholder type can be `title` or `ctrTitle`; lookup is "first `p:ph` with idx
absent/0 in spTree order", regardless of the shape's name (slide 4's title is named
`'タイトル 1'`, a Japanese-named shape, and is still treated as title).

### 2.2 Text frames (body, textboxes, autoshapes)

`text + "\n"` — raw, no escaping, no bullet rendering:

- `text` = python-pptx `TextFrame.text` = paragraphs joined by `"\n"`
  (one entry per `a:p`, **including empty paragraphs**, so a 1-para + 1-empty-para frame
  yields `"para0\n"`), each paragraph = concat of run texts (`a:r/a:t`), field texts
  (`a:fld`), and `"\v"` (vertical tab, U+000B) for each `a:br` soft line break.
- **Bullet levels are NOT represented.** A level-2 paragraph comes out as a plain line.
  Verified on a synthetic deck — a 4-paragraph body with levels 0,1,2,0 produces:

```python
'# Bullet Slide\nLevel zero item\nLevel one item\nLevel two item\nBack to zero\n'
```

- test.pptx slide 5 group member "Rectangle 83" has paragraphs
  `['This is a nested shape with content in 2 shapes', 'Comment 1', 'Comment 2: ', 'Sub comment 2']`
  (last one `level=1`); output is:

```python
'This is a nested shape with content in 2 shapes\nComment 1\nComment 2: \nSub comment 2\n'
```

  (note the trailing space in `'Comment 2: '` survives; the separate sibling shape whose
  text is `' '` contributes `" \n"` which is removed by the slide-level strip).
- Empty paragraphs in a frame produce blank lines *inside* the slide content that only
  disappear if they are at the very end of the slide (strip). Slide 1 subtitle's empty
  second paragraph is at slide end → invisible. Markdown-special characters are NOT
  escaped: a textbox with `"brackets [x] and\nnewline"` emits exactly
  `'brackets [x] and\nnewline\n'`.

### 2.3 Pictures

Output: `"\n![{alt}]({filename})\n"` (default) — note the blank-line-producing leading
`\n` and trailing `\n`.

- `alt` construction:
  1. `llm_description` (only when an LLM client+model are passed; empty otherwise).
  2. `alt_text` = `descr` attribute of the shape's `p:nvPicPr/p:nvPr.../p:cNvPr`
     element (works for pictures and placeholders via `shape._element._nvXxPr.cNvPr`).
  3. `alt = "\n".join(t for t in [llm_description, alt_text] if t and t.strip()) or shape.name`
  4. Sanitize: `alt = re.sub(r"[\r\n\[\]]", " ", alt)` then
     `re.sub(r"\s+", " ", alt).strip()` — newlines, CR and square brackets become
     spaces; runs of whitespace collapse to one space.
- `filename` = `re.sub(r"\W", "", shape.name) + ".jpg"` — **every** non-word char
  removed, always `.jpg` extension regardless of the real image type (Unicode `\W`:
  word chars are `[a-zA-Z0-9_]` + Unicode letters/digits). `"Picture 4"` →
  `"Picture4.jpg"`. Distinct pictures on different slides with the same name produce the
  same placeholder filename (test.pptx has `Picture4.jpg` on slides 2 and 6).
- `keep_data_uris=True` option: `"\n![{alt}](data:{content_type};base64,{b64})\n"` with
  `content_type = image.content_type or "image/png"`.
- Image resolution: `shape.image.blob/.content_type/.filename`; if that raises (SVG
  pictures without rasterized fallback — `a:blip` has no `r:embed`, only an
  `<asvg:svgBlip>` in namespace
  `http://schemas.microsoft.com/office/drawing/2016/SVG/main`), fall back to the part
  referenced by `svgBlip`'s r:embed with content type `"image/svg+xml"`. If both fail,
  blob is None → alt falls back to `shape.name`; default path still emits the link.
- Real examples from test.pptx (no LLM configured):
  - Slide 2: `descr='The first page of the AutoGen ArXiv paper.  44bf7d06-5e7a-4a40-a2e1-a2e42ef28c8a'`
    (double space in source) →

```python
'\n![The first page of the AutoGen ArXiv paper. 44bf7d06-5e7a-4a40-a2e1-a2e42ef28c8a](Picture4.jpg)\n'
```

  - Slide 6: `descr='This phrase of the caption is Human-written.'` →

```python
'\n![This phrase of the caption is Human-written.](Picture4.jpg)\n'
```

  Upstream test asserts the slide-6 string verbatim
  (`test_module_misc.py::test_pptx_converter_treats_none_llm_caption_as_empty`).

### 2.4 Tables (the only intermediate HTML)

Build exactly this string, run it through the HTML converter, append
`result.markdown.strip() + "\n"`:

```python
html_table = "<html><body><table>"
first_row = True
for row in table.rows:
    html_table += "<tr>"
    for cell in row.cells:
        html_table += ("<th>" if first_row else "<td>") + html.escape(cell.text) + ("</th>" if first_row else "</td>")
    html_table += "</tr>"
    first_row = False
html_table += "</table></body></html>"
```

- First row → `<th>`, all others `<td>`; cell text = `a:txBody` text joined like a text
  frame (same `"\n"`/`"\v"` rules), then `html.escape` (`& < >` and quotes become
  `&amp; &lt; &gt; &#x27; &#x27;`... precisely `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#x27;`).
- The HTML converter (markdownify) turns that into a pipe table with `"| --- |"`-style
  separators (padded). Exact round-trip for test.pptx slide 3 (4x6 table):

```python
'<html><body><table><tr><th>ColA</th><th>ColB</th><th>ColC</th><th>ColD</th><th>ColE</th><th>ColF</th></tr><tr><td>1</td><td>2</td><td>3</td><td>4</td><td>5</td><td>6</td></tr><tr><td>7</td><td>8</td><td>9</td><td>1b92870d-e3b5-4e65-8153-919f4ff45592</td><td>11</td><td>12</td></tr><tr><td>13</td><td>14</td><td>15</td><td>16</td><td>17</td><td>18</td></tr></table></body></html>'
```

  →

```python
'| ColA | ColB | ColC | ColD | ColE | ColF |\n| --- | --- | --- | --- | --- | --- |\n| 1 | 2 | 3 | 4 | 5 | 6 |\n| 7 | 8 | 9 | 1b92870d-e3b5-4e65-8153-919f4ff45592 | 11 | 12 |\n| 13 | 14 | 15 | 16 | 17 | 18 |\n'
```

- **Verified byte-identical** through markitdown-rs:
  `target/release/examples/dump.exe <file.html> .html utf-8` on the exact table HTML
  above produced the same markdown, so generating this HTML and calling
  `crate::converters::html::convert_html_string` is the correct port strategy.
- Merged cells: python-pptx `row.cells` repeats the spanned cell text per column
  (no colspan handling) — the HTML converter's colspan logic is never triggered here.

### 2.5 Charts

```python
md = "\n\n### Chart"
if chart.has_title and chart.chart_title.has_text_frame:
    md += f": {chart.chart_title.text_frame.text}"
md += "\n\n"
# markdown table: header ["Category", *series names], separator "|---|...|",
# then one row per category: [category_label, *series values (None if missing)]
"\n".join(...)   # no trailing newline
```

- Values go through Python `str()`; numCache numbers are floats, so `2000.0` renders as
  `"2000.0"`. **Rust warning:** `format!("{}", 2000.0_f64)` gives `"2000"`; the port must
  emulate Python float str (shortest repr, but append `.0` for integral finite values,
  and `inf`/`nan`/`1e+21` styles). Missing points become `str(None)` = `"None"`.
- Separator has NO padding: `"|" + "---|"*n + "|"` → `|---|---|` (unlike the HTML-table
  path which produces `| --- | --- |`).
- Header line and rows are padded pipes: `"| " + " | ".join(cells) + " |"`.
- Any exception (incl. unsupported plot types → `ValueError("unsupported plot type")`)
  yields `"\n\n[unsupported chart]\n\n"`.
- Exact test.pptx output (slide 4, COLUMN_CLUSTERED, title GUID, 1 series, 4 categories):

```python
'\n\n### Chart: a3f6004b-6f4f-4ea8-bee3-3741f4dc385f\n\n| Category | Series 1 |\n|---|---|\n| 2000 | 2000.0 |\n| 2001 | 2001.0 |\n| 2002 | 2002.0 |\n| 2003 | 2003.0 |'
```

  (the leading `\n\n` + the title line's trailing `\n` produce the
  `...parsing:\n\n\n### Chart:` triple newline seen in the final markdown; the chart
  block's lack of trailing `\n` is irrelevant because the slide-level strip follows).

### 2.6 Speaker notes

Only if `slide.has_notes_slide` (guard — never touch `slide.notes_slide` otherwise;
python-pptx would *create* a notes slide on access). Append
`"\n\n### Notes:\n"` + `notes_text_frame.text` (may be None if the notes slide has no
body placeholder → append nothing). Then strip. Notes text keeps its internal `"\n"`
paragraph separators verbatim:

```python
'...Back to zero\n\n### Notes:\nSpeaker notes line 1\nSpeaker notes line 2'
```

test.pptx has **no notes slides**, so the port's ground truth doesn't exercise this —
the synthetic-deck example above is the reference.

---

## 3. test.pptx inventory (ground truth context)

Zip parts that matter: `ppt/presentation.xml` (+`_rels` → slide order), `ppt/slides/slide1..6.xml`
(+ per-slide `_rels` for image/chart relationships), `ppt/slideLayouts/` +
`ppt/slideMasters/` (placeholder geometry/text inheritance), `ppt/charts/chart1.xml`
(+`_rels`), `ppt/media/image1.jpeg`, `ppt/media/image2.jpg`. Ignored by the converter:
`docProps/*` (incl. `thumbnail.jpeg`), `ppt/theme/`, `ppt/tableStyles.xml`,
`ppt/viewProps.xml`, `ppt/presProps.xml`, `docMetadata/`.

| Slide | Layout | Shapes (doc order) | Output features |
|---|---|---|---|
| 1 | Title Slide | Title (inherited geom), Subtitle (2 paras, 2nd empty) | `# title` + plain body |
| 2 | Title and Content | Title, Picture `Picture 4` (top=1486948) , Content Placeholder (top=1825625) | picture **sorts before** body text (smaller top); alt from descr; filename `Picture4.jpg` |
| 3 | Title and Content | Title, `Table 6` (4x6) | table → HTML round-trip |
| 4 | Title and Content | Title `'タイトル 1'` (NBSP in text), `グラフ 3` chart | NBSP preserved; chart table; non-ASCII shape names irrelevant |
| 5 | Title and Content | Title, `Group 5` (3 autoshapes; one with level-1 para; one with text `' '`) | group recursion + re-sort; level flattened; whitespace-only shape stripped away |
| 6 | Title and Content | Title, `Picture 4` (descr = human caption) | alt sanitize; same placeholder filename as slide 2 |

No notes slides, no SVG images, no grouped pictures/charts, no `<a:br>` soft breaks.

---

## 4. Recommended Rust implementation sketch

Add `src/converters/pptx.rs`; register in `DocumentConverter` chain. No new deps for the
default path (`zip` + `roxmltree` suffice). Optional `keep_data_uris` needs a base64
encoder (crate has none today — add `base64` or skip the option initially).

```
accepts: extension == ".pptx" (case-insensitive) || mimetype.starts_with(
    "application/vnd.openxmlformats-officedocument.presentationml")

convert(stream):
 1. zip::read → load parts into a map (resolve OPC rels lazily).
 2. Parse ppt/presentation.xml + ppt/_rels/presentation.xml.rels → ordered slide part
    paths from p:sldIdLst (authoritative order; do NOT sort by filename).
 3. For each slide part:
    a. Parse slideN.xml with roxmltree.
    b. Determine title element: first p:sp in document order whose p:nvSpPr/p:nvPr/p:ph
       has no idx attr or idx="0".
    c. Collect top-level children of p:spTree (p:sp, p:pic, p:graphicFrame, p:grpSp,
       p:cxnSp) in document order; sort stably by (top_key, left_key) using:
          - explicit a:xfrm/a:off if present (group children use their own raw off);
          - else inherited from layout/master placeholder with same ph idx;
          - else (or if 0) -inf.
       (Stable sort: python's sorted() is stable, ties keep document order.)
    d. emit_shape(shape, title_elem):
       - is_picture: node is p:pic (MSO PICTURE) OR p:sp placeholder whose blipFill
         r:embed resolves to an image part (or has asvg:svgBlip). Build alt per 2.3
         ( descr attr: p:nvPicPr/p:cNvPr@descr — for placeholder pics
           p:nvSpPr/p:cNvPr@descr ), sanitize with the two regexes
         (in Rust: replace [\r\n\[\]]→" ", then collapse \s+→" ", trim).
         filename: shape name chars, keep only word chars (alphanumeric + '_' +
         Unicode alphabetic — use char::is_alphanumeric() || '_' ) + ".jpg".
         Emit "\n![{alt}]({filename})\n".
       - is_table: p:graphicFrame with a:tbl child. Build the exact
         "<html><body><table>..." string (first row <th>, html-escape cell text:
         & → &amp;, < → &lt;, > → &gt;, " → &quot;, ' → &#x27;), call
         crate::converters::html::convert_html_string(&html), append
         markdown.trim() + "\n" (NOTE: trim both ends like Python .strip()).
       - has_chart: p:graphicFrame with chart rel (r:id → ppt/charts/chartN.xml).
         Parse c:title (has_title: c:title element present && not autoTitleDeleted),
         c:ser list: name from c:tx/c:strRef/c:strCache/c:pt/c:v (else "" or first pt),
         categories from first plot's c:cat (strCache labels; numeric cats via
         numCache → str), values from c:val/c:numRef/c:numCache pts (missing → "None").
         Emit per 2.5 with Python-float-str emulation. On any parse failure emit
         "\n\n[unsupported chart]\n\n".
       - else if p:txBody present (p:sp): text = paragraphs (a:p) joined "\n";
         paragraph text = concat over children: a:r → its a:t texts concatenated,
         a:br → "\v", a:fld → its a:t text. Title element → "# " + trim_start + "\n",
         else text + "\n". (Handle None-text quirk: empty → just "\n".)
       - if p:grpSp: recurse over its p:sp/p:pic/p:graphicFrame/p:grpSp children
         (sorted by the same key, using their raw child a:off values).
    e. trim_end the whole buffer; if slide has notesSlide rel (resolve
       ppt/notesSlides/notesSlideN.xml; do NOT create), append "\n\n### Notes:\n" +
       notes body placeholder (ph type="body") text or "", then trim.
 5. Return markdown = whole buffer trim; title = None.
```

Reuse notes: the table path is the only place `convert_html_string` is used; everything
else is direct string building. This mirrors the docx flow's philosophy but is much
thinner.

Python `str.strip()` equivalence: Python strips a fixed set of whitespace chars
(space \t \n \r \v \x0c and some Unicode spaces) — Rust `trim()` strips Unicode
whitespace incl. `\v`? (Rust `char::is_whitespace` includes U+000B.) Close enough; test
with the sample deck.

---

## 5. Edge cases the port must handle (from test.pptx + probes)

1. **NBSP inside title text** (`\xa0`) must survive untouched (byte-exact output).
2. **Alt-text whitespace collapse**: source descr had two spaces; output has one.
   Newlines/brackets in descr → spaces.
3. **Placeholder filenames**: `re.sub(r"\W","",name)+".jpg"` — punctuation removed
   (`"Picture 4"` → `Picture4.jpg`), Unicode word chars kept; `.jpg` even for jpeg/svg.
4. **Ordering by (top,left)** with `-inf` for falsy/missing values; pictures can precede
   body text; group children re-sorted with group-local coordinates.
5. **Title placeholder geometry inherited from layout** (no `a:xfrm` on slide) — needed
   for the sort; see 1.1 options.
6. **In-shape order**: picture before table before chart/text for the same shape; group
   check last and recursive.
7. **Trailing whitespace-only shapes** (text `" "`) produce `" \n"` that must be removed
   by the slide-end trim, but a `"Comment 2: "` trailing space mid-slide must survive.
8. **Empty text frames / None text** emit `"\n"` (upstream regression #1808) — never
   panic on missing `a:t`, missing txBody, missing cNvPr descr.
9. **Empty slide** → lone `<!-- Slide number: N -->` comment; final output still starts
   with slide 1's comment and has no trailing newline.
10. **Chart float formatting**: `2000.0` not `2000`; missing points print `None`;
    separator `|---|` unpadded vs HTML-table `| --- |` padded — both formats coexist in
    one output.
11. **Multi-paragraph frames**: every `a:p` contributes a `"\n"` join entry, so an empty
    last paragraph adds a trailing newline (later stripped only if at slide end).
12. **Non-ASCII shape names** (`タイトル 1`, `グラフ 3`) must not affect behavior.
13. **Same-named picture shapes on different slides** → identical placeholder filenames
    (do not deduplicate).
14. **keep_data_uris**: base64 of raw blob, content type from the image part (or
    `image/png` fallback); SVG-only pictures → `image/svg+xml`.

## 6. Ground truth artifacts

- `target/parity/py_test_pptx.md` — exact Python PptxConverter output for test.pptx
  (2049 chars / 2050 bytes UTF-8, no BOM, LF, no trailing newline).
- `target/parity/py_test_pptx.html` — annotated sample of the captured intermediate
  table HTML (the exact string built for slide 3's table).
- Probe scripts kept alongside (not part of the crate): `target/parity/_pptx_probe.py`,
  `target/parity/_pptx_inventory.py`, `target/parity/_pptx_edge_probe.py`.
- Upstream `expected_outputs/test.md` is the **PDF** ground truth, unrelated to pptx;
  vector tests only assert `must_include` GUIDs for test.pptx.
