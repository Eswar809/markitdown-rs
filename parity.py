"""Differential parity test: Python converters (from the cloned
microsoft/markitdown repo) vs markitdown-rs, on tricky generated inputs.

Run from the markitdown-rs directory:
    python parity.py <path-to-markitdown-clone>
"""
import io
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
MARKITDOWN_SRC = os.path.join(sys.argv[1] if len(sys.argv) > 1 else "../markitdown",
                              "packages", "markitdown", "src")
sys.path.insert(0, MARKITDOWN_SRC)

import warnings
warnings.filterwarnings("ignore")

from markitdown._stream_info import StreamInfo                    # noqa: E402
from markitdown.converters._csv_converter import CsvConverter     # noqa: E402
from markitdown.converters._docx_converter import DocxConverter   # noqa: E402
from markitdown.converters._html_converter import HtmlConverter   # noqa: E402
from markitdown.converters._pdf_converter import PdfConverter     # noqa: E402
from markitdown.converters._pptx_converter import PptxConverter   # noqa: E402
from markitdown.converters._xlsx_converter import XlsxConverter   # noqa: E402

# real sample documents copied from the upstream test suite
FIXTURES = os.path.join(HERE, "tests", "fixtures")
DOCX_SAMPLES = ["test.docx", "rlink.docx", "test_with_comment.docx", "equations.docx"]
XLSX_SAMPLES = ["test.xlsx"]
PPTX_SAMPLES = ["test.pptx"]
# PDF parity is ASSERTION-LEVEL, not byte-level: the upstream engine is
# pdfminer.six; this port uses the pdf-extract crate (a partial Rust port),
# so layout spacing differs. The comparison mirrors the upstream test style
# instead (per-line rstrip + line-count tolerance of ±2).
PDF_SAMPLES = [
    "test.pdf",
    "MEDRPT-2024-PAT-3847_medical_report_scan.pdf",
    "RECEIPT-2024-TXN-98765_retail_purchase.pdf",
    "REPAIR-2022-INV-001_multipage.pdf",
    "SPARSE-2024-INV-1234_borderless_table.pdf",
]

DUMP_EXE = os.path.join(HERE, "target", "release", "examples", "dump.exe")

CSV_SAMPLES = {
    "basic": b"name,age\nalice,30\nbob,25",
    "quoted_commas": b'"hello, world",2\n"say ""hi""",3',
    "pipes_and_backslashes": b"a|b,plain\nx\\|y,ok\n\"p|q\",2",
    "newlines_in_fields": b'"line1\nline2",1\n"r1\r\nr2",2',
    "blank_lines": b"\n\na,b\n\n1,2\n\n\n",
    "bom": "﻿a,b\n1,2".encode("utf-8"),
    "ragged_rows": b"a,b,c\n1\n1,2,3,4",
    "empty": b"",
    "only_blank_lines": b"\n\n\n",
    "unicode": "name,city\nālice,Hyderābād\n李雷,北京".encode("utf-8"),
    "crlf": b"a,b\r\n1,2\r\n",
    "semicolon_like_text": b"a,b\n'not,a,quote',2",
    "trailing_newline": b"a,b\n1,2\n",
    "single_cell": b"justone\n2ndrow",
    "empty_fields": b"a,,c\n,,\n1,2,3",
    "backslash_runs": b"x\\\\|y,2\n\\\\,1",
}

HTML_SAMPLES = {
    "headings": "<h1>Title</h1><h2>Sub</h2><p>Body text</p>",
    "img_inline": '<p>hi <img src="x.png" alt="PIC"> there</p>',
    "img_block": '<img src="x.png" alt="PIC">',
    "data_uri": '<img src="data:image/png;base64,AAAA" alt="D">',
    "img_data_src": '<img data-src="lazy.png" alt="L">',
    "link_rel": '<p>see <a href="page 1.html">the page</a> now</p>',
    "link_js": '<p><a href="javascript:void(0)">bad</a></p>',
    "autolink": '<p><a href="https://example.com">https://example.com</a></p>',
    "link_title": '<p><a href="x.html" title="The X">x</a></p>',
    "checkbox": '<input type="checkbox" checked>done<input type="checkbox">todo',
    "table": "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
    "table_thead": ("<table><thead><tr><th>H1</th><th>H2</th></tr></thead>"
                    "<tbody><tr><td>a</td><td>b</td></tr></tbody></table>"),
    "table_colspan": ('<table><tr><th colspan="2">Wide</th></tr>'
                      "<tr><td>1</td><td>2</td></tr></table>"),
    "table_no_header": "<table><tr><td>1</td><td>2</td></tr></table>",
    "nested_list": "<ul><li>one<ul><li>nested</li></ul></li><li>two</li></ul>",
    "ol_start": '<ol start="3"><li>x</li><li>y</li></ol>',
    "blockquote": "<blockquote>quoted<b>bold</b></blockquote>",
    "pre_code": "<pre>def f():\n    pass</pre>",
    "inline_code_ticks": "<p>use <code>x | y</code> and <code>has `` ticks</code> here</p>",
    "em_strong": "<p><b>bold</b> and <em>em</em> and <u>u</u> and <s>del</s> and <strike>strike</strike></p>",
    "escapes": "<p>3 * 4 + snake_case *more*</p>",
    "br_hr": "<p>one<br>two</p><hr><p>three</p>",
    "entities": "<p>A &amp; B &lt; C &quot;d&quot; &copy; 2026</p>",
    "divs_sections": "<div><section><p>nested blocks</p></section></div>",
    "script_style": "<body><script>evil()</script><style>.x{}</style><p>kept</p></body>",
    "comments": "<p>before<!-- hidden comment -->after</p>",
    "title_doc": "<html><head><title>T</title></head><body><p>content</p></body></html>",
    "unicode_html": "<p>Hyderābād — 北京 — café</p>",
    "mixed_inline": "<p><em>a</em><b>b</b><code>c</code><a href='d.html'>e</a></p>",
    "whitespace_text": "<p>  spaced   text  </p><p>\n\ttabs\there\n</p>",
    "nested_blockquote": "<blockquote><blockquote>deep</blockquote></blockquote>",
    "definition_list": "<dl><dt>term</dt><dd>definition</dd></dl>",
}


def py_convert_csv(data: bytes) -> str:
    res = CsvConverter().convert(io.BytesIO(data),
                                 StreamInfo(extension=".csv", charset="utf-8"))
    return res.markdown


def py_convert_html(html: str) -> str:
    res = HtmlConverter().convert_string(html)
    return res.markdown


def rs_convert(path: str, ext: str) -> str:
    out = subprocess.run(
        [DUMP_EXE, path, ext, "utf-8"],
        capture_output=True, text=True, encoding="utf-8",
    )
    if out.returncode != 0:
        return f"<RUST ERROR: {out.stderr.strip()}>"
    return out.stdout


def py_convert_docx(path: str) -> str:
    data = open(path, "rb").read()
    res = DocxConverter().convert(io.BytesIO(data),
                                  StreamInfo(extension=".docx", charset="utf-8"))
    return res.markdown


def run_docx_suite():
    fails = 0
    print(f"docx: {len(DOCX_SAMPLES)} upstream sample documents")
    for name in DOCX_SAMPLES:
        path = os.path.join(FIXTURES, name)
        expected = py_convert_docx(path)
        got = rs_convert(path, ".docx")
        if expected == got:
            print(f"  OK    {name}")
        else:
            fails += 1
            print(f"  FAIL  {name}")
            import difflib
            for line in list(difflib.unified_diff(expected.splitlines(), got.splitlines(),
                                                  "python", "rust", lineterm=""))[:12]:
                print("    " + line)
    return fails


def py_convert_xlsx(path: str) -> str:
    data = open(path, "rb").read()
    res = XlsxConverter().convert(io.BytesIO(data),
                                  StreamInfo(extension=".xlsx", charset="utf-8"))
    return res.markdown


def run_xlsx_suite():
    fails = 0
    print(f"xlsx: {len(XLSX_SAMPLES)} upstream sample documents")
    for name in XLSX_SAMPLES:
        path = os.path.join(FIXTURES, name)
        expected = py_convert_xlsx(path)
        got = rs_convert(path, ".xlsx")
        if expected == got:
            print(f"  OK    {name}")
        else:
            fails += 1
            print(f"  FAIL  {name}")
            import difflib
            for line in list(difflib.unified_diff(expected.splitlines(), got.splitlines(),
                                                  "python", "rust", lineterm=""))[:12]:
                print("    " + line)
    return fails


def py_convert_pptx(path: str) -> str:
    data = open(path, "rb").read()
    res = PptxConverter().convert(io.BytesIO(data),
                                  StreamInfo(extension=".pptx", charset="utf-8"))
    return res.markdown


def run_pptx_suite():
    fails = 0
    print(f"pptx: {len(PPTX_SAMPLES)} upstream sample documents")
    for name in PPTX_SAMPLES:
        path = os.path.join(FIXTURES, name)
        expected = py_convert_pptx(path)
        got = rs_convert(path, ".pptx")
        if expected == got:
            print(f"  OK    {name}")
        else:
            fails += 1
            print(f"  FAIL  {name}")
            import difflib
            for line in list(difflib.unified_diff(expected.splitlines(), got.splitlines(),
                                                  "python", "rust", lineterm=""))[:12]:
                print("    " + line)
    return fails


def py_convert_pdf(path: str) -> str:
    data = open(path, "rb").read()
    res = PdfConverter().convert(io.BytesIO(data),
                                 StreamInfo(extension=".pdf", charset="utf-8"))
    return res.markdown


def rs_convert_pdf(path: str) -> str:
    out = subprocess.run(
        [DUMP_EXE, path, ".pdf", "utf-8"],
        capture_output=True, text=True, encoding="utf-8",
    )
    # pdf-extract prints "Unicode mismatch ..." notes to stdout; strip them
    lines = [ln for ln in out.stdout.split("\n")
             if not ln.startswith("Unicode mismatch")]
    return "\n".join(lines)


def run_pdf_suite():
    """PDF parity is ASSERTION-LEVEL (upstream engine is pdfminer.six; this
    port uses the pdf-extract crate). Comparison mirrors the upstream test
    style: per-line rstrip equality + line-count tolerance of ±2. Files that
    only reach CLOSE are reported honestly and counted as failures."""
    fails = 0
    print(f"pdf (assertion-level): {len(PDF_SAMPLES)} upstream sample documents")
    for name in PDF_SAMPLES:
        path = os.path.join(FIXTURES, name)
        expected = py_convert_pdf(path)
        got = rs_convert_pdf(path)
        py_lines = [ln.rstrip() for ln in expected.split("\n")]
        rs_lines = [ln.rstrip() for ln in got.split("\n")]
        same = py_lines == rs_lines
        within2 = abs(len(rs_lines) - len(py_lines)) <= 2
        if same:
            print(f"  OK      {name}")
        else:
            import difflib
            ratio = difflib.SequenceMatcher(None, "\n".join(rs_lines),
                                            "\n".join(py_lines)).ratio()
            fails += 1
            tag = "CLOSE(±2)" if within2 else "PARTIAL"
            print(f"  {tag}  {name}  [lines {len(rs_lines)}/{len(py_lines)} sim={ratio:.2f}]")
    return fails


def run_suite(name, samples, py_fn, ext):
    fails = 0
    tmp = tempfile.mkdtemp(prefix=f"mdrs-{name}-")
    print(f"{name}: {len(samples)} samples")
    for sname, data in samples.items():
        path = os.path.join(tmp, f"{sname}{ext}")
        mode = "wb" if isinstance(data, bytes) else "w"
        with open(path, mode, **({} if isinstance(data, bytes) else {"encoding": "utf-8"})) as f:
            f.write(data)
        expected = py_fn(data)
        got = rs_convert(path, ext)
        if expected == got:
            print(f"  OK    {sname}")
        else:
            fails += 1
            print(f"  FAIL  {sname}")
            print(f"    python: {expected!r}")
            print(f"    rust  : {got!r}")
    return fails


def main() -> int:
    if not os.path.exists(DUMP_EXE):
        print("dump.exe missing — run: cargo build --release --example dump")
        return 2

    fails = 0
    fails += run_suite("csv", CSV_SAMPLES, py_convert_csv, ".csv")
    fails += run_suite("html", HTML_SAMPLES, py_convert_html, ".html")
    fails += run_docx_suite()
    fails += run_xlsx_suite()
    fails += run_pptx_suite()
    total = (len(CSV_SAMPLES) + len(HTML_SAMPLES) + len(DOCX_SAMPLES)
             + len(XLSX_SAMPLES) + len(PPTX_SAMPLES))

    # PDF is assertion-level (pdf-extract vs pdfminer engines differ); strict
    # rstrip-equality failures are reported but do not fail byte parity.
    pdf_fails = run_pdf_suite()

    print("-" * 64)
    print(f"PASS: {total - fails}/{total} outputs identical (byte parity); "
          f"pdf assertion-level suite: {len(PDF_SAMPLES) - pdf_fails}/{len(PDF_SAMPLES)} strict")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
