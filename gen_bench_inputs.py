"""Generate deterministic benchmark inputs matching the documented scale.

Creates (repo root):
  big.csv  - 300,000 data rows (~11.8MB), prose-like cells with unicode,
             quoting, pipes and embedded newlines to exercise the escaper.
  big.html - 3,000 sections (~2.8MB) mixing headings, tables, lists, links,
             code and entities to exercise the markdownify pipeline.

Both files are gitignored (see .gitignore). Fixed seed -> byte-identical
inputs on every run, so the benchmark is reproducible:

    python gen_bench_inputs.py
    python plot_bench.py
"""

import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
rng = random.Random(20260907)

FIRST = ["alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi",
         "ivan", "judy", "mallory", "niaj", "olivia", "peggy", "quinn",
         "rupert", "sybil", "trent", "ursula", "victor", "wendy", "xavier",
         "yvonne", "zach", "ālice", "李雷", "café-owner", "o'brien"]
CITY = ["Hyderabad", "Beijing", "Berlin", "Cairo", "Denver", "Oslo",
        "Paris", "Quito", "Rome", "Seoul", "Tokyo", "Lima", "Nairobi"]
NOTE = ["paid in full", "a|b pipe", 'say "hi", twice', "line1\nline2",
        "back\\slash", "50% off, today", "naïve café — 北京", "ok"]


def gen_csv(path, rows=300_000):
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write("id,name,city,amount,note\n")
        for i in range(rows):
            name = rng.choice(FIRST)
            city = rng.choice(CITY)
            amount = rng.randint(1, 99999)
            note = rng.choice(NOTE)
            # quote fields that need it, like a real exporter would
            cells = [str(i), name, city, str(amount), note]
            out = []
            for c in cells:
                if any(ch in c for ch in ',"\n'):
                    c = '"' + c.replace('"', '""') + '"'
                out.append(c)
            f.write(",".join(out) + "\n")
    print(f"wrote {path} ({os.path.getsize(path) / 1e6:.1f}MB, {rows} rows)")


def gen_html(path, sections=3_000):
    with open(path, "w", encoding="utf-8") as f:
        f.write("<html><head><title>Bench</title></head><body>\n")
        for s in range(sections):
            name = rng.choice(FIRST)
            city = rng.choice(CITY)
            f.write(f"<section><h2>Section {s}: {name} in {city}</h2>\n")
            f.write(f"<p>Prose with <b>bold</b>, <em>em</em>, <code>code|pipe</code>, "
                    f"a <a href=\"page {s}.html\">relative link</a>, an autolink "
                    f"<a href=\"https://example.com/{s}\">https://example.com/{s}</a>, "
                    f"entities &amp; &lt;tag&gt; &copy;, and unicode Hyderābād — 北京 — café.</p>\n")
            f.write("<table><thead><tr><th>Item</th><th>Qty</th><th>Price</th></tr></thead><tbody>")
            for r in range(5):
                f.write(f"<tr><td>widget-{s}-{r}</td><td>{r + 1}</td><td>{(s * r) % 997}.00</td></tr>")
            f.write("</tbody></table>\n")
            f.write(f"<ul><li>point one<ul><li>nested {s}</li></ul></li><li>point two</li></ul>\n")
            f.write(f"<blockquote>quoted <b>bold {s}</b></blockquote>\n")
            f.write(f"<p>see <a href=\"javascript:void(0)\">bad</a> vs <img src=\"img{s}.png\" alt=\"PIC{s}\"></p>\n")
            f.write("</section>\n")
        f.write("</body></html>\n")
    print(f"wrote {path} ({os.path.getsize(path) / 1e6:.1f}MB, {sections} sections)")


if __name__ == "__main__":
    gen_csv(os.path.join(HERE, "big.csv"))
    gen_html(os.path.join(HERE, "big.html"))
