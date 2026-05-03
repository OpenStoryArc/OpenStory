#!/usr/bin/env python3
"""Orchestrator — run the full pipeline and write artifacts to a dated dir.

Methodology:
  Runs gather → classify → enrich → measure → validate in order, writing each
  intermediate JSON to disk so the pipeline is inspectable. The final
  artifact is `bundle.json` with everything composed; sibling files exist
  for debugging and for the agent's narrator step.

  Output layout:
    out_dir/{date}/
      01_gather.json       # raw sessions in window
      02_classify.json     # + tags
      03_enrich.json       # + files, transcript heads, errors, MCP counts
      04_measure.json      # + metrics (throughput, hot files, health, tokens)
      05_validate.json     # + validation warnings
      bundle.json          # final artifact (== 05_validate.json)
      facts.md             # human-readable fact sheet (no narrative)

Usage:
  python3 scripts/team_day/run.py
  python3 scripts/team_day/run.py --date 2026-05-02 --tz America/New_York
  python3 scripts/team_day/run.py --include-subagents
"""

from __future__ import annotations

import argparse
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import DEFAULT_URL, load_roster, utc_to_local, write_json  # noqa: E402
from gather import gather  # noqa: E402
from classify import classify  # noqa: E402
from enrich import enrich  # noqa: E402
from measure import measure  # noqa: E402
from validate import validate  # noqa: E402


def fact_sheet_md(bundle: dict) -> str:
    """Render a facts-only markdown sheet — no narrative.

    The composer (Claude) reads this to write the report. The sheet's job
    is to make it impossible to hallucinate counts or attributions.
    """
    w = bundle["window"]
    tz = w["tz"]
    lines: list[str] = []
    lines.append(f"# Team Day Facts — {w['date']} ({tz})")
    lines.append("")
    lines.append(f"Window: `{w['utc_start']}` → `{w['utc_end']}` (UTC)")
    lines.append("")

    # Sessions per author
    lines.append("## Sessions by author")
    lines.append("")
    by_author: dict[str, list[dict]] = {}
    for s in bundle["sessions"]:
        a = s.get("tags", {}).get("author", "unknown")
        by_author.setdefault(a, []).append(s)

    for author in sorted(by_author):
        sessions = sorted(by_author[author], key=lambda s: s.get("first_event") or "")
        primaries = [s for s in sessions if s.get("tags", {}).get("role") == "primary"]
        subs = [s for s in sessions if s.get("tags", {}).get("role") == "subagent"]
        lines.append(f"### {author} — {len(sessions)} sessions ({len(primaries)} primary, {len(subs)} subagent)")
        lines.append("")
        lines.append("| EDT start | Session | Repo | Kind | Events | Tools | Files | MCP-OS | Errors | Opening (verbatim) |")
        lines.append("|---|---|---|---|---:|---:|---:|---:|---:|---|")
        for s in sessions:
            tags = s.get("tags", {})
            enr = s.get("enriched") or {}
            opening = enr.get("opening_prompt") or s.get("label") or ""
            opening_short = (opening or "").replace("\n", " ").replace("|", "/")[:80]
            files = enr.get("files_touched") or []
            mcp = (enr.get("mcp_openstory") or {}).get("total", 0)
            errs = len(enr.get("errors") or [])
            label_marker = "" if enr.get("opening_prompt") else " *(label, no transcript)*"
            lines.append(
                f"| {utc_to_local(s.get('first_event'), tz)} "
                f"| `{s.get('session_id','')[:18]}…` "
                f"| {tags.get('repo','?')} "
                f"| {tags.get('kind','?')} "
                f"| {s.get('event_count',0)} "
                f"| {s.get('tool_count',0)} "
                f"| {len(files)} "
                f"| {mcp} "
                f"| {errs} "
                f"| {opening_short}{label_marker} |"
            )
        lines.append("")

    # Throughput
    m = bundle.get("metrics", {})
    t = m.get("throughput", {})
    lines.append("## Throughput")
    lines.append("")
    lines.append(f"- Commits in window: **{t.get('commits_total', 0)}** ({dict(t.get('commits_by_author', {}))})")
    lines.append(f"- Merges in window: **{t.get('merges_total', 0)}** ({dict(t.get('merges_by_author', {}))})")
    if m.get("tokens"):
        tok = m["tokens"]
        ratio = tok.get("ratio_today_over_avg")
        ratio_str = f" ({ratio}× 7d avg)" if ratio is not None else ""
        lines.append(
            f"- Tokens today: **{tok.get('today_total', 0):,}**{ratio_str} "
            f"across {tok.get('today_messages', 0)} messages"
        )
    lines.append("")

    # Hot files
    hot = m.get("hot_files", [])
    if hot:
        lines.append("## Hot files (touched in ≥2 sessions)")
        lines.append("")
        for h in hot[:20]:
            cross = " ⚡cross-author" if h.get("cross_author") else ""
            lines.append(f"- `{h['path']}` — {h['session_count']} sessions, authors: {', '.join(h['authors'])}{cross}")
        lines.append("")

    # Health
    h = m.get("health", {})
    lines.append("## Health")
    lines.append("")
    lines.append(f"- Ghost sessions: **{len(h.get('ghost_sessions', []))}**")
    lines.append(f"- Error sessions: **{len(h.get('error_sessions', []))}**")
    lines.append(f"- Compaction events: **{len(h.get('compactions', []))}**")
    lines.append(f"- Recall sessions: **{len(h.get('recall_sessions', []))}**")
    lines.append("")

    # Validation
    v = bundle.get("validation", {})
    warnings = v.get("warnings", [])
    if warnings:
        lines.append("## Validation warnings")
        lines.append("")
        lines.append("Sessions with warnings should be quoted with care or excluded from the report.")
        lines.append("")
        for w in warnings:
            lines.append(f"- `{w['session_id'][:18]}…` — **{w['rule']}**: {w['detail']}")
        lines.append("")
    else:
        lines.append("## Validation: clean ✓")
        lines.append("")

    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the full team-day pipeline.")
    parser.add_argument(
        "--date",
        default=datetime.now().strftime("%Y-%m-%d"),
        help="Local date YYYY-MM-DD (default: today)",
    )
    parser.add_argument("--tz", default=load_roster()["default_tz"])
    parser.add_argument(
        "--out-dir",
        default="captures/team_day",
        help="Where to write artifact directory (default: captures/team_day)",
    )
    parser.add_argument("--url", default=DEFAULT_URL)
    parser.add_argument("--repo", default=None, help="Path to git repo (default: cwd)")
    parser.add_argument("--include-subagents", action="store_true")
    parser.add_argument(
        "--mode",
        default="started",
        choices=["started", "active"],
        help="started: sessions that began today (default). active: any overlap.",
    )
    args = parser.parse_args()

    out_dir = Path(args.out_dir) / args.date
    out_dir.mkdir(parents=True, exist_ok=True)

    sys.stderr.write(f"team_day: {args.date} ({args.tz}) -> {out_dir}\n")

    sys.stderr.write(f"  [1/5] gather (mode={args.mode})...\n")
    b = gather(args.date, args.tz, args.url, mode=args.mode)
    write_json(b, str(out_dir / "01_gather.json"))

    sys.stderr.write("  [2/5] classify...\n")
    b = classify(b, load_roster())
    write_json(b, str(out_dir / "02_classify.json"))

    sys.stderr.write(f"  [3/5] enrich (include_subagents={args.include_subagents})...\n")
    b = enrich(b, args.url, args.include_subagents)
    write_json(b, str(out_dir / "03_enrich.json"))

    sys.stderr.write("  [4/5] measure...\n")
    b = measure(b, args.url, args.repo)
    write_json(b, str(out_dir / "04_measure.json"))

    sys.stderr.write("  [5/5] validate...\n")
    b = validate(b)
    write_json(b, str(out_dir / "05_validate.json"))
    write_json(b, str(out_dir / "bundle.json"))

    sheet = fact_sheet_md(b)
    (out_dir / "facts.md").write_text(sheet)

    n_sessions = len(b["sessions"])
    n_warn = len(b.get("validation", {}).get("warnings", []))
    sys.stderr.write(
        f"done: {n_sessions} sessions, {n_warn} warnings -> {out_dir}/facts.md\n"
    )


if __name__ == "__main__":
    main()
