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
from markitdown.converters._html_converter import HtmlConverter   # noqa: E402

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
    total = len(CSV_SAMPLES) + len(HTML_SAMPLES)
    print("-" * 64)
    print(f"PASS: {total - fails}/{total} outputs identical" if fails == 0
          else f"{fails} FAILURES out of {total}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
