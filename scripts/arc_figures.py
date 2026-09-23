#!/usr/bin/env python3
"""Render the figures for an "arc of the work" reel as Kindle-paper PNGs.

Each figure is a still image meant to be a reel stop (kind=image) and to survive
Send-to-Kindle conversion: raster PNG, one hue, large type, no hover layer.
Palette and type follow the Kindle reading aesthetic (warm paper, warm ink,
Charter as the Bookerly stand-in, one amber accent).

Inputs
  --repo PATH          git repo to count commits in (default: cwd)
  --since YYYY-MM-DD   window start (default 2026-06-23)
  --until YYYY-MM-DD   window end (default today)
  --tokens-json PATH   cached daily token rows [[date, total_millions, messages], ...]
                       (fallback when the OpenStory API is down; --api wins when it answers)
  --api URL            OpenStory API base (default http://localhost:3002)
  --prs-json PATH      cached open PRs [[created, number, title], ...] (fallback for gh)
  --out DIR            output directory (default ./out)

Outputs
  out/commits_per_week.png, out/tokens_per_day.png, out/write_surfaces.png,
  out/open_prs.png and out/manifest.json mapping name -> data URL, ready to paste
  into a reel stop's visual.imageHref.

Run --test for the pure helpers.
"""
from __future__ import annotations

import argparse
import base64
import collections
import datetime as dt
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

PAPER = "#f6efe2"      # Kindle sepia paper
INK = "#2b241c"        # warm ink (text token)
INK_2 = "#6b5d4d"      # secondary ink
RULE = "#d9cfbd"       # recessive grid / axis
ACCENT = "#c25a1e"     # one amber accent, validated 3:1+ on PAPER
ACCENT_SOFT = "#e6b899"

MILESTONES = [
    ("2026-06-23", "consent model removed"),
    ("2026-07-04", "#96 UI emerges from the data"),
    ("2026-07-08", "v0.4.0"),
    ("2026-07-17", "#103 Grok, #104 control vocabulary"),
    ("2026-08-07", "#106 Reels"),
    ("2026-08-25", "#112 reel export"),
    ("2026-09-18", "#119 memory hands"),
]

WRITE_SURFACES = [
    ("2026-07-01", "ui.*", "drive the dashboard"),
    ("2026-08-05", "reels/", "author a story"),
    ("2026-09-18", "memory.*", "enrich memory"),
]


# ---------- pure helpers ----------

def iso_week_start(d: dt.date) -> dt.date:
    return d - dt.timedelta(days=d.weekday())


def commits_per_week(dates: list[dt.date]) -> list[tuple[dt.date, int]]:
    c: collections.Counter[dt.date] = collections.Counter(iso_week_start(d) for d in dates)
    if not c:
        return []
    lo, hi = min(c), max(c)
    out = []
    w = lo
    while w <= hi:
        out.append((w, c.get(w, 0)))
        w += dt.timedelta(days=7)
    return out


def fill_days(rows: list[tuple[dt.date, float]], since: dt.date, until: dt.date) -> list[tuple[dt.date, float]]:
    by = {d: v for d, v in rows}
    out, d = [], since
    while d <= until:
        out.append((d, by.get(d, 0.0)))
        d += dt.timedelta(days=1)
    return out


def days_open(created: dt.date, today: dt.date) -> int:
    return (today - created).days


def data_url(png_path: Path) -> str:
    return "data:image/png;base64," + base64.b64encode(png_path.read_bytes()).decode("ascii")


# ---------- data sources ----------

def git_commit_dates(repo: Path, since: str, until: str) -> list[dt.date]:
    out = subprocess.run(
        ["git", "log", f"--since={since}", f"--until={until} 23:59", "--date=short",
         "--format=%ad", "master", "--no-merges"],
        cwd=repo, capture_output=True, text=True, check=True).stdout
    return [dt.date.fromisoformat(l.strip()) for l in out.splitlines() if l.strip()]


def token_rows(api: str, cache: Path | None, days: int) -> list[tuple[dt.date, float]]:
    try:
        with urllib.request.urlopen(f"{api}/api/analytics/daily-token-usage?days={days}", timeout=5) as r:
            data = json.load(r)
        return [(dt.date.fromisoformat(x["date"]), x["total_tokens"] / 1e6) for x in data]
    except Exception:
        if not cache:
            raise
        return [(dt.date.fromisoformat(d), float(m)) for d, m, _ in json.loads(cache.read_text())]


def open_prs(repo: Path, cache: Path | None) -> list[tuple[dt.date, int, str]]:
    try:
        out = subprocess.run(
            ["gh", "pr", "list", "--state", "open", "--limit", "50", "--json", "number,title,createdAt"],
            cwd=repo, capture_output=True, text=True, check=True, timeout=20).stdout
        return [(dt.date.fromisoformat(p["createdAt"][:10]), p["number"], p["title"]) for p in json.loads(out)]
    except Exception:
        if not cache:
            raise
        return [(dt.date.fromisoformat(c), int(n), t) for c, n, t in json.loads(cache.read_text())]


# ---------- rendering ----------

def _style():
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib import font_manager
    serif = "Charter" if any("Charter" in f.name for f in font_manager.fontManager.ttflist) else "Georgia"
    plt.rcParams.update({
        "figure.facecolor": PAPER, "axes.facecolor": PAPER, "savefig.facecolor": PAPER,
        "font.family": serif, "text.color": INK, "axes.labelcolor": INK_2,
        "xtick.color": INK_2, "ytick.color": INK_2, "axes.edgecolor": RULE,
        "axes.spines.top": False, "axes.spines.right": False, "axes.spines.left": False,
        "ytick.left": False, "grid.color": RULE, "grid.linewidth": 0.8,
        "font.size": 16, "axes.titlesize": 24, "axes.titleweight": "normal",
        "axes.titlelocation": "left", "axes.titlepad": 18,
    })
    return plt


def _month_ticks(ax):
    import matplotlib.dates as mdates
    ax.xaxis.set_major_locator(mdates.DayLocator(bymonthday=(1, 15)))
    ax.xaxis.set_major_formatter(mdates.DateFormatter("%b %-d"))
    ax.tick_params(axis="x", length=0)


def _frame(plt, w=12, h=6.75):
    fig, ax = plt.subplots(figsize=(w, h), dpi=100)
    fig.subplots_adjust(left=0.07, right=0.97, top=0.82, bottom=0.16)
    ax.grid(axis="y")
    ax.set_axisbelow(True)
    _month_ticks(ax)
    return fig, ax


def _caption(fig, text):
    fig.text(0.07, 0.03, text, color=INK_2, fontsize=13, ha="left")


def _title(fig, text):
    fig.text(0.07, 0.905, text, color=INK, fontsize=24, ha="left", va="center")


def render_commits(plt, weeks, out: Path):
    fig, ax = _frame(plt)
    xs = [w for w, _ in weeks]
    ys = [n for _, n in weeks]
    ax.bar(xs, ys, width=5.2, color=ACCENT, linewidth=0, align="edge")
    _title(fig, "Commits on master, by week")
    ax.set_ylabel("commits")
    for w, n in weeks:
        if n >= 40:
            ax.text(w + dt.timedelta(days=2.6), n + 3, str(n), ha="center", color=INK, fontsize=14)
    ymax = max(ys) if ys else 1
    ax.set_ylim(0, ymax * 1.22)
    labeled = [(d, l) for d, l in MILESTONES if not l.startswith("consent") and not l.startswith("#103")]
    for i, (d, label) in enumerate(labeled):
        x = dt.date.fromisoformat(d)
        ax.axvline(x, color=INK_2, linewidth=0.8, alpha=0.5)
        last = i == len(labeled) - 1
        ax.text(x + dt.timedelta(days=-0.8 if last else 0.8), ymax * (1.19 if i % 2 == 0 else 1.10),
                label, va="top", ha="right" if last else "left", color=INK_2, fontsize=11.5)
    ax.set_xlim(min(xs) - dt.timedelta(days=1), max(xs) + dt.timedelta(days=9))
    _caption(fig, "Three bursts: the July UI loop, the August reel work, the September memory hands. Rules mark merges.")
    fig.savefig(out); plt.close(fig)


def render_tokens(plt, days, out: Path):
    fig, ax = _frame(plt)
    xs = [d for d, _ in days]
    ys = [v for _, v in days]
    ax.bar(xs, ys, width=0.9, color=ACCENT, linewidth=0, align="center")
    _title(fig, "Tokens per day, millions")
    ax.set_ylabel("millions of tokens")
    top = sorted(days, key=lambda t: -t[1])[:3]
    for d, v in top:
        ax.text(d, v + 15, f"{d.strftime('%b %-d')}\n{v:,.0f}M", ha="center", color=INK, fontsize=13)
    ax.set_xlim(min(xs) - dt.timedelta(days=1), max(xs) + dt.timedelta(days=1))
    _caption(fig, "Quiet weeks are real: late July, mid August, early September. The peak is the memory-hands build day.")
    fig.savefig(out); plt.close(fig)


def render_surfaces(plt, since: dt.date, until: dt.date, out: Path):
    fig, ax = plt.subplots(figsize=(12, 6.75), dpi=100)
    fig.subplots_adjust(left=0.22, right=0.96, top=0.82, bottom=0.16)
    _title(fig, "The mirror grows hands: three write surfaces, one untouched record")
    lanes = [("events.*", "observed, never written")] + \
            [(name, what) for _, name, what in WRITE_SURFACES]
    n = len(lanes)
    for i, (name, what) in enumerate(lanes):
        y = n - 1 - i
        ax.hlines(y, since, until, color=RULE, linewidth=2)
        ax.text(since - dt.timedelta(days=2), y + 0.1, name, ha="right", va="center", color=INK, fontsize=19)
        ax.text(since - dt.timedelta(days=2), y - 0.22, what, ha="right", va="center", color=INK_2, fontsize=12)
    for d, name, _ in WRITE_SURFACES:
        x = dt.date.fromisoformat(d)
        y = n - 1 - [l[0] for l in lanes].index(name)
        ax.hlines(y, x, until, color=ACCENT, linewidth=6, capstyle="round")
        ax.plot([x], [y], marker="o", markersize=14, color=ACCENT, markeredgecolor=PAPER, markeredgewidth=2)
        ax.text(x, y + 0.28, x.strftime("%b %-d"), ha="center", color=INK, fontsize=13)
    ax.set_ylim(-0.7, n - 0.3)
    ax.set_xlim(since - dt.timedelta(days=1), until + dt.timedelta(days=4))
    ax.set_yticks([])
    _month_ticks(ax)
    for s in ("top", "right", "left"):
        ax.spines[s].set_visible(False)
    ax.spines["bottom"].set_color(RULE)
    _caption(fig, "Each hand got its own authored namespace beside the record. The record itself was never written to.")
    fig.savefig(out); plt.close(fig)


def render_prs(plt, prs, today: dt.date, out: Path):
    prs = sorted(prs, key=lambda p: p[0])
    fig, ax = plt.subplots(figsize=(12, 6.75), dpi=100)
    fig.subplots_adjust(left=0.40, right=0.95, top=0.82, bottom=0.18)
    ax.grid(axis="x", color=RULE); ax.set_axisbelow(True)
    labels, ages = [], []
    for created, num, title in prs:
        title = title.replace("→", " to ").replace("—", ",").replace("  ", " ")
        short = title if len(title) <= 42 else title[:40].rstrip() + "…"
        labels.append(f"#{num}  {short}")
        ages.append(days_open(created, today))
    ys = list(range(len(prs)))[::-1]
    ax.barh(ys, ages, height=0.62, color=ACCENT_SOFT, linewidth=0)
    for y, a, (created, num, _) in zip(ys, ages, prs):
        if a <= 45:
            ax.barh(y, a, height=0.62, color=ACCENT, linewidth=0)
        ax.text(a + 1.5, y, f"{a} d", va="center", color=INK, fontsize=13)
    ax.set_yticks(ys); ax.set_yticklabels(labels, fontsize=12.5, color=INK)
    ax.set_xlabel("days open", labelpad=6)
    _title(fig, "Open pull requests, by age")
    for s in ("top", "right", "left"):
        ax.spines[s].set_visible(False)
    ax.tick_params(axis="y", length=0)
    _caption(fig, "Amber: opened in the last six weeks. Pale: the July stream-cap fixes and June spikes still waiting.")
    fig.savefig(out); plt.close(fig)


# ---------- main ----------

def run(args) -> dict:
    plt = _style()
    since = dt.date.fromisoformat(args.since)
    until = dt.date.fromisoformat(args.until) if args.until else dt.date.today()
    out = Path(args.out); out.mkdir(parents=True, exist_ok=True)
    repo = Path(args.repo)

    weeks = commits_per_week(git_commit_dates(repo, args.since, until.isoformat()))
    render_commits(plt, weeks, out / "commits_per_week.png")

    days = fill_days(token_rows(args.api, Path(args.tokens_json) if args.tokens_json else None,
                                (until - since).days + 1), since, until)
    render_tokens(plt, days, out / "tokens_per_day.png")

    render_surfaces(plt, since, until, out / "write_surfaces.png")

    prs = open_prs(repo, Path(args.prs_json) if args.prs_json else None)
    render_prs(plt, prs, until, out / "open_prs.png")

    manifest = {p.stem: data_url(p) for p in sorted(out.glob("*.png"))}
    (out / "manifest.json").write_text(json.dumps(manifest))
    return {k: len(v) for k, v in manifest.items()}


def _test() -> int:
    d = dt.date
    assert iso_week_start(d(2026, 9, 23)) == d(2026, 9, 21)
    wk = commits_per_week([d(2026, 9, 21), d(2026, 9, 23), d(2026, 10, 6)])
    assert wk == [(d(2026, 9, 21), 2), (d(2026, 9, 28), 0), (d(2026, 10, 5), 1)], wk
    assert commits_per_week([]) == []
    fd = fill_days([(d(2026, 1, 2), 5.0)], d(2026, 1, 1), d(2026, 1, 3))
    assert fd == [(d(2026, 1, 1), 0.0), (d(2026, 1, 2), 5.0), (d(2026, 1, 3), 0.0)], fd
    assert days_open(d(2026, 9, 1), d(2026, 9, 23)) == 22
    print("ok: 5 assertions")
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--repo", default=".")
    p.add_argument("--since", default="2026-06-23")
    p.add_argument("--until", default=None)
    p.add_argument("--tokens-json", default=None)
    p.add_argument("--prs-json", default=None)
    p.add_argument("--api", default="http://localhost:3002")
    p.add_argument("--out", default="out")
    p.add_argument("--test", action="store_true")
    a = p.parse_args(argv)
    if a.test:
        return _test()
    sizes = run(a)
    for k, v in sizes.items():
        print(f"{k}: {v:,} chars as data URL")
    return 0


if __name__ == "__main__":
    sys.exit(main())
