"""Differential parity test: Python CsvConverter (from the cloned
microsoft/markitdown repo) vs markitdown-rs, on tricky generated CSVs.

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

from markitdown._stream_info import StreamInfo          # noqa: E402
from markitdown.converters._csv_converter import CsvConverter  # noqa: E402

DUMP_EXE = os.path.join(HERE, "target", "release", "examples", "dump.exe")

SAMPLES = {
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


def py_convert(data: bytes) -> str:
    conv = CsvConverter()
    res = conv.convert(io.BytesIO(data), StreamInfo(extension=".csv", charset="utf-8"))
    return res.markdown


def rs_convert(path: str) -> str:
    out = subprocess.run(
        [DUMP_EXE, path, ".csv", "utf-8"],
        capture_output=True, text=True, encoding="utf-8",
    )
    if out.returncode != 0:
        return f"<RUST ERROR: {out.stderr.strip()}>"
    return out.stdout


def main() -> int:
    if not os.path.exists(DUMP_EXE):
        print("dump.exe missing — run: cargo build --release --example dump")
        return 2

    fails = 0
    tmp = tempfile.mkdtemp(prefix="mdrs-parity-")
    print(f"parity: {len(SAMPLES)} samples, Python CsvConverter vs markitdown-rs")
    print("-" * 64)
    for name, data in SAMPLES.items():
        path = os.path.join(tmp, f"{name}.csv")
        with open(path, "wb") as f:
            f.write(data)
        expected = py_convert(data)
        got = rs_convert(path)
        if expected == got:
            print(f"  OK    {name}")
        else:
            fails += 1
            print(f"  FAIL  {name}")
            print(f"    python: {expected!r}")
            print(f"    rust  : {got!r}")
    print("-" * 64)
    print("PASS: outputs identical" if fails == 0 else f"{fails} FAILURES")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
