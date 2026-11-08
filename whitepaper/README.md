<!-- CONCEPT: the Oasis technical report (PDF + HTML) and the script that rebuilds it from the raw bench files.
WHY: Aayo 2026-11-08: a short report of what we built and measured, written by agents and labelled as such.
DEPENDS ON: test-runs/bench/ (copied into data/), Python 3 + matplotlib, Microsoft Edge for the PDF.
AFFECTS: nothing else in the repo; Devpost and the pitch can link the PDF. -->
# Oasis technical report

`oasis_whitepaper.pdf` is a 6-page proof-of-concept report on Oasis: the system, the measured runs on three
MacBook Airs, and calculated estimates for larger pools. AI agents working for the team (Anthropic's Claude) wrote
it from the team's code, raw benchmark files and notes; the team built Oasis and ran every experiment. It has not
been peer reviewed.

| File | What it is |
|---|---|
| `oasis_whitepaper.pdf` / `.html` | The report (the HTML reads on a phone) |
| `body.html` | The text; edit this, then rebuild |
| `build.py` | Draws the figures from `data/`, fills the generated tables, checks every quoted run number against the raw files, prints the PDF with headless Edge |
| `data/` | Copies of the raw bench files used (B3, B2, B1, B1s; sources listed in `build.py`) |
| `fig/` | Generated figures |

Rebuild: `python whitepaper/build.py` (add `--no-pdf` to skip Edge). It stops if any number in the text differs from
the data.
