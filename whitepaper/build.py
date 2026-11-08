# -*- coding: utf-8 -*-
"""
File Path: whitepaper/build.py
Purpose: Build the Oasis white paper (working paper v1): figures from the raw bench files, the HTML document, the PDF.
  Same pipeline as the LUMA papers (cdxtest/conversion-paper): Python figures -> HTML + paper.css -> headless Edge PDF.
Inputs (data/, copied from hacktud_team_win origin/main d6bc20d and cdxtest/compute-index):
  B3_26b_pool.csv   test-runs/bench/p4-26b-final/B3/bench-2026-11-08T13-13-13Z.csv
  B2_12b_pool.csv   test-runs/bench/p4-20261108-0150-existing-fa-q8/bench-2026-11-08T07-50-33Z.csv
  B1-0408.log, B1s-0408.log, B1-0329.log, B1s-0329.log   test-runs/bench/night-*/B1*/bench.log
Outputs: fig/*.png, oasis_whitepaper.html, oasis_whitepaper.pdf, check.md (plain text for check_draft.py)
Usage: python build.py            (figures + html + pdf)
       python build.py --no-pdf
Every number the text quotes from a run is recomputed here and asserted (see check_numbers()).
"""
import csv, json, math, os, re, statistics as S, subprocess, sys
from datetime import datetime

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

HERE = os.path.dirname(os.path.abspath(__file__))
D = lambda f: os.path.join(HERE, "data", f)
FIG = os.path.join(HERE, "fig")
os.makedirs(FIG, exist_ok=True)

BLUE, GRAY, DARK, LIGHT, RED = "#1f4e9a", "#8a8a8a", "#222222", "#c9d3e6", "#a33b2b"
plt.rcParams.update({"font.family": "serif", "font.serif": ["Cambria", "Georgia", "DejaVu Serif"], "font.size": 9,
                     "axes.spines.top": False, "axes.spines.right": False, "axes.linewidth": 0.7,
                     "xtick.major.width": 0.6, "ytick.major.width": 0.6, "savefig.dpi": 220,
                     "legend.frameon": False})
PROMPTS = ["short", "long", "edit"]


# ---------------------------------------------------------------------------------------------------------- data
def load_csv(name):
    rows = list(csv.DictReader(open(D(name), encoding="utf-8")))
    for r in rows:
        r["t0"] = datetime.strptime(r["started_at"], "%Y-%m-%dT%H:%M:%SZ")
        r["tps"], r["ttft"], r["ms"] = float(r["gen_tokens_per_s"]), float(r["ttft_ms"]), float(r["total_ms"])
        r["prompt"] = float(r["prompt_tokens_per_s"])
        r["measured"] = r["warmup"] == "false"
    t0 = rows[0]["t0"]
    for r in rows:
        r["start_min"] = (r["t0"] - t0).total_seconds() / 60
        r["end_min"] = r["start_min"] + r["ms"] / 60000
    return rows


LOG_RE = re.compile(r"(\w+) (warmup|run) #\d+: TTFT (\d+) ms, ([\d.]+) tok/s, (\d+) tokens, (\d+) ms")


def load_log(name):
    """bench.log has no timestamps: requests ran back to back, so elapsed time is the running sum of durations."""
    rows, t = [], 0.0
    for m in LOG_RE.finditer(open(D(name), encoding="utf-8").read()):
        p, kind, ttft, tps, _, ms = m.groups()
        r = dict(prompt_id=p, measured=kind == "run", ttft=float(ttft), tps=float(tps), ms=float(ms),
                 start_min=t / 60000)
        t += float(ms)
        r["end_min"] = t / 60000
        rows.append(r)
    return rows


def med(rows, prompt, key):
    return S.median(r[key] for r in rows if r["measured"] and r["prompt_id"] == prompt)


B3, B2 = load_csv("B3_26b_pool.csv"), load_csv("B2_12b_pool.csv")
B1, B1s = load_log("B1-0408.log"), load_log("B1s-0408.log")
B1a, B1sa = load_log("B1-0329.log"), load_log("B1s-0329.log")

GIB = 1024 ** 3
MODEL_26B_BYTES = 26859861728              # diagnosis.md, verified file size
MODEL_26B_GIB = MODEL_26B_BYTES / GIB      # 25.0
BUDGET = {"M3 Air 16 GB (main)": 11.8, "M5 Air 24 GB (helper)": 17.8, "M5 Air 24 GB (helper) ": 17.8}
PLACEMENT_MIB = [("Main CPU", 748), ("Main M3 GPU", 5718.50), ("Helper M5 .174", 9940.98), ("Helper M5 .204", 9940.98)]


# ---------------------------------------------------------------------------------------------------------- figures
def save(fig, name):
    fig.tight_layout()
    fig.savefig(os.path.join(FIG, name), bbox_inches="tight")
    plt.close(fig)


def fig_memory():
    fig, ax = plt.subplots(figsize=(6.4, 2.7))
    labels = ["M3 Air 16 GB\n(main .113)", "M5 Air 24 GB\n(helper .174)", "M5 Air 24 GB\n(helper .204)", "Pool\n(sum)"]
    vals = [11.8, 17.8, 17.8, 11.8 + 17.8 + 17.8]
    cols = [GRAY, GRAY, GRAY, BLUE]
    b = ax.bar(range(4), vals, color=cols, width=0.62)
    ax.set_xticks(range(4))
    ax.set_xticklabels(labels)
    for rect, v in zip(b, vals):
        ax.text(rect.get_x() + rect.get_width() / 2, v + 0.8, f"{v:.1f}", ha="center", fontsize=8.5)
    ax.axhline(MODEL_26B_GIB, color=RED, lw=1.2, ls="--", label=f"Gemma 4 26B Q8_0 = {MODEL_26B_GIB:.1f} GiB")
    ax.axhline(7.1 * 1e9 / GIB, color=DARK, lw=0.9, ls=":", label="Gemma 4 12B Q4_K_M = 6.6 GiB")
    ax.legend(loc="upper left", fontsize=8)
    ax.set_ylabel("GPU memory budget (GiB)")
    ax.set_ylim(0, 52)
    save(fig, "fig1_memory.png")


def fig_sustained():
    fig, ax = plt.subplots(figsize=(6.4, 2.9))
    series = [(B3, BLUE, "-", "3 Airs, 26B Q8 (B3)"), (B2, "#4f78b8", "--", "3 Airs, 12B Q4 (B2)"),
              (B1, RED, "-", "1 M5 Air, 12B Q4 (B1)")]
    for rows, c, ls, lab in series:
        xs, ys = [], []
        for r in rows:
            xs += [r["start_min"], r["end_min"]]
            ys += [r["tps"], r["tps"]]
        ax.plot(xs, ys, color=c, ls=ls, lw=1.6, label=lab)
    ax.set_xlabel("Minutes of back-to-back generation (each step is one request, warm-ups included)")
    ax.set_ylabel("Generation (tokens/s)")
    ax.set_ylim(0, 18)
    ax.legend(loc="lower left", fontsize=8)
    ax.annotate("one Air throttles\nafter about 1.5 min", xy=(2.0, 10.3), xytext=(3.2, 15.2), fontsize=8,
                arrowprops=dict(arrowstyle="->", lw=0.7))
    save(fig, "fig2_sustained.png")


LAPTOP_GIB, LAPTOP_HEADROOM = 14.0, 0.87     # 14 GiB per laptop (our measured mean is 15.8); keep 13% for KV cache
SCALE_MODELS = [  # GGUF sizes in GiB, summed shards, unsloth repos via the Hugging Face API (2026-11-08)
    ("Gemma 4 26B-A4B", "Q8_0", 25.0), ("Gemma 4 31B", "Q8_0", 30.4), ("Llama 3.3 70B", "Q4_K_M", 39.6),
    ("gpt-oss-120b", "MXFP4", 59.0), ("GLM-4.5-Air", "Q8_0", 109.4), ("Qwen3-235B-A22B", "Q4_K_M", 132.4),
    ("Qwen3-235B-A22B", "Q8_0", 232.8), ("DeepSeek V3.1 671B", "Q4_K_M", 377.6), ("DeepSeek V3.1 671B", "Q8_0", 664.3),
    ("Kimi K2 1T", "Q8_0", 1016.1)]


def laptops_needed(gib):
    return math.ceil(gib / (LAPTOP_GIB * LAPTOP_HEADROOM))


def fig_scale():
    fig, ax = plt.subplots(figsize=(6.4, 2.9))
    names = [f"{m} {q}" for m, q, _ in SCALE_MODELS]
    need = [laptops_needed(g) for _, _, g in SCALE_MODELS]
    cols = [RED if n <= 3 else BLUE for n in need]
    ax.barh(range(len(names)), need, color=cols, height=0.62)
    for i, (n, (_, _, g)) in enumerate(zip(need, SCALE_MODELS)):
        ax.text(n * 1.06, i, f"{n}  ({g:g} GiB)", va="center", fontsize=7.4)
    ax.set_yticks(range(len(names)))
    ax.set_yticklabels(names, fontsize=7.6)
    ax.invert_yaxis()
    ax.set_xscale("log")
    ax.set_xlim(1, 260)
    ax.set_xticks([1, 3, 10, 30, 100])
    ax.set_xticklabels(["1", "3", "10", "30", "100"])
    ax.axvline(3, color=RED, lw=0.7, ls=":")
    ax.set_xlabel("MacBook Airs needed at 14 GiB each, 13% kept free (calculated, not run); red = fits our 3")
    save(fig, "fig3_scale.png")


DS_QUANTS = [("IQ1_S", 76.9), ("Q2_K_XL", 90.2), ("IQ3_XXS", 95.9), ("Q3_K_XL", 120.6), ("Q4_K_XL", 144.4),
             ("Q8_K_XL", 150.8)]
CARD_GIB, HEADROOM = 22.5, 0.90


def cards_needed(gib):
    return math.ceil(gib / (CARD_GIB * HEADROOM))


def fig_deepseek():
    fig, ax = plt.subplots(figsize=(6.4, 2.6))
    names = [q for q, _ in DS_QUANTS]
    sizes = [s for _, s in DS_QUANTS]
    ax.bar(names, sizes, color=[GRAY] * 4 + [BLUE, BLUE], width=0.6)
    for i, s in enumerate(sizes):
        ax.text(i, s + 3, f"{s:.0f} GiB\n{cards_needed(s)} cards", ha="center", fontsize=7.3)
    for k in (3, 5, 8):
        y = k * CARD_GIB * HEADROOM
        ax.axhline(y, color=RED, lw=0.7, ls="--")
        ax.text(5.42, y + 3.5, f"{k} x RTX 3090 = {y:.0f} GiB", fontsize=7, color=RED, va="center")
    ax.set_ylabel("GGUF size (GiB)")
    ax.set_ylim(0, 190)
    ax.set_xlim(-0.5, 7.4)
    save(fig, "fig4_deepseek.png")


# ---------------------------------------------------------------------------------------------------------- tables
DESKTOPS = [("8 GB card (GTX 1070, GTX 1080)", 8, "GPU"), ("11-12 GB card (GTX 1080 Ti, RTX 3060)", 11, "GPU"),
            ("No GPU, 32 GB DDR4 (24 GiB for the model)", 24 / 0.9, "CPU")]
GEMMA_Q4, GEMMA_Q8 = 15.78, 25.0   # unsloth gemma-4-26B-A4B-it-UD-Q4_K_M / Q8_0


def tables():
    rows = "".join(f'<tr><td class="l">{m}</td><td class="l">{q}</td><td>{g:g}</td><td>{laptops_needed(g)}</td></tr>'
                   for m, q, g in SCALE_MODELS)
    t_scale = ('<table><thead><tr><th class="l">Model</th><th class="l">GGUF</th><th>Size (GiB)</th>'
               f'<th>Laptops needed</th></tr></thead><tbody>{rows}</tbody></table>')
    rows = ""
    for name, gib, kind in DESKTOPS:
        use = gib * 0.9
        rows += (f'<tr><td class="l">{name}</td><td>{math.ceil(GEMMA_Q4 / use)}</td><td>{math.ceil(GEMMA_Q8 / use)}</td>'
                 f'<td>{"~30-60" if kind == "GPU" else "~6-12"}</td></tr>')
    t_desk = ('<table><thead><tr><th class="l">Desktop</th><th>PCs for Q4_K_M (15.8 GiB)</th><th>PCs for Q8_0 (25.0 GiB)</th>'
              f'<th>Upper-bound tok/s</th></tr></thead><tbody>{rows}</tbody></table>')
    rows = "".join(f'<tr{" class=key" if q == "Q8_K_XL" else ""}><td class="l">{q}</td><td>{g:g}</td><td>{cards_needed(g)}</td></tr>'
                   for q, g in DS_QUANTS)
    t_ds = ('<table><thead><tr><th class="l">GGUF</th><th>Size (GiB)</th><th>RTX 3090s needed</th></tr></thead>'
            f'<tbody>{rows}</tbody></table>')
    return {"{{T_SCALE}}": t_scale, "{{T_DESKTOP}}": t_desk, "{{T_DEEPSEEK}}": t_ds}


# ---------------------------------------------------------------------------------------------------------- checks
def check_numbers():
    """Every run number quoted in body.html, recomputed from the raw files."""
    def eq(label, got, want):
        assert got == want, f"{label}: data says {got}, paper says {want}"
    eq("26B GiB", f"{MODEL_26B_GIB:.1f}", "25.0")
    eq("26B / biggest budget", f"{MODEL_26B_GIB / 17.8:.1f}", "1.4")
    eq("B3 median all measured", f"{S.median(r['tps'] for r in B3 if r['measured']):.1f}", "12.5")
    eq("B3 range", f"{min(r['tps'] for r in B3):.1f}-{max(r['tps'] for r in B3):.1f}", "12.1-12.6")
    eq("B3 ttft short", f"{med(B3, 'short', 'ttft') / 1000:.1f}", "0.6")
    eq("B3 ttft long", f"{med(B3, 'long', 'ttft') / 1000:.1f}", "0.6")
    eq("B3 ttft edit", f"{med(B3, 'edit', 'ttft') / 1000:.1f}", "1.9")
    eq("B3 requests", f"{sum(1 for r in B3 if r['measured'] and not r['error'])}/{sum(1 for r in B3 if r['measured'])}", "9/9")
    eq("B3 minutes", f"{B3[-1]['end_min']:.1f}", "5.4")
    eq("B2 median", f"{S.median(r['tps'] for r in B2 if r['measured']):.1f}", "11.2")
    eq("B2 range", f"{min(r['tps'] for r in B2 if r['measured']):.1f}-{max(r['tps'] for r in B2 if r['measured']):.1f}", "10.8-11.9")
    eq("B2 minutes", f"{B2[-1]['end_min']:.1f}", "6.1")
    eq("B2 ttft edit", f"{med(B2, 'edit', 'ttft') / 1000:.1f}", "4.7")
    eq("B1 cold short", f"{med(B1, 'short', 'tps'):.1f}", "16.1")
    eq("B1 edit", f"{med(B1, 'edit', 'tps'):.1f}", "9.3")
    eq("B1 last 6", f"{min(r['tps'] for r in B1[-6:]):.1f}-{max(r['tps'] for r in B1[-6:]):.1f}", "8.3-10.3")
    eq("spec edit 0329", f"{med(B1a, 'edit', 'tps'):.1f}->{med(B1sa, 'edit', 'tps'):.1f}", "7.2->12.4")
    eq("spec edit 0408", f"{med(B1, 'edit', 'tps'):.1f}->{med(B1s, 'edit', 'tps'):.1f}", "9.3->14.5")
    eq("spec ratio", f"{med(B1sa, 'edit', 'tps') / med(B1a, 'edit', 'tps'):.1f},{med(B1s, 'edit', 'tps') / med(B1, 'edit', 'tps'):.1f}", "1.7,1.6")
    eq("placement GiB", ",".join(f"{m / 1024:.1f}" for _, m in PLACEMENT_MIB), "0.7,5.6,9.7,9.7")
    eq("cards Q4/Q8/IQ1", f"{cards_needed(144.4)},{cards_needed(150.8)},{cards_needed(76.9)}", "8,8,4")
    eq("laptops DeepSeek Q4 / Kimi Q8 / GLM Q8", f"{laptops_needed(377.6)},{laptops_needed(1016.1)},{laptops_needed(109.4)}", "32,84,9")
    eq("B3 prompt tok/s edit 26B vs 12B", f"{med(B3, 'edit', 'prompt'):.0f},{med(B2, 'edit', 'prompt'):.0f}", "177,59")
    eq("tok time ms", f"{1000 / 12.5:.0f}", "80")
    print("check_numbers: all run numbers match the raw files")


# ---------------------------------------------------------------------------------------------------------- document
def build_html():
    css = open(os.path.join(HERE, "paper.css"), encoding="utf-8").read()
    body = open(os.path.join(HERE, "body.html"), encoding="utf-8").read()
    for k, v in tables().items():
        body = body.replace(k, v)
    html = (f'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, '
            f'initial-scale=1"><title>Oasis: pooling idle laptops to serve a language model none can hold</title>'
            f'<style>{css}</style></head><body><div class="paper">{body}</div></body></html>')
    out = os.path.join(HERE, "oasis_whitepaper.html")
    open(out, "w", encoding="utf-8").write(html)
    # plain prose for the register checker: drop tables, captions, references
    prose = re.sub(r"<table.*?</table>|<div class=\"cap.*?</div>|<section id=\"refs\".*?</section>", " ", body, flags=re.S)
    prose = re.sub(r"<h[12][^>]*>(.*?)</h[12]>", r"\n\n## \1\n\n", prose)
    prose = re.sub(r"</p>|</li>", "\n\n", prose)
    prose = re.sub(r"<[^>]+>", "", prose)
    prose = re.sub(r"&nbsp;", " ", prose).replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
    open(os.path.join(HERE, "check.md"), "w", encoding="utf-8").write(re.sub(r"\n{3,}", "\n\n", prose))
    return out


def print_pdf(html):
    edge = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
    pdf = os.path.join(HERE, "oasis_whitepaper.pdf")
    subprocess.run([edge, "--headless", "--disable-gpu", "--no-pdf-header-footer", f"--print-to-pdf={pdf}",
                    "file:///" + html.replace("\\", "/")], check=True, capture_output=True, timeout=120)
    print("pdf:", pdf)


if __name__ == "__main__":
    for old in os.listdir(FIG):
        os.remove(os.path.join(FIG, old))
    for f in (fig_memory, fig_sustained, fig_scale, fig_deepseek):
        f()
    check_numbers()
    h = build_html()
    print("html:", h)
    if "--no-pdf" not in sys.argv:
        print_pdf(h)
