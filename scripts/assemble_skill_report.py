"""Assemble drafts/skill_report_data.json for build_skill_report.py.

Merges the per-skill chart specs gathered from the live OpenStory store (each a
JSON dict of {skill: {source, viz}}, e.g. produced by a fan-out of MCP queries
and saved to drafts/_part*.json) and stamps each skill with its presentation
metadata (index, accent colour, title, the question it answers) in reading
order. Output is the data file the renderer consumes.

    python3 scripts/assemble_skill_report.py            # merges drafts/_part*.json
    python3 scripts/assemble_skill_report.py a.json b.json   # explicit parts
"""

import glob
import json
import sys
from pathlib import Path

# Reading order + presentation metadata. Colours mirror the prompt-library deck:
# blue = know your work, green = coach, purple = narrate, orange = recall,
# red = ground your agent, cyan = team.
SECTION_META = [
    ("cost",    "01", "blue",   "/cost · what your sessions cost",        "What have my agent sessions cost me?"),
    ("time",    "02", "blue",   "/time · where your time goes",           "Where does my time actually go?"),
    ("tools",   "03", "blue",   "/tools · what you reach for",            "Which tools and commands do I rely on most?"),
    ("coach",   "04", "green",  "/coach · how you work",                  "Honest feedback on how I work with coding agents."),
    ("arc",     "05", "purple", "/arc · the story so far",                "Trace the story of how this project came to be."),
    ("recap",   "06", "purple", "/recap · what shipped lately",          "What have I worked on lately, grouped by project?"),
    ("standup", "07", "purple", "/standup · today",                       "Write my standup: what I did, what's blocked, what's next."),
    ("recall",  "08", "orange", "/recall · find it again",               "Find the last time I solved this — and exactly how."),
    ("prime",   "09", "red",    "/prime · pick up where you left off",    "Where did we leave off on this project?"),
    ("watch",   "10", "red",    "/watch · live work",                     "Watch the work happening right now and summarise it."),
    ("team",    "11", "cyan",   "/team · who's working",                  "Who on my team is active, and on what?"),
    ("scan",    "12", "blue",   "/scan · before you share",              "Is there anything sensitive in my history?"),
]

REPORT = {
    "title": "Your work, in your own data",
    "title_grad": "your own data",
    "subtitle": "The twelve openstory-skills, run against your live store. The same reports as the "
                "prompt library — but every number here is yours.",
    "window": "the last ~30 days (arc & recall reach across all history)",
}


def assemble(parts: list[dict]) -> dict:
    merged: dict = {}
    for p in parts:
        merged.update(p)
    sections = []
    for skill, ix, color, title, prompt in SECTION_META:
        entry = merged.get(skill)
        if not entry:
            sections.append({"ix": ix, "color": color, "title": title, "prompt": prompt,
                             "source": "—", "viz": [{"type": "note",
                             "text": f"_No data returned for **{skill}**._"}]})
            continue
        sections.append({"ix": ix, "color": color, "title": title, "prompt": prompt,
                         "source": entry.get("source", ""), "viz": entry.get("viz", [])})
    return {**REPORT, "sections": sections}


if __name__ == "__main__":
    paths = sys.argv[1:] or sorted(glob.glob("drafts/_part*.json"))
    if not paths:
        sys.exit("no part files found (drafts/_part*.json) — pass them explicitly")
    parts = [json.loads(Path(p).read_text(encoding="utf-8")) for p in paths]
    out = assemble(parts)
    Path("drafts/skill_report_data.json").write_text(json.dumps(out, indent=2), encoding="utf-8")
    n = sum(1 for s in out["sections"] if s["source"] != "—")
    print(f"Wrote drafts/skill_report_data.json — {len(out['sections'])} sections ({n} with live data)")
    print(f"  parts merged: {', '.join(paths)}")
