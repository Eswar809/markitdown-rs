"""Generate benchmark.png — Python markitdown vs markitdown-rs across all
converted formats, measured fresh in one session for consistency.

Run from the repo root:  python plot_bench.py
Requires: the upstream markitdown clone at ../markitdown (for the Python
converters) and cargo-built examples (bench.exe).
"""
import io
import json
import os
import subprocess
import sys
import time
import warnings

warnings.filterwarnings("ignore")

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "..", "markitdown", "packages", "markitdown", "src"))

BENCH_EXE = os.path.join(HERE, "target", "release", "examples", "bench.exe")

# (label, file, extension, python converter factory)
def make_cases():
    from markitdown._stream_info import StreamInfo
    from markitdown.converters._csv_converter import CsvConverter
    from markitdown.converters._docx_converter import DocxConverter
    from markitdown.converters._html_converter import HtmlConverter
    from markitdown.converters._pptx_converter import PptxConverter
    from markitdown.converters._xlsx_converter import XlsxConverter

    big_csv = open(os.path.join(HERE, "big.csv"), "rb").read()
    big_html = open(os.path.join(HERE, "big.html"), encoding="utf-8").read()

    return [
        ("CSV 13MB (300k rows)", big_csv, ".csv", CsvConverter()),
        ("HTML 1.7MB (3000 sections)", big_html.encode("utf-8"), ".html", HtmlConverter()),
        ("DOCX (AutoGen paper)", open(os.path.join(HERE, "tests/fixtures/test.docx"), "rb").read(), ".docx", DocxConverter()),
        ("XLSX (2 sheets)", open(os.path.join(HERE, "tests/fixtures/test.xlsx"), "rb").read(), ".xlsx", XlsxConverter()),
        ("PPTX (6 slides, chart+table)", open(os.path.join(HERE, "tests/fixtures/test.pptx"), "rb").read(), ".pptx", PptxConverter()),
    ]


def best_us_py(data, conv, runs):
    best = float("inf")
    for _ in range(runs):
        t0 = time.perf_counter()
        conv.convert(io.BytesIO(data), StreamInfo(extension=EXT, charset="utf-8"))
        best = min(best, time.perf_counter() - t0)
    return best / 1e-3  # ms


import io
import time
from markitdown._stream_info import StreamInfo

def measure():
    rows = []
    for label, data, ext, conv in make_cases():
        # python: in-process best of N
        runs = 3 if len(data) > 500_000 else 5
        best_py = float("inf")
        for _ in range(runs):
            t0 = time.perf_counter()
            conv.convert(io.BytesIO(data), StreamInfo(extension=ext, charset="utf-8"))
            best_py = min(best_py, time.perf_counter() - t0)
        py_ms = best_py * 1000

        # rust: bench.exe loops 10x in-process, prints best ms
        path = os.path.join(HERE, "target", "parity", "bench_input" + ext)
        with open(path, "wb") as f:
            f.write(data)
        out = subprocess.run([BENCH_EXE, path, ext], capture_output=True, text=True)
        rs_ms = float(out.stdout.strip().splitlines()[-1])
        rows.append((label, py_ms, rs_ms))
        print(f"  {label:32s} python={py_ms:8.1f} ms  rust={rs_ms:8.2f} ms  {py_ms/rs_ms:6.1f}x")
    return rows


def plot(rows):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    labels = [r[0] for r in rows]
    py = [r[1] for r in rows]
    rs = [r[2] for r in rows]

    fig, ax = plt.subplots(figsize=(11.5, 6), dpi=150)
    y = np.arange(len(labels))
    h = 0.38

    ax.barh(y - h / 2, py, h, label="markitdown (Python)", color="#3776AB")
    ax.barh(y + h / 2, rs, h, label="markitdown-rs (Rust)", color="#DE6437")

    ax.set_yticks(y)
    ax.set_yticklabels(labels)
    ax.invert_yaxis()
    ax.set_xscale("log")
    ax.set_xlabel("ms per document (log scale) — lower is better")
    ax.set_title("Python markitdown vs markitdown-rs — conversion speed",
                 fontsize=13, fontweight="bold", pad=34)
    ax.legend(loc="lower left", bbox_to_anchor=(0.0, 1.005), ncols=2,
              fontsize=10, frameon=False)

    for bars in ax.containers:
        for rect in bars:
            w = rect.get_width()
            if w > 0:
                ax.annotate(f"{w:.2f}" if w < 10 else f"{w:.0f}",
                            xy=(w, rect.get_y() + rect.get_height() / 2),
                            xytext=(4, 0), textcoords="offset points",
                            va="center", fontsize=8, color="#333333")

    for i, r in enumerate(rows):
        ax.annotate(f"{r[1] / r[2]:.0f}x",
                    xy=(1.015, y[i]), xycoords=("axes fraction", "data"),
                    ha="left", va="center", fontsize=12,
                    fontweight="bold", color="#B22222",
                    annotation_clip=False)

    ax.grid(axis="x", which="both", alpha=0.25)
    fig.text(0.99, 0.01,
             "i5-12500H · CPython 3.12 · markitdown 0.1.8b1 vs markitdown-rs · "
             "best of N in-process · measured in a single session",
             ha="right", fontsize=7, color="#777777")
    fig.subplots_adjust(left=0.26, right=0.90, top=0.86, bottom=0.12)
    out = os.path.join(HERE, "benchmark.png")
    fig.savefig(out)
    print("saved", out)


if __name__ == "__main__":
    from markitdown._stream_info import StreamInfo as _SI  # noqa: F401
    globals()["StreamInfo"] = _SI
    rows = measure()
    plot(rows)
