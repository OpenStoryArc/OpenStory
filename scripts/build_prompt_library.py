"""Build the OpenStory "prompt library" draft for the website.

Single source of truth for the starter-prompt library. Each prompt carries:
  - its reusable report TEMPLATE (rendered into the markdown reference), and
  - a VIZ spec: structured blocks (stats / bars / table / timeline / steps / …)
    rendered into crisp, consultancy-grade CSS charts on the page.

Examples are frozen snapshots — real output captured from the live store by a
fan-out of category subagents, then anonymized for a public page (people →
roles, projects → a-project, no secrets/hosts/keys). KIND is "real" (queried)
or "illustrative" (representative — clearly labelled).

    --html PATH   the public page: prompt + copy button + collapsible chart
    --md   PATH   the reusable templates reference
    --pdf  PATH   a print-ready PDF: contents (TOC) page + every report expanded
                  inline (rendered via headless Chrome)
    --marp PATH   a Marp slide deck (markdown): contents agenda + one slide per
                  prompt with its example below (render with the Marp CLI)
    --test        self-checks (counts, escaping, leakage guard)
"""

import argparse
import html
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


INTRO = ("Twenty ways to point OpenStory at your own work — to know yourself, sense your team, "
         "and never lose what you've already figured out. Every prompt is distilled from how "
         "OpenStory actually gets used, and each opens a real report, charted from a live store.")

# -- Content ----------------------------------------------------------
# Each prompt: text, hint?, template (md), kind, source, viz (list of blocks).

SECTIONS = [
 {"ix": "01", "color": "blue", "title": "Know your own work",
  "why": "The mirror. What did I do, what did it cost, and is there anything in here I wouldn't want seen?",
  "prompts": [
    {"text": "Summarize everything I've worked on this week across all my projects — group it by project and tell me what actually shipped.",
     "kind": "real",
     "template": "### Week in review — <range>\n<N> sessions · <M> projects · <K> events\n| Project | Sessions | Activity | What shipped |\n**Highlights:** <2-3 shipped outcomes>",
     "source": "GET /api/sessions → group by project  +  git log --since",
     "viz": [
       {"type": "stats", "items": [{"num": "25", "label": "sessions"}, {"num": "4", "label": "projects"}, {"num": "13.2K", "label": "events"}]},
       {"type": "table", "cols": ["Project", "Sess", "Activity", "What shipped"], "align": ["l", "r", "l", "l"],
        "rows": [
          ["a-project", "26", {"bar": 100, "text": "9.3K"}, "store fix · codex view parity · replay tests"],
          ["a-website", "10", {"bar": 29, "text": "2.7K"}, "starter-prompt library page"],
          ["a-deploy", "8", {"bar": 10, "text": "0.9K"}, "distributed NATS leaf hardening"],
          ["research-spike", "1", {"bar": 3, "text": "0.3K"}, "one-off analysis"]]},
       {"type": "note", "text": "**6 commits** landed this week · zero regressions on golden replays."}]},

    {"text": "What have my agent sessions cost me? Give me the total spend, and a tokens-per-day timeline.",
     "kind": "real",
     "template": "### Agent spend — last <N> days\n**Total $<total>** · <sessions> sessions · cache saved $<saved>\nTokens/day: <date> $<cost> <bar>",
     "source": "python3 scripts/token_usage.py --days 7 --by-day",
     "viz": [
       {"type": "stats", "items": [{"num": "$615", "label": "spent · 7 days"}, {"num": "$3,141", "label": "saved by cache", "accent": True}, {"num": "84%", "label": "off retail", "accent": True}]},
       {"type": "bars", "title": "Cost per day", "items": [
          {"label": "Jun 13", "bar": 100, "val": "$304"}, {"label": "Jun 14", "bar": 40, "val": "$122"},
          {"label": "Jun 17", "bar": 29, "val": "$89"}, {"label": "Jun 16", "bar": 14, "val": "$44"},
          {"label": "Jun 12", "bar": 4, "val": "$12"}, {"label": "Jun 18", "bar": 4, "val": "$11"}]},
       {"type": "note", "text": "Biggest single call: **642K** prompt tokens."}]},

    {"text": "Before I share my session history with anyone, scan it for anything sensitive — secrets, API keys, private details, or anything I'd be embarrassed to expose.",
     "hint": "sovereignty check — know your record before anyone else sees it",
     "kind": "illustrative",
     "template": "### Sensitivity scan — <scope>\nScanned <events> events / <sessions> sessions.\n| Category | Hits | Severity |\nSample (redacted): `<category> → [REDACTED]`",
     "source": "regex sweep over event payloads (pattern set from scripts/scrub_check.py) — values never printed",
     "viz": [
       {"type": "stats", "items": [{"num": "~19.6K", "label": "events scanned"}, {"num": "119", "label": "sessions"}, {"num": "2", "label": "high-severity", "accent": True}]},
       {"type": "table", "cols": ["Category", "Exposure", "Severity"], "align": ["l", "l", "r"],
        "rows": [
          ["Personal fs paths", "common", {"chip": "Low"}],
          ["Private IPs / hosts", "some", {"chip": "Medium"}],
          ["Email addresses", "some", {"chip": "Medium"}],
          ["Inline credentials", "a few", {"chip": "High"}],
          ["Named secret assigns", "rare", {"chip": "High"}]]},
       {"type": "note", "tone": "warn", "text": "Sample (redacted): **password=[REDACTED]** · no live API keys (sk- / ghp_ / AKIA) found. Scrub the High-severity sessions before sharing."}]},

    {"text": "Which tools and commands do I rely on most, and where does my time actually go?",
     "kind": "real",
     "template": "### Tool reliance & time — <range>\n<category> <pct>% <bar>\nTop commands: <verb>(<n>) · Read:Write <r>",
     "source": "SQL: group message.assistant.tool_use by tool name + bash first-word",
     "viz": [
       {"type": "bars", "title": "Where time goes", "items": [
          {"label": "Run / shell", "bar": 53, "val": "53%"}, {"label": "Read code", "bar": 21, "val": "21%"},
          {"label": "Write code", "bar": 18, "val": "18%"}, {"label": "Plan / track", "bar": 3, "val": "3%"},
          {"label": "Research", "bar": 1, "val": "1%"}]},
       {"type": "chips", "label": "Top commands", "items": ["grep · 127", "git · 46", "ssh · 31", "python3 · 27", "curl · 24"]},
       {"type": "note", "text": "**Read:Write 1.2:1** · most-used tool: Bash (53% of all calls)."}]},
  ]},

 {"ix": "02", "color": "cyan", "title": "Sense your team",
  "why": "Co-presence. When sessions federate, OpenStory becomes a shared room — you can tell who's in it.",
  "prompts": [
    {"text": "Who on my team has an active session right now, and what is each person working on?",
     "kind": "illustrative",
     "template": "# Team — Active Now (≤ <N> min)\n| Teammate | Project | Branch | Idle |\nLive now: <count> · Idle: <count>",
     "source": "GET /api/sessions → group by host/user, keep last_event within window",
     "viz": [
       {"type": "stats", "items": [{"num": "2", "label": "live now", "accent": True}, {"num": "1", "label": "idle < 1h"}, {"num": "4", "label": "federated hosts"}]},
       {"type": "table", "cols": ["", "Teammate", "Project", "Branch", "Idle"], "align": ["l", "l", "l", "l", "r"],
        "rows": [
          [{"dot": "live"}, "Teammate A", "a-project · UI", "master", "<1m"],
          [{"dot": "live"}, "Teammate B", "a-project", "feat/macos-watcher", "2m"],
          [{"dot": "idle"}, "Teammate C", "project-x", "feat/federation", "1h 42m"]]}]},

    {"text": "Summarize my teammates' sessions from the last day — one short paragraph each.",
     "kind": "real",
     "template": "# Teammate Recap — last 24h\n### <role> · <n> sessions · <events> events\n<one paragraph: projects, branches, the thread>",
     "source": "GET /api/sessions → filter teammate + last_event within 24h",
     "viz": [
       {"type": "cards", "items": [
          {"title": "Teammate A", "sub": "6 sessions · ~620 events", "text": "UI prototype + agentic-team repo. Pulled latest on master, stood up a Dockerised NATS server, opened a thread to find dev-loop friction. Branches: master, main, sprint-9/dev-loop-rebuild."},
          {"title": "Teammate B", "sub": "1 session · 296 events", "text": "One deep codex run on project-x; no feature branch."}]}]},

    {"text": "Show me live activity across the team in the last hour — who's streaming, on what branch.",
     "kind": "illustrative",
     "template": "# Live Stream — last hour\n<role> <proj> <branch> <status> <events>\nStreaming: <count> · Branches: <list>",
     "source": "GET /api/sessions → keep last_event within 60 min, status=ongoing",
     "viz": [
       {"type": "stats", "items": [{"num": "2", "label": "streaming", "accent": True}, {"num": "2", "label": "branches active"}]},
       {"type": "table", "cols": ["", "Teammate", "Project", "Branch", "Events"], "align": ["l", "l", "l", "l", "r"],
        "rows": [
          [{"dot": "live"}, "Teammate B", "a-project", "feat/macos-watcher", "17"],
          [{"dot": "live"}, "Teammate C", "project-x", "chore/idle-watcher", "306"]]}]},

    {"text": "Find a teammate's sessions and summarize what they've been focused on this week, so I can sync before we talk.",
     "kind": "real",
     "template": "# Sync Prep — <role>, this week\nSessions <n> · Events <total>\nTop projects: <proj> <bar> <n>\nThemes: <…> · Talk about: <…>",
     "source": "GET /api/sessions → filter user; aggregate project / branch / events",
     "viz": [
       {"type": "stats", "items": [{"num": "62", "label": "sessions · this wk"}, {"num": "~10.3K", "label": "events"}]},
       {"type": "bars", "title": "Top projects", "items": [
          {"label": "a-project · UI", "bar": 100, "val": "25"}, {"label": "agentic-team", "bar": 60, "val": "15"}, {"label": "a-project", "bar": 52, "val": "13"}]},
       {"type": "chips", "label": "Branches", "items": ["master (11)", "main (11)", "sprint-1/wire-live-data (9)"]},
       {"type": "note", "text": "**Themes:** live-data wiring · dev-loop friction · sprint-9 rebuild. **Talk about:** the sprint-9 rebuild — their heaviest thread."}]},
  ]},

 {"ix": "03", "color": "purple", "title": "Narrate the story",
  "why": "Your session log is canonical memory. Turn it into a story you can read, share, or stand up on.",
  "prompts": [
    {"text": "Trace the story of how this project came to be from my session history. Highlight the key decisions and turning points — and script it so the result is deterministic.",
     "hint": "deterministic = reproducible, not a one-off vibe summary",
     "kind": "real",
     "template": "# How <project> Came To Be\n> Deterministic: git log + the session store; re-run identical.\n| Date | Milestone | Why it mattered |\n**Arc:** <throughline>",
     "source": "git log per milestone + sessions table (deterministic re-run)",
     "viz": [
       {"type": "timeline", "items": [
          {"date": "2026-03-07", "title": "First exploratory sessions", "note": "mapped the Rust + React shape before any code"},
          {"date": "2026-03-21", "title": "Initial commit", "note": "the project formally begins"},
          {"date": "2026-03-24", "title": "Multi-agent + first deploy", "note": "one agent → many; first live deploy"},
          {"date": "2026-03-31", "title": "Vector search → SQLite FTS", "note": "cut a heavy dependency — “your data is yours”"},
          {"date": "2026-04-05", "title": "Five-layer pipeline + Story tab", "note": "events → turns → sentences locks in"},
          {"date": "2026-06-11", "title": "Security audit · 6 CVEs fixed", "note": "production-readiness gate"},
          {"date": "2026-06-13", "title": "Federation validated", "note": "single-machine → multi-machine streaming"}],
        "arc": "A read-only file watcher → a federated, multi-agent store — every step preserving “observe, never interfere.”"}]},

    {"text": "Compile every session related to <topic> and narrate the arc, start to finish.",
     "kind": "real",
     "template": "# The \"<topic>\" Arc — <N> sessions · <span>\n| Date | Phase | What happened |\n**Outcome:** <where it landed>",
     "source": "GET /api/search?q=<topic>  +  git log",
     "viz": [
       {"type": "stats", "items": [{"num": "63", "label": "sessions"}, {"num": "6 days", "label": "Jun 13–19"}]},
       {"type": "timeline", "items": [
          {"date": "Jun 13", "title": "Spike", "note": "“get our networks to talk” → a routing design report"},
          {"date": "Jun 13", "title": "Design", "note": "agents map host-in-subject routing"},
          {"date": "Jun 14", "title": "Build", "note": "host encoded in event subject; node auto-discovers the hub"},
          {"date": "Jun 16", "title": "Polish", "note": "UI copy: “configured” vs “live sources”"},
          {"date": "Jun 18", "title": "Reflect", "note": "state-of-work reflection consolidates the arc"}],
        "arc": "Federation validated end-to-end; one binary now joins a shared hub."}]},

    {"text": "Write me a standup update from today's sessions: what I did, what's blocked, and what's next.",
     "kind": "real",
     "template": "# Standup — <date>\nDid: <bullets> · Blocked: <…> · Next: <…>\n_<N> sessions · <M> turns · top tools_",
     "source": "scripts/sessionstory.py <id> (today's sessions)",
     "viz": [
       {"type": "groups", "cols": [
          {"title": "Did", "items": ["State-of-work reflection across git + the store", "Triaged a big batch of WIP into shippable value", "Reconciled a second agent's parallel work", "Planned an extraction, executed it, opened a PR"]},
          {"title": "Blocked", "tone": "warn", "items": ["Waiting on cross-device data confirmation"]},
          {"title": "Next", "items": ["Finish the extraction", "Land the open PR after review"]}]},
       {"type": "note", "text": "1 session · 44 turns · top tools: **tool_use 211** · file.snapshot 70 · 24 errors handled."}]},
  ]},

 {"ix": "04", "color": "green", "title": "Coach yourself",
  "why": "The mirror turned evaluative. Not “what happened” but “what does what-happened say about how I work.”",
  "prompts": [
    {"text": "Analyze my sessions from the last month and give me honest feedback on my prompt engineering — what I do well, and what I could do better.",
     "kind": "real",
     "template": "# Prompt Engineering Scorecard — <window>\n| Metric | Value | Read |\n**Do well:** <…> · **Grow:** <…>",
     "source": "SQL over prompt lengths + turns-per-session",
     "viz": [
       {"type": "stats", "items": [{"num": "424", "label": "sessions · 30d"}, {"num": "835", "label": "prompts"}]},
       {"type": "table", "cols": ["Metric", "Value", "Read"], "align": ["l", "r", "l"],
        "rows": [
          ["Median prompt length", "161c", "terse by default"],
          ["Long prompts (>600c)", "35%", "spec-grade briefs"],
          ["Short prompts (<80c)", "38%", "one-liners"],
          ["One-shot sessions", "83%", "fire-and-forget"],
          ["Avg turns / session", "10.4", "runs long unattended"]]},
       {"type": "note", "text": "**Strength:** 35% are detailed briefs that set up 10+ autonomous turns. **Grow:** the 38% of <80-char prompts have no follow-up to course-correct under an 83% one-shot rate — invest the brief up front."}]},

    {"text": "Find where my sessions tend to stall, loop, or repeat work. What are my recurring failure patterns?",
     "kind": "real",
     "template": "# Failure-Pattern Report — <window>\n| Signal | Rate | Meaning |\n**Recurring:** <pattern — evidence + fix>",
     "source": "SQL: repeated tool-call detection + error / compaction counts",
     "viz": [
       {"type": "table", "cols": ["Signal", "Rate", "Meaning"], "align": ["l", "r", "l"],
        "rows": [
          ["3+ identical tool calls / session", "27%", "loop / thrash"],
          ["File edited 5+× in a session", "46×", "rework churn"],
          ["Tool-result errors", "1.7%", "clean execution"],
          ["Sessions hitting an error", "3%", "rarely blocked"],
          ["Sessions hitting compaction", "1%", "context bounded"]]},
       {"type": "note", "tone": "warn", "text": "**#1 failure mode: edit-thrash** — 89 Edit loops vs 28 Bash. Fix: read fully, edit once. Execution is healthy; the waste is iteration churn."}]},

    {"text": "What direction has my work been pointing lately? Cluster my recent sessions by theme.",
     "kind": "real",
     "template": "# Work Direction — <window>\n| Theme | Share |\n**Through-line:** <where it's heading>",
     "source": "keyword-bucketing of prompt text + tool histogram",
     "viz": [
       {"type": "bars", "title": "Themes · last 30 days", "items": [
          {"label": "Docs / research", "bar": 40, "val": "40%"}, {"label": "Deploy / infra", "bar": 24, "val": "24%"},
          {"label": "Security / audit", "bar": 12, "val": "12%"}, {"label": "Data pipeline", "bar": 9, "val": "9%"},
          {"label": "Testing / E2E", "bar": 7, "val": "7%"}, {"label": "UI / frontend", "bar": 6, "val": "6%"}]},
       {"type": "note", "text": "**Through-line:** shifted from building features to shipping & explaining — 64% of effort is docs + deploy/infra."}]},
  ]},

 {"ix": "05", "color": "orange", "title": "Recall anything",
  "why": "The store is associative memory across every session — far more than one conversation can hold.",
  "prompts": [
    {"text": "Find the last time I solved <problem>, and show me exactly how I did it.",
     "kind": "real",
     "template": "PROBLEM: <problem>\nLAST SOLVED: <date> · <project>\nROOT CAUSE / STEPS / VERIFIED BY: <…>",
     "source": "GET /api/search?q=max_file+stream+crash → tool_use commands",
     "viz": [
       {"type": "meta", "rows": [
          ["Problem", "NATS JetStream stream creation crashes on first boot"],
          ["Last solved", "2026-06-13 · a-project"],
          ["Root cause", "store quota too small (max_file < ~1.3GB)"]]},
       {"type": "steps", "items": [
          "cp nats.conf nats.conf.bak",
          "set jetstream { store_dir: <path>, max_file: 4GB }",
          "nats-server -c nats.conf -t          # validate",
          "kill -HUP $(pgrep -f nats-server)    # hot reload, no restart"]},
       {"type": "note", "text": "**Verified:** curl …:8222/leafz → healthy stream. Also hit 2026-06-10; earliest 2026-04-30."}]},

    {"text": "Did I ever set up <X>? Locate the session and pull out the precise commands.",
     "kind": "real",
     "template": "SET UP <X>? → YES · <date> · <project>\nTHE EXACT COMMANDS / PROOF: <…>",
     "source": "GET /api/search?q=nats+leaf+hub → Bash commands",
     "viz": [
       {"type": "meta", "rows": [["Set up", "NATS leaf-node federation → YES"], ["Where", "2026-06-13 · a-project"]]},
       {"type": "code", "lines": [
          "$ cp nats.conf nats.conf.bak",
          "$ source .env.federation              # NATS_LEAF_URL from env, not git",
          "$ printf 'leafnodes { remotes [ {url:\"%s\"} ] }\\n' \"$NATS_LEAF_URL\" \\",
          "      > deploy/leaf-remotes.generated.conf",
          "$ nats-server -c nats.conf -t         # validate",
          "$ kill -HUP $(pgrep -f nats-server)   # reload, no downtime"]},
       {"type": "note", "text": "**Proof:** curl …:8222/leafz → leaf connection live."}]},

    {"text": "Search my sessions for <topic> and list every session that touched it, newest first.",
     "kind": "real",
     "template": "TOPIC: <topic> — <N> sessions, <M> mentions\n<date> session <id> <hits>\nNEWEST: <gist>",
     "source": "GET /api/search?q=<topic> (grouped by session, newest first)",
     "viz": [
       {"type": "stats", "items": [{"num": "8", "label": "sessions"}, {"num": "894", "label": "mentions"}]},
       {"type": "list", "items": [
          {"date": "Jun 19", "label": "session <id> · a-project", "meta": "2 hits"},
          {"date": "Jun 18", "label": "session <id> · a-project", "meta": "4 hits"},
          {"date": "Jun 17", "label": "session <id> · a-project", "meta": "1 hit"},
          {"date": "Jun 13", "label": "session <id> · a-project", "meta": "4 hits"},
          {"date": "Jun 12", "label": "session <id> · a-project", "meta": "5 hits"}]},
       {"type": "note", "text": "Newest: wiring the NATS leaf over the tailnet so a remote agent streams home. (+3 older)"}]},
  ]},

 {"ix": "06", "color": "red", "title": "Ground your agent",
  "why": "Tell your agent to lean on the record instead of guessing — the highest-leverage habit of all.",
  "prompts": [
    {"text": "Before you start, query OpenStory for prior context on this project and pick up where the last session left off.",
     "kind": "real",
     "template": "Prior context — <project>\nLast session / worked on / open threads / resume here: <…>",
     "source": "GET /api/sessions (newest) + /api/sessions/{id}/records (or sessionstory.py {id} --unfinished)",
     "viz": [
       {"type": "meta", "rows": [
          ["Last session", "2026-06-19 · branch chore/x · 312 events"],
          ["Worked on", "mined the store for common prompts → built a copy-paste prompt library + live demos"],
          ["Tools", "Bash×36 · Edit×23 · Read×5 · Write×4 · Agent×4"],
          ["Open threads", "wire demos to live data · keep copy public-safe"]]},
       {"type": "note", "text": "**Resume here →** 6 template subagents were mid-flight; collect results and wire the live examples into the page."}]},

    {"text": "Use OpenStory to check whether we've hit this error before — and what fixed it — before debugging from scratch.",
     "kind": "real",
     "template": "Error check — \"<signature>\"\nSeen before / context / last fix / recommendation: <…>",
     "source": "GET /api/search?q=Address+already+in+use  + system.error tally",
     "viz": [
       {"type": "stats", "items": [{"num": "yes", "label": "seen before", "accent": True}, {"num": "113", "label": "bind / conn errors"}, {"num": "174×", "label": "lsof+kill in record"}]},
       {"type": "code", "lines": [
          "lsof -ti:3002,5173,4222     # find the holders",
          "kill <pids>; sleep 1        # release them, then re-boot"]},
       {"type": "note", "text": "A leftover server / Docker stack holds the port at boot. **Free the ports first — don't re-debug the bind.** (docker-mode: compose down)"}]},

    {"text": "Watch the work happening on <branch> through OpenStory and summarize it for me as it streams.",
     "kind": "illustrative",
     "template": "Live on <branch> — <timestamp>\n<time> <tool> <target>\nRollup: <N> sessions · <M> events",
     "source": "SQL: recent tool_use for the branch. Live ticker → subscribe_session (WebSocket)",
     "viz": [
       {"type": "feed", "items": [
          {"time": "01:14:39", "tool": "Agent", "text": "fan out report-template subagent"},
          {"time": "01:14:10", "tool": "Agent", "text": "fan out next subagent"},
          {"time": "23:27:07", "tool": "Bash", "text": "cd rs && cargo test …  (verifying watcher)"},
          {"time": "23:26:59", "tool": "Edit", "text": "rs/src/watcher.rs  (4 rapid edits)"},
          {"time": "23:25:16", "tool": "Bash", "text": "write prompt-library draft"}]},
       {"type": "stats", "items": [{"num": "3", "label": "sessions"}, {"num": "1,438", "label": "events"}]},
       {"type": "note", "text": "Replayed from the record (not a live socket). A true ticker → **subscribe_session** (WebSocket)."}]},
  ]},
]


# -- Leakage guard ----------------------------------------------------

_DENY = re.compile(r"\b(katie|kloughra|maxglassie|ycombinator|a16z|dora-metrics|bobby|founder)\b", re.I)


def leakage(text: str) -> list[str]:
    return sorted(set(m.group(0) for m in _DENY.finditer(text)))


# -- Block renderers --------------------------------------------------

def _e(s) -> str:
    return html.escape(str(s))


def _md(text: str) -> str:
    """Escape, then turn **bold** into <b> (notes carry literal '<', e.g. '<80c')."""
    return re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", _e(text))


def _cell(c, align: str) -> str:
    cls = ' class="r"' if align == "r" else ""
    if isinstance(c, dict):
        if "chip" in c:
            k = {"High": "hi", "Medium": "med", "Low": "lo"}.get(c["chip"], "lo")
            inner = f'<span class="chip-sev {k}">{_e(c["chip"])}</span>'
        elif "dot" in c:
            on = c["dot"] == "live"
            inner = f'<span class="dot {"on" if on else "off"}">{"●" if on else "○"}</span>{_e(c.get("text", ""))}'
        elif "bar" in c:
            inner = f'<span class="cellbar"><span style="width:{int(c["bar"])}%"></span></span>{_e(c.get("text", ""))}'
        else:
            inner = _e(c.get("text", ""))
    else:
        inner = _e(c)
    return f"<td{cls}>{inner}</td>"


def _block(b: dict) -> str:
    t = b["type"]
    if t == "stats":
        cells = "".join(f'<div class="st{" ac" if it.get("accent") else ""}">'
                         f'<div class="n">{_e(it["num"])}</div><div class="l">{_e(it["label"])}</div></div>'
                         for it in b["items"])
        return f'<div class="vstats">{cells}</div>'
    if t == "bars":
        title = f'<div class="vtitle">{_e(b["title"])}</div>' if b.get("title") else ""
        rows = "".join(f'<div class="vbar"><span class="bl">{_e(it["label"])}</span>'
                       f'<span class="bt"><span class="bf" style="width:{int(it["bar"])}%"></span></span>'
                       f'<span class="bv">{_e(it["val"])}</span></div>' for it in b["items"])
        return f'<div>{title}<div class="vbars">{rows}</div></div>'
    if t == "table":
        align = b.get("align", ["l"] * len(b["cols"]))
        rcls = ' class="r"'
        th = "".join(f"<th{rcls if align[i] == 'r' else ''}>{_e(c)}</th>" for i, c in enumerate(b["cols"]))
        trs = "".join("<tr>" + "".join(_cell(c, align[i]) for i, c in enumerate(row)) + "</tr>" for row in b["rows"])
        return f'<table class="vtable"><thead><tr>{th}</tr></thead><tbody>{trs}</tbody></table>'
    if t == "timeline":
        evs = "".join(f'<div class="ev"><div class="d">{_e(e["date"])}</div>'
                      f'<div class="t">{_e(e["title"])}</div><div class="nt">{_e(e["note"])}</div></div>'
                      for e in b["items"])
        arc = f'<div class="tlarc">{_e(b["arc"])}</div>' if b.get("arc") else ""
        return f'<div class="vtl">{evs}</div>{arc}'
    if t == "cards":
        cs = "".join(f'<div class="vcard"><div class="ch">{_e(c["title"])}'
                     f'<span class="cs">{_e(c["sub"])}</span></div><div class="ct">{_e(c["text"])}</div></div>'
                     for c in b["items"])
        return f'<div class="vcards">{cs}</div>'
    if t == "groups":
        gs = "".join(f'<div class="vgroup{" warn" if g.get("tone") == "warn" else ""}">'
                     f'<div class="gh">{_e(g["title"])}</div><ul>'
                     + "".join(f"<li>{_e(x)}</li>" for x in g["items"]) + "</ul></div>"
                     for g in b["cols"])
        return f'<div class="vgroups">{gs}</div>'
    if t == "meta":
        rows = "".join(f"<dt>{_e(k)}</dt><dd>{_e(v)}</dd>" for k, v in b["rows"])
        return f'<dl class="vmeta">{rows}</dl>'
    if t == "steps":
        return '<div class="vsteps">' + "".join(f'<div class="vstep"><code>{_e(s)}</code></div>' for s in b["items"]) + "</div>"
    if t == "code":
        return f'<div class="vcode">{_e(chr(10).join(b["lines"]))}</div>'
    if t == "list":
        items = "".join(f'<div class="li"><span class="ld">{_e(i["date"])}</span>'
                        f'<span>{_e(i["label"])}</span><span class="lm">{_e(i["meta"])}</span></div>' for i in b["items"])
        return f'<div class="vlist">{items}</div>'
    if t == "feed":
        rows = "".join(f'<div class="fr"><span class="ft">{_e(i["time"])}</span>'
                       f'<span class="fk">{_e(i["tool"])}</span><span class="fx">{_e(i["text"])}</span></div>' for i in b["items"])
        return f'<div class="vfeed">{rows}</div>'
    if t == "chips":
        label = f'<span class="cl">{_e(b["label"])}</span>' if b.get("label") else ""
        return f'<div class="vchips">{label}' + "".join(f'<span class="vchip">{_e(x)}</span>' for x in b["items"]) + "</div>"
    if t == "note":
        return f'<div class="vnote{" warn" if b.get("tone") == "warn" else ""}">{_md(b["text"])}</div>'
    raise ValueError(f"unknown block type: {t}")


# -- Page renderer ----------------------------------------------------

def _prompt_html(p: dict, pid: str = "", num: int = 0, expand: bool = False) -> str:
    hint = f'<span class="hint">{_e(p["hint"])}</span>' if p.get("hint") else ""
    cls = "real" if p["kind"] == "real" else "illus"
    txt = "real · live data" if p["kind"] == "real" else "illustrative"
    viz = "".join(_block(b) for b in p["viz"])
    idattr = f' id="{pid}"' if pid else ""
    tag = f'<span class="pnum">{num:02d}</span>' if num else ""
    openattr = " open" if expand else ""  # PDF/print variant ships every report expanded
    return (f'<div class="prompt"{idattr}>{tag}<p>{_e(p["text"])}</p>{hint}'
            f'<details class="ex"{openattr}><summary>open report <span class="extag {cls}">{txt}</span></summary>'
            f'<div class="viz">{viz}<div class="src">{_e(p["source"])}</div></div></details></div>')


def render_toc() -> str:
    """A printable contents block: every prompt, grouped by section, linking to its card."""
    out, n = [], 0
    for s in SECTIONS:
        items = []
        for p in s["prompts"]:
            n += 1
            items.append(f'<li><a href="#p{n}"><span class="tn">{n:02d}</span>'
                         f'<span class="tt">{_e(p["text"])}</span></a></li>')
        out.append(f'<div class="tocsec {s["color"]}"><div class="toch">'
                   f'<span class="ix">{s["ix"]}</span><h3>{_e(s["title"])}</h3></div>'
                   f'<ol class="toclist">{"".join(items)}</ol></div>')
    return f'<nav class="toc"><h2>Contents — 20 prompts</h2>{"".join(out)}</nav>'


# Each theme sets the design tokens; the rest of the CSS derives from them
# (semantic chip/track colors use color-mix, so light + dark both work).
_SANS = "Inter,ui-sans-serif,system-ui,sans-serif"
_SERIF = '"Source Serif 4",Georgia,Cambria,"Times New Roman",serif'
_GF = "https://fonts.googleapis.com/css2?family="  # google fonts base
_LINK_INTER = _GF + "Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap"
_LINK_AISTACK = _GF + "Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&family=Source+Serif+4:opsz,wght@8..60,400;8..60,600;8..60,700&display=swap"
_LINK_GTOWN = _GF + "Public+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&family=Castoro:ital@0;1&display=swap"
THEMES = {
 "aistack": {  # editorial light, warm-white + vivid purple + serif heads (Substack "AI Stack")
   "bg": "#ffffff", "bg2": "#f7f6f3", "panel": "#ffffff", "panel2": "#f7f6f3", "line": "#e6e3de",
   "ink": "#1a1a1a", "ink2": "#3a3a3c", "dim": "#8a8a8e",
   "blue": "#2f6bff", "cyan": "#0e9aa7", "green": "#1f8f4e", "purple": "#ab40ff", "orange": "#c2680a", "red": "#e23b30",
   "grad1": "#ab40ff", "grad2": "#6a5bff", "glow": "#ab40ff14",
   "shadow": "0 1px 2px rgba(0,0,0,.04), 0 10px 30px rgba(30,15,50,.06)", "r": "14px", "fsans": _SANS, "fdisplay": _SERIF, "link": _LINK_AISTACK},
 "georgetown": {  # institutional report: navy + gold brand, vivid categorical charts, Castoro serif
   "bg": "#ffffff", "bg2": "#f2f5f8", "panel": "#ffffff", "panel2": "#f2f5f8", "line": "#d8e0ea",
   "ink": "#13233d", "ink2": "#3c4a5e", "dim": "#79869a",
   "blue": "#0074cc", "cyan": "#1f7a8c", "green": "#266150", "purple": "#6b3fa0", "orange": "#e08a00", "red": "#de354c",
   "grad1": "#144175", "grad2": "#f9a21c", "glow": "#14417510",
   "shadow": "0 1px 2px rgba(16,32,60,.05), 0 10px 30px rgba(16,32,60,.06)", "r": "10px",
   "fsans": '"Public Sans",Inter,system-ui,sans-serif', "fdisplay": '"Castoro",Georgia,serif', "link": _LINK_GTOWN},
 "midnight": {  # vivid, high-contrast dark — "more color" than Tokyo Night
   "bg": "#090a11", "bg2": "#10121f", "panel": "#141627", "panel2": "#1b1e34", "line": "#272b46",
   "ink": "#eef2ff", "ink2": "#b3bce0", "dim": "#7b86ac",
   "blue": "#6ea8fe", "cyan": "#34d8ee", "green": "#3ddc97", "purple": "#c08cff", "orange": "#ffc24b", "red": "#ff7a93",
   "grad1": "#6ea8fe", "grad2": "#34d8ee", "glow": "#2a44b855", "shadow": "none", "r": "14px", "fsans": _SANS, "fdisplay": _SANS, "link": _LINK_INTER},
 "apple": {  # crisp light, Apple-blue, generous whitespace + soft depth
   "bg": "#ffffff", "bg2": "#f5f5f7", "panel": "#ffffff", "panel2": "#f5f5f7", "line": "#e5e5ea",
   "ink": "#1d1d1f", "ink2": "#424245", "dim": "#86868b",
   "blue": "#0071e3", "cyan": "#0098a6", "green": "#1a9c3e", "purple": "#5e5ce6", "orange": "#c25e00", "red": "#e0322a",
   "grad1": "#0071e3", "grad2": "#9f5cff", "glow": "#0071e312",
   "shadow": "0 1px 2px rgba(0,0,0,.05), 0 10px 30px rgba(0,0,0,.05)", "r": "18px", "fsans": _SANS, "fdisplay": _SANS, "link": _LINK_INTER},
 "airbnb": {  # warm light, Rausch coral, friendly + rounded
   "bg": "#ffffff", "bg2": "#f7f7f7", "panel": "#ffffff", "panel2": "#fbfbfb", "line": "#ebebeb",
   "ink": "#222222", "ink2": "#484848", "dim": "#767676",
   "blue": "#3b7dd8", "cyan": "#008489", "green": "#1a7a2e", "purple": "#a61d55", "orange": "#b8730a", "red": "#ff385c",
   "grad1": "#ff385c", "grad2": "#ff9156", "glow": "#ff385c12",
   "shadow": "0 6px 16px rgba(0,0,0,.08)", "r": "20px", "fsans": _SANS, "fdisplay": _SANS, "link": _LINK_INTER},
}


def root_css(theme: str) -> str:
    t = THEMES[theme]
    decls = "".join(f"--{k}:{v};" for k, v in t.items() if k != "link")
    return f":root{{{decls}}}"


def render_html(theme: str = "aistack", expand: bool = False) -> str:
    if theme not in THEMES:
        raise ValueError(f"unknown theme {theme!r}; choose from {list(THEMES)}")
    secs, n = [], 0
    for s in SECTIONS:
        cards = []
        for p in s["prompts"]:
            n += 1
            cards.append(_prompt_html(p, pid=f"p{n}", num=n, expand=expand))
        cards_html = "\n".join(cards)
        secs.append(f'<section class="{s["color"]}"><div class="sechead"><span class="ix">{s["ix"]}</span>'
                    f'<h2>{_e(s["title"])}</h2></div><p class="why">{_e(s["why"])}</p>'
                    f'<div class="prompts">{cards_html}</div></section>')
    return _PAGE.format(css=root_css(theme) + _CSS, intro=_e(INTRO), toc=render_toc(),
                        body="\n".join(secs), js=_COPY_JS, fontlink=THEMES[theme]["link"])


def render_md() -> str:
    out = ["# OpenStory report templates\n",
           "The reusable report shapes behind the starter prompts — the spec each answer fills. "
           "Generated by `scripts/build_prompt_library.py`; rendered, charted examples live on "
           "`prompt-library.html`. Anonymized for public use.\n"]
    for s in SECTIONS:
        out.append(f"\n## {s['ix']} · {s['title']}\n\n_{s['why']}_\n")
        for i, p in enumerate(s["prompts"], 1):
            out.append(f"### {s['ix']}.{i} {p['text']}\n")
            out.append("```\n" + p["template"] + "\n```")
            out.append(f"**Source:** `{p['source']}` · _{p['kind']}_\n")
    return "\n".join(out)


def _chart_css() -> str:
    """The chart/viz CSS, sliced live from the page `_CSS` so the deck and the web
    page share one source of truth (no duplication, no drift). Pulls the
    per-section accent classes plus the whole report-canvas block."""
    accents = _CSS[_CSS.index("/* per-section accent"):_CSS.index("  .prompts {")]
    charts = _CSS[_CSS.index("/* report canvas */"):_CSS.index("  footer {")]
    return accents + charts


# Deck-only layout (uses the same --tokens as the page; no Python interpolation
# needed, so braces stay literal). Brand colours come from root_css(theme).
_DECK_CSS = """
  section { background:radial-gradient(1200px 640px at 84% -14%, var(--glow), transparent), var(--bg);
    color:var(--ink); font-family:var(--fsans); font-size:21px; padding:40px 60px; --ac:var(--blue);
    justify-content:flex-start !important; }   /* top-align (beat theme's centering) */
  section .viz { padding:16px 20px; }            /* slightly tighter canvas on slides */
  section .vtl .ev { padding-bottom:10px; }      /* fit the densest 7-event timeline */
  section h1, section h2, section h3 { font-family:var(--fdisplay); color:var(--ink); letter-spacing:-.02em; font-weight:700; }
  section code { font-family:"JetBrains Mono",monospace; background:var(--bg2); border:1px solid var(--line); border-radius:6px; padding:1px 6px; font-size:.8em; color:var(--ink2); }
  section::after { color:var(--dim); font-family:"JetBrains Mono",monospace; font-size:13px; }
  /* breadcrumb */
  .crumb { display:flex; align-items:center; gap:10px; font:600 12.5px/1 "JetBrains Mono",monospace; letter-spacing:.12em; text-transform:uppercase; color:var(--dim); margin-bottom:14px; }
  .crumb .ix { color:var(--ac); }
  .crumb .tag { margin-left:auto; font-size:10px; padding:4px 10px; border:1px solid var(--line); border-radius:20px; color:var(--dim); letter-spacing:.1em; }
  .crumb .tag.real { color:var(--green); border-color:color-mix(in srgb,var(--green) 45%,var(--line)); }
  .crumb .tag.illus { color:var(--orange); border-color:color-mix(in srgb,var(--orange) 45%,var(--line)); }
  /* prompt headline */
  .q { font-family:var(--fdisplay); font-size:29px; line-height:1.16; color:var(--ink); margin:0 0 18px; font-weight:700; }
  .q .qn { color:var(--ac); margin-right:12px; font-variant-numeric:tabular-nums; }
  /* block flow (not flex) so a <table> child resolves width:100% to full width */
  section .viz { margin-top:0; display:block; }
  section .viz > * + * { margin-top:18px; }
  section .viz .vstats .st .n { font-size:28px; }
  /* Marp's default theme styles raw section table/ul/code at ID specificity;
     !important reclaims our clean chart styling regardless of that. */
  section .vtable { display:table !important; width:100% !important; overflow:visible !important; table-layout:fixed; border-collapse:collapse; }
  section .vtable tr { background:transparent !important; border:0 !important; }
  section .vtable th, section .vtable td { border:0 !important; border-bottom:1px solid var(--line) !important; padding:9px 0 !important; }
  section .vtable th { padding-bottom:11px !important; }
  section .vtable th + th, section .vtable td + td { padding-left:18px !important; }
  section .vtable td.r, section .vtable th.r { text-align:right !important; white-space:nowrap; }
  section .vtable tr:last-child td { border-bottom:0 !important; }
  section .vgroup ul, section .vgroup li { margin:0 !important; }
  section .vgroup ul { padding-left:16px !important; }
  section .vstep code { background:none !important; border:0 !important; padding:0 !important; }
  section h2 { border:0 !important; }
  /* lead / title */
  section.lead { background:linear-gradient(150deg, var(--grad1), var(--grad2)); color:#fff; justify-content:center; }
  section.lead .eyebrow { font:600 14px/1 "JetBrains Mono",monospace; letter-spacing:.24em; text-transform:uppercase; color:rgba(255,255,255,.82); margin-bottom:22px; }
  section.lead .hero { font-size:64px; line-height:1.04; color:#fff; margin:0 0 24px; max-width:18ch; }
  section.lead .lead-sub { font-size:22px; line-height:1.5; color:rgba(255,255,255,.94); max-width:58ch; margin:0; }
  section.lead .lead-foot { margin-top:32px; font:600 13px/1 "JetBrains Mono",monospace; letter-spacing:.12em; color:rgba(255,255,255,.78); }
  section.lead::after { color:rgba(255,255,255,.6); }
  /* contents — all 20 on one slide */
  section.contents { padding:32px 56px; }
  section.contents h2 { font-size:27px; margin:0 0 3px; }
  section.contents .lede { color:var(--dim); font-size:13px; margin:0 0 14px; }
  .agenda { columns:2; column-gap:44px; }
  .ag-sec { break-inside:avoid; margin:0 0 12px; }
  .ag-h { display:flex; align-items:baseline; gap:8px; margin:0 0 5px; padding-bottom:4px; border-bottom:1px solid var(--line); }
  .ag-h .ix { font:700 11.5px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .ag-h .t { font:700 12px/1 var(--fsans); text-transform:uppercase; letter-spacing:.05em; color:var(--ink2); }
  .ag-item { display:flex; gap:10px; align-items:baseline; padding:2.5px 0; font-size:12.5px; color:var(--ink2); line-height:1.32; text-decoration:none; break-inside:avoid; }
  .ag-item:hover { color:var(--ac); }
  .ag-item .n { flex:none; width:20px; font:700 11.5px/1.32 "JetBrains Mono",monospace; color:var(--ac); font-variant-numeric:tabular-nums; }
  .lede strong { color:var(--ink); font-weight:700; }
  /* skills grid (closing slide) */
  .skills { display:grid; grid-template-columns:repeat(3,1fr); gap:14px; margin-top:8px; }
  .skill { background:var(--panel); border:1px solid var(--line); border-left:3px solid var(--ac); border-radius:var(--r); padding:14px 16px; box-shadow:var(--shadow); }
  .skill .cmd { display:block; font:700 18px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .skill .sd { display:block; margin-top:8px; font-size:13px; line-height:1.4; color:var(--ink2); }
"""

_DECK_FM = ("---\n"
            "marp: true\n"
            "theme: default\n"
            "paginate: true\n"
            "size: 16:9\n"
            "---\n")

# The same prompts ship as slash commands in the `openstory-skills` Claude Code
# plugin. (cmd, section-colour, one-line gist) — gists condensed from each
# skill's SKILL.md `description`; colour ties it to the matching deck section.
SKILLS = [
    ("/cost", "blue", "total spend, cache savings, tokens-per-day"),
    ("/time", "blue", "where time goes — by project, by hour"),
    ("/tools", "blue", "top tools & commands; read-vs-write ratio"),
    ("/scan", "blue", "find secrets before you share your history"),
    ("/team", "cyan", "who's active now & what each teammate's on"),
    ("/arc", "purple", "a project's story as a deterministic timeline"),
    ("/standup", "purple", "today's work → did · blocked · next"),
    ("/coach", "green", "honest feedback on your prompting & patterns"),
    ("/recall", "orange", "find how you solved it before — exact commands"),
    ("/recap", "orange", "what you shipped lately, grouped by project"),
    ("/prime", "red", "pick up where the last session left off"),
    ("/watch", "red", "a live feed of a branch as it streams"),
]


def render_marp(theme: str = "georgetown") -> str:
    """A Marp (https://marp.app) slide deck: every prompt on one contents slide,
    then one click-through slide per prompt with its real chart.

    Charts are the same HTML/CSS as the web page, so render with HTML enabled:
        marp deck.md --html -o deck.pdf      # or .html / .pptx
    """
    if theme not in THEMES:
        raise ValueError(f"unknown theme {theme!r}; choose from {list(THEMES)}")
    t = THEMES[theme]
    style = (f"<style>\n@import url('{t['link']}');\n{root_css(theme)}\n"
             f"{_chart_css()}\n{_DECK_CSS}</style>")

    slides = []
    # title
    slides.append(
        "<!-- _class: lead -->\n<!-- _paginate: false -->\n\n"
        '<div class="eyebrow">OpenStory · draft</div>\n'
        '<div class="hero">Empower yourself with your own history</div>\n'
        f'<div class="lead-sub">{_e(INTRO)}</div>\n'
        '<div class="lead-foot">20 prompts · 6 ways to point it at your own work</div>'
    )

    # contents — all 20 prompts, one slide, two columns, grouped by section.
    # Each item links to its prompt slide (title=1, contents=2, prompt n = n+2).
    blocks = []
    n = 0
    for s in SECTIONS:
        items = []
        for p in s["prompts"]:
            n += 1
            items.append(f'<a class="ag-item" href="#{n + 2}"><span class="n">{n:02d}</span>'
                         f'<span>{_e(p["text"])}</span></a>')
        blocks.append(f'<div class="ag-sec {s["color"]}"><div class="ag-h">'
                      f'<span class="ix">{s["ix"]}</span><span class="t">{_e(s["title"])}</span></div>'
                      + "".join(items) + "</div>")
    slides.append(
        "<!-- _class: contents -->\n\n## Contents — 20 prompts\n\n"
        '<p class="lede">Paste any of these to your coding agent — each runs against your own OpenStory and opens a real report.</p>\n'
        f'<div class="agenda">{"".join(blocks)}</div>'
    )

    # one slide per prompt — breadcrumb, headline, the real chart
    n = 0
    for s in SECTIONS:
        for i, p in enumerate(s["prompts"], 1):
            n += 1
            cls = "real" if p["kind"] == "real" else "illus"
            tag = "real · live data" if p["kind"] == "real" else "illustrative"
            viz = "".join(_block(b) for b in p["viz"])
            slides.append(
                f'<!-- _class: {s["color"]} -->\n\n'
                f'<div class="crumb"><span class="ix">{s["ix"]}.{i}</span>'
                f'<span>{_e(s["title"])}</span><span class="tag {cls}">{tag}</span></div>\n'
                f'<div class="q"><span class="qn">{n:02d}</span>{_e(p["text"])}</div>\n'
                f'<div class="viz">{viz}<div class="src">{_e(p["source"])}</div></div>'
            )

    # closing slide — the same prompts as one-keystroke slash commands
    cards = "".join(
        f'<div class="skill {color}"><span class="cmd">{_e(cmd)}</span>'
        f'<span class="sd">{_e(desc)}</span></div>'
        for cmd, color, desc in SKILLS
    )
    slides.append(
        "<!-- _class: contents -->\n\n## Or skip the prompt — just type the command\n\n"
        '<p class="lede">The same reports ship as <strong>openstory-skills</strong>, a Claude Code plugin. '
        'Twelve slash commands, backed by the OpenStory MCP server.</p>\n'
        f'<div class="skills">{cards}</div>'
    )

    return _DECK_FM + style + "\n\n" + "\n\n---\n\n".join(slides) + "\n"


# -- CSS / shell ------------------------------------------------------

_CSS = """
  * { box-sizing:border-box; }
  body { margin:0; background:radial-gradient(1100px 520px at 78% -8%, var(--glow), transparent), var(--bg);
    color:var(--ink); font-family:var(--fsans); -webkit-font-smoothing:antialiased; }
  .wrap { max-width:920px; margin:0 auto; padding:56px 24px 90px; }
  code, .mono { font-family:"JetBrains Mono",ui-monospace,Menlo,monospace; }
  .eyebrow { font:600 12px/1 "JetBrains Mono",monospace; letter-spacing:.18em; text-transform:uppercase; color:var(--blue); margin:0 0 14px; }
  header h1 { font-family:var(--fdisplay); font-size:43px; line-height:1.06; letter-spacing:-.02em; margin:0 0 12px; font-weight:700; }
  header h1 .grad { background:linear-gradient(90deg,var(--grad1),var(--grad2)); -webkit-background-clip:text; background-clip:text; color:transparent; }
  header .sub { color:var(--ink2); font-size:17px; line-height:1.5; max-width:66ch; margin:0; }
  .howto { margin:22px 0 0; padding:14px 18px; border:1px solid var(--line); border-left:3px solid var(--cyan); border-radius:12px; background:var(--bg2); color:var(--dim); font-size:13.5px; line-height:1.55; }
  .howto code { color:var(--ink2); }
  section { margin-top:46px; }
  .sechead { display:flex; align-items:baseline; gap:12px; margin-bottom:4px; }
  .sechead .ix { font:700 14px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .sechead h2 { font-size:22px; font-weight:700; letter-spacing:-.01em; margin:0; }
  .why { color:var(--dim); font-size:14px; margin:0 0 18px; max-width:70ch; }
  /* per-section accent — every chart derives its colour from this */
  .blue{--ac:var(--blue)} .cyan{--ac:var(--cyan)} .purple{--ac:var(--purple)}
  .green{--ac:var(--green)} .orange{--ac:var(--orange)} .red{--ac:var(--red)}
  .prompts { display:grid; gap:12px; }
  .prompt { position:relative; background:var(--panel); border:1px solid var(--line); border-left:3px solid var(--ac); border-radius:var(--r); padding:16px 56px 16px 18px; box-shadow:var(--shadow); }
  .prompt > p { margin:0; font-size:15px; line-height:1.5; color:var(--ink); }
  .prompt .hint { display:block; margin-top:7px; color:var(--dim); font:500 12px/1.4 "JetBrains Mono",monospace; }
  .copy { position:absolute; top:12px; right:12px; width:34px; height:34px; border-radius:8px; cursor:pointer;
    background:var(--panel2); border:1px solid var(--line); color:var(--dim); font-size:14px; display:grid; place-items:center; transition:all .12s ease; }
  .copy:hover { color:var(--ink); border-color:var(--ac); }
  .copy.done { color:var(--green); border-color:var(--green); }
  /* report disclosure */
  details.ex { margin-top:13px; border-top:1px solid var(--line); padding-top:12px; }
  details.ex > summary { cursor:pointer; list-style:none; color:var(--ac); font:600 11.5px/1 "JetBrains Mono",monospace; letter-spacing:.06em; display:inline-flex; align-items:center; gap:8px; }
  details.ex > summary::-webkit-details-marker { display:none; }
  details.ex > summary::before { content:"\\25B8"; transition:transform .15s ease; }
  details.ex[open] > summary::before { transform:rotate(90deg); }
  .extag { font:600 9.5px/1 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.1em; padding:3px 7px; border-radius:5px; border:1px solid var(--line); color:var(--dim); }
  .extag.real { color:var(--green); border-color:color-mix(in srgb, var(--green) 45%, var(--line)); }
  .extag.illus { color:var(--orange); border-color:color-mix(in srgb, var(--orange) 45%, var(--line)); }
  /* report canvas */
  .viz { margin-top:14px; padding:20px; background:var(--bg2); border:1px solid var(--line); border-radius:var(--r); display:flex; flex-direction:column; gap:20px; }
  .viz .src { color:var(--dim); font:500 11px/1.4 "JetBrains Mono",monospace; border-top:1px solid var(--line); padding-top:11px; }
  .vtitle { font:600 10.5px/1 "JetBrains Mono",monospace; letter-spacing:.14em; text-transform:uppercase; color:var(--dim); margin-bottom:12px; }
  /* stats */
  .vstats { display:flex; flex-wrap:wrap; gap:10px; }
  .vstats .st { flex:1; min-width:96px; background:var(--panel); border:1px solid var(--line); border-radius:10px; padding:13px 15px; box-shadow:var(--shadow); }
  .vstats .st .n { font:700 25px/1 Inter,sans-serif; letter-spacing:-.02em; font-variant-numeric:tabular-nums; }
  .vstats .st.ac .n { color:var(--ac); }
  .vstats .st .l { margin-top:6px; color:var(--dim); font:500 11px/1.3 "JetBrains Mono",monospace; }
  /* bars */
  .vbars { display:flex; flex-direction:column; gap:10px; }
  .vbar { display:grid; grid-template-columns:128px 1fr 46px; gap:14px; align-items:center; }
  .vbar .bl { font-size:12.5px; color:var(--ink2); }
  .vbar .bt { display:block; height:22px; background:color-mix(in srgb, var(--ink) 6%, transparent); border-radius:6px; overflow:hidden; }
  .vbar .bf { display:block; height:100%; background:var(--ac); border-radius:6px; min-width:4px; }
  .vbar .bv { font:600 12.5px/1 "JetBrains Mono",monospace; color:var(--ink); font-variant-numeric:tabular-nums; text-align:right; }
  /* table */
  .vtable { width:100%; border-collapse:collapse; font-size:12.5px; }
  .vtable th { text-align:left; font:600 9.5px/1 "JetBrains Mono",monospace; letter-spacing:.1em; text-transform:uppercase; color:var(--dim); padding:0 0 10px 0; border-bottom:1px solid var(--line); }
  .vtable td { padding:10px 0; border-bottom:1px solid var(--line); color:var(--ink2); vertical-align:middle; }
  .vtable th + th, .vtable td + td { padding-left:20px; }   /* guarantees columns never collide */
  .vtable tr:last-child td { border-bottom:none; }
  .vtable td.r, .vtable th.r { text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }
  .chip-sev { font:600 10px/1 "JetBrains Mono",monospace; padding:3px 8px; border-radius:5px; border:1px solid; }
  .chip-sev.hi { color:var(--red); border-color:color-mix(in srgb, var(--red) 45%, var(--line)); background:color-mix(in srgb, var(--red) 12%, transparent); }
  .chip-sev.med { color:var(--orange); border-color:color-mix(in srgb, var(--orange) 45%, var(--line)); background:color-mix(in srgb, var(--orange) 10%, transparent); }
  .chip-sev.lo { color:var(--dim); border-color:var(--line); }
  .dot { margin-right:8px; font-size:11px; } .dot.on { color:var(--green); } .dot.off { color:var(--dim); }
  .cellbar { display:inline-block; width:64px; height:8px; background:color-mix(in srgb, var(--ink) 9%, transparent); border-radius:3px; overflow:hidden; vertical-align:middle; margin-right:9px; }
  .cellbar > span { display:block; height:100%; background:var(--ac); }
  /* timeline */
  .vtl { position:relative; padding-left:24px; }
  .vtl::before { content:""; position:absolute; left:4px; top:6px; bottom:6px; width:2px; background:var(--line); }
  .vtl .ev { position:relative; padding:0 0 16px; }
  .vtl .ev:last-child { padding-bottom:0; }
  .vtl .ev::before { content:""; position:absolute; left:-24px; top:3px; width:9px; height:9px; border-radius:50%; background:var(--bg2); border:2px solid var(--ac); }
  .vtl .d { font:600 11px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .vtl .t { font-size:13.5px; color:var(--ink); margin-top:4px; font-weight:600; }
  .vtl .nt { font-size:12.5px; color:var(--dim); margin-top:2px; line-height:1.45; }
  .tlarc { margin-top:6px; padding-left:13px; border-left:2px solid var(--ac); color:var(--ink2); font-size:13px; font-style:italic; line-height:1.5; }
  /* cards */
  .vcards { display:flex; flex-direction:column; gap:10px; }
  .vcard { background:var(--panel); border:1px solid var(--line); border-left:2px solid var(--ac); border-radius:0 10px 10px 0; padding:13px 16px; box-shadow:var(--shadow); }
  .vcard .ch { font-weight:600; font-size:13.5px; color:var(--ink); }
  .vcard .cs { font:500 11px/1 "JetBrains Mono",monospace; color:var(--dim); margin-left:8px; }
  .vcard .ct { margin-top:6px; color:var(--dim); font-size:12.5px; line-height:1.5; }
  /* groups */
  .vgroups { display:grid; grid-template-columns:repeat(3,1fr); gap:16px; }
  .vgroup .gh { font:600 10px/1 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.1em; color:var(--dim); margin-bottom:9px; padding-bottom:7px; border-bottom:1px solid var(--line); }
  .vgroup.warn .gh { color:var(--orange); }
  .vgroup ul { margin:0; padding-left:16px; color:var(--ink2); font-size:12.5px; line-height:1.5; }
  .vgroup li { margin-bottom:5px; }
  /* meta */
  .vmeta { display:grid; grid-template-columns:auto 1fr; gap:9px 18px; font-size:13px; margin:0; }
  .vmeta dt { color:var(--dim); font:600 10px/1.6 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.06em; }
  .vmeta dd { margin:0; color:var(--ink2); line-height:1.45; }
  /* steps + code */
  .vsteps { counter-reset:s; display:flex; flex-direction:column; gap:9px; }
  .vstep { display:grid; grid-template-columns:23px 1fr; gap:12px; align-items:center; }
  .vstep::before { counter-increment:s; content:counter(s); width:23px; height:23px; border-radius:50%; background:var(--panel); border:1px solid var(--ac); color:var(--ac); font:600 11px/23px "JetBrains Mono",monospace; text-align:center; }
  .vstep code { font-family:"JetBrains Mono",monospace; font-size:12px; color:var(--ink2); }
  .vcode { background:var(--panel); border:1px solid var(--line); border-left:2px solid var(--ac); border-radius:0 8px 8px 0; padding:13px 16px; white-space:pre; overflow-x:auto; font-family:"JetBrains Mono",monospace; font-size:12px; line-height:1.7; color:var(--ink2); }
  /* list */
  .vlist { display:flex; flex-direction:column; }
  .vlist .li { display:grid; grid-template-columns:60px 1fr auto; gap:14px; padding:8px 0; border-bottom:1px solid var(--line); font-size:12.5px; align-items:center; color:var(--ink2); }
  .vlist .li:last-child { border-bottom:none; }
  .vlist .li .ld { font:600 11px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .vlist .li .lm { font:600 11px/1 "JetBrains Mono",monospace; color:var(--dim); }
  /* feed */
  .vfeed { display:flex; flex-direction:column; }
  .vfeed .fr { display:grid; grid-template-columns:54px 92px 1fr; gap:12px; padding:7px 0; border-bottom:1px solid var(--line); font-size:12px; align-items:baseline; }
  .vfeed .fr:last-child { border-bottom:none; }
  .vfeed .ft { font:600 11px/1.4 "JetBrains Mono",monospace; color:var(--dim); }
  .vfeed .fk { font:600 11px/1.4 "JetBrains Mono",monospace; color:var(--ac); }
  .vfeed .fx { color:var(--ink2); }
  /* chips + note */
  .vchips { display:flex; flex-wrap:wrap; gap:7px; align-items:center; }
  .vchips .cl { font:600 10px/1 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.1em; color:var(--dim); margin-right:4px; }
  .vchip { background:var(--panel); border:1px solid var(--line); border-radius:6px; padding:5px 10px; font:500 11.5px/1 "JetBrains Mono",monospace; color:var(--ink2); }
  .vnote { border-left:2px solid var(--ac); padding:3px 0 3px 14px; color:var(--ink2); font-size:13px; line-height:1.55; }
  .vnote.warn { border-color:var(--orange); }
  .vnote b { color:var(--ink); font-weight:600; }
  footer { margin-top:54px; color:var(--dim); font:500 12px/1.6 "JetBrains Mono",monospace; text-align:center; }
  .draftnote { margin:30px 0 0; padding:12px 16px; border:1px dashed var(--line); border-radius:10px; color:var(--dim); font-size:12.5px; background:var(--bg2); }
  @media (max-width:680px){ header h1{font-size:30px} .prompt{padding-right:18px} .copy{position:static; margin-bottom:10px}
    .vgroups{grid-template-columns:1fr} .vbar{grid-template-columns:104px 1fr 42px} .viz{padding:15px} }
  /* contents / table of contents */
  .toc { margin:40px 0 0; padding:26px 28px; border:1px solid var(--line); border-radius:var(--r); background:var(--bg2); box-shadow:var(--shadow); }
  .toc > h2 { font-family:var(--fdisplay); font-size:20px; font-weight:700; margin:0 0 18px; letter-spacing:-.01em; }
  .tocsec { margin-top:18px; }
  .tocsec:first-of-type { margin-top:0; }
  .tocsec .toch { display:flex; align-items:baseline; gap:10px; margin-bottom:8px; padding-bottom:7px; border-bottom:1px solid var(--line); }
  .tocsec .toch .ix { font:700 12px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .tocsec .toch h3 { font-size:13px; font-weight:700; letter-spacing:.02em; text-transform:uppercase; margin:0; color:var(--ink2); }
  .toclist { list-style:none; margin:0; padding:0; }
  .toclist li { margin:0; }
  .toclist a { display:flex; gap:12px; align-items:baseline; padding:5px 0; text-decoration:none; color:var(--ink2); font-size:13.5px; line-height:1.4; border-bottom:1px dotted color-mix(in srgb, var(--line) 70%, transparent); }
  .toclist li:last-child a { border-bottom:none; }
  .toclist a:hover .tt { color:var(--ac); }
  .toclist .tn { flex:none; width:24px; font:600 11px/1.4 "JetBrains Mono",monospace; color:var(--ac); font-variant-numeric:tabular-nums; }
  /* prompt number badge */
  .prompt .pnum { position:absolute; top:14px; left:-1px; transform:translateX(-50%); width:26px; height:22px;
    background:var(--ac); color:#fff; border-radius:6px; font:700 11px/22px "JetBrains Mono",monospace; text-align:center; font-variant-numeric:tabular-nums; box-shadow:var(--shadow); }
  /* print → PDF: open every report, no copy buttons, sane page breaks */
  @page { size:A4; margin:14mm 12mm; }
  @media print {
    body { background:var(--bg); }
    .wrap { max-width:none; padding:0; }
    .copy, .draftnote { display:none !important; }
    /* Chrome's print renderer turns large-blur shadows into hard grey blocks — drop them all */
    * { box-shadow:none !important; }
    .toc { break-after:page; page-break-after:always; }
    .sechead { break-after:avoid; }            /* keep a heading with its first prompt */
    .prompt { break-inside:avoid; }
    details.ex { display:block; }              /* force every report open in print */
    details.ex > summary { list-style:none; }
    a { color:inherit; text-decoration:none; }
  }
"""

_COPY_JS = """
    document.querySelectorAll('.prompt').forEach(function (card) {
      var text = card.querySelector('p').textContent.trim();
      var btn = document.createElement('button');
      btn.className = 'copy'; btn.type = 'button'; btn.title = 'Copy prompt'; btn.textContent = '\\u29C9';
      btn.addEventListener('click', function () {
        navigator.clipboard.writeText(text).then(function () {
          btn.textContent = '\\u2713'; btn.classList.add('done');
          setTimeout(function () { btn.textContent = '\\u29C9'; btn.classList.remove('done'); }, 1200);
        });
      });
      card.appendChild(btn);
    });
"""

_PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Empower yourself — an OpenStory prompt library</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="{fontlink}" rel="stylesheet">
<style>{css}</style></head>
<body><div class="wrap">
  <header>
    <p class="eyebrow">OpenStory · draft</p>
    <h1>Empower yourself with <span class="grad">your own history</span></h1>
    <p class="sub">{intro}</p>
    <div class="howto"><strong>How to use:</strong> paste any of these to your coding agent. They run against <em>your</em> OpenStory instance — via the MCP server or the REST API at <code>localhost:3002</code>. Swap anything in <code>&lt;angle brackets&gt;</code> for your specifics. Open <em>the report</em> under any prompt to see what it returns.</div>
  </header>
{toc}
{body}
  <div class="draftnote">Draft · incubating for openstory.work — not yet live. Reports charted from a real OpenStory store and anonymized (people → roles, projects → a-project, no secrets). <span class="mono">real</span> = queried live; <span class="mono">illustrative</span> = representative.</div>
  <footer>OpenStory · a mirror, not a leash.</footer>
  <script>{js}</script>
</div></body></html>"""


# -- PDF --------------------------------------------------------------

def _find_chrome() -> str | None:
    """Locate a headless-capable Chromium binary across platforms."""
    for name in ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome"):
        if found := shutil.which(name):
            return found
    for path in (
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "C:/Program Files/Google/Chrome/Application/chrome.exe",
    ):
        if Path(path).exists():
            return path
    return None


def chrome_pdf(html_text: str, out_path: str, deadline: float = 60.0) -> None:
    """Render an HTML string to PDF via headless Chrome. Generic — reused by other
    generators (e.g. build_skill_report.py).

    Chrome writes the PDF and then, on macOS with the user's own Chrome already
    running, often fails to self-exit. So we don't wait for a clean exit: we poll
    for the output file to appear and stop growing, then terminate the process.
    """
    chrome = _find_chrome()
    if not chrome:
        raise RuntimeError("no Chrome/Chromium found — install one, or print to PDF from a browser")
    out = Path(out_path).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    if out.exists():
        out.unlink()
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "page.html"
        src.write_text(html_text, encoding="utf-8")
        # Classic --headless (not =new, which hangs here). Dedicated --user-data-dir
        # so it never collides with the user's already-open Chrome. No
        # --virtual-time-budget: with the page's CSS transitions it never advances.
        proc = subprocess.Popen(
            [chrome, "--headless", "--disable-gpu", "--no-sandbox",
             f"--user-data-dir={Path(tmp) / 'chrome'}",
             "--no-pdf-header-footer",
             f"--print-to-pdf={out}", src.resolve().as_uri()],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            start, last_size, stable = time.monotonic(), -1, 0
            while time.monotonic() - start < deadline:
                if proc.poll() is not None:    # Chrome exited on its own
                    break
                size = out.stat().st_size if out.exists() else 0
                stable = stable + 1 if size > 0 and size == last_size else 0
                last_size = size
                if stable >= 2:                # file present and unchanged twice → done
                    break
                time.sleep(0.5)
        finally:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
    if not out.exists() or out.stat().st_size < 1024:
        raise RuntimeError(f"Chrome did not produce a valid PDF at {out}")


def render_pdf(pdf_path: str, theme: str = "aistack", deadline: float = 60.0) -> None:
    """Render the expanded (every-report-open) prompt-library page to PDF."""
    chrome_pdf(render_html(theme, expand=True), pdf_path, deadline)
    print(f"Wrote {pdf_path} [{theme}]")


# -- Tests ------------------------------------------------------------

def run_tests() -> None:
    print("Running tests...")
    total = sum(len(s["prompts"]) for s in SECTIONS)
    assert total == 20 and len(SECTIONS) == 6, (total, len(SECTIONS))
    print(f"  OK: {len(SECTIONS)} sections, {total} prompts")

    # every block type renders without error
    for s in SECTIONS:
        for p in s["prompts"]:
            for b in p["viz"]:
                assert _block(b), b
    print("  OK: every viz block renders")

    h, m = render_html(), render_md()
    assert h.startswith("<!doctype html>") and h.rstrip().endswith("</html>")
    assert h.count('class="prompt"') == 20 and h.count('details class="ex"') == 20
    for klass in ("vstats", "vbars", "vtable", "vtl", "vcards", "vgroups", "vmeta", "vsteps", "vcode", "vlist", "vfeed", "vchips", "vnote", "chip-sev", "dot on"):
        assert klass in h, f"missing chart class: {klass}"
    print("  OK: HTML renders with all chart primitives")

    # TOC: one entry per prompt, each linking to an existing anchor on the page
    assert h.count('<nav class="toc"') == 1, "exactly one TOC"
    assert h.count('class="tocsec') == len(SECTIONS), "one TOC group per section"
    for i in range(1, total + 1):
        assert f'href="#p{i}"' in h, f"TOC link #p{i} missing"
        assert f'id="p{i}"' in h, f"prompt anchor p{i} missing"
    assert h.index('<nav class="toc"') < h.index('id="p1"'), "TOC must come before the prompt list"
    assert '<details class="ex" open>' not in h, "default page keeps reports collapsed"
    he = render_html(expand=True)
    assert he.count('<details class="ex" open>') == total, "expanded variant opens every report"
    print(f"  OK: TOC links all {total} prompts before the list · expand opens every report")

    # escaping: literal angle brackets in content must be entity-encoded
    body = h.split("</style>")[1]
    assert "&lt;id&gt;" in h and "<id>" not in body, "placeholders must be escaped"
    assert "&lt;80-char" in h, "literal '<' in note text must be escaped"
    assert "<b>" in h, "note bold (**...**) should become <b>"
    print("  OK: content escaped; note markup intact")

    leaks = leakage(h) + leakage(m)
    assert not leaks, f"LEAKAGE: {leaks}"
    kinds = [p["kind"] for s in SECTIONS for p in s["prompts"]]
    print(f"  OK: no leakage · {kinds.count('real')} real · {kinds.count('illustrative')} illustrative")

    for th in THEMES:
        hh = render_html(th)
        assert hh.count('class="prompt"') == 20
        assert f"--bg:{THEMES[th]['bg']};" in hh, f"theme tokens missing for {th}"
        assert not leakage(hh)
    print(f"  OK: {len(THEMES)} themes render cleanly ({', '.join(THEMES)})")

    # Marp deck: front matter, ONE contents slide + one slide per prompt
    deck = render_marp()
    assert deck.startswith("---\nmarp: true\n"), "Marp front matter required"
    n_slides = deck.count("\n---\n\n") + 1
    assert n_slides == 23, f"expected 23 slides (title + contents + 20 + skills), got {n_slides}"
    assert deck.count('class="agenda"') == 1 and deck.count('class="ag-item"') == 20, "20 agenda items on one slide"
    # closing skills slide carries every slash command
    assert deck.count('class="skills"') == 1 and deck.count('class="skill ') == len(SKILLS), "one card per skill"
    for cmd, _, _ in SKILLS:
        assert f'class="cmd">{cmd}<' in deck, f"skill command missing from deck: {cmd}"
    # contents items link to their prompt slides (title=1, contents=2, prompt n = n+2)
    assert deck.count('class="ag-item" href="#3"') == 1 and f'href="#{20 + 2}"' in deck, "agenda items link to slides"
    # one real HTML chart canvas per prompt slide (same charts as the web page)
    assert deck.count('<div class="viz">') == 20, "every prompt slide carries a chart"
    assert deck.count('<div class="q">') == 20, "every prompt slide has a headline"
    for klass in ("vstats", "vbars", "vtable", "vtl", "vgroups", "vsteps", "vchips"):
        assert klass in deck, f"deck missing chart primitive: {klass}"
    # the shared chart CSS was sliced in (page + deck, one source of truth)
    cc = _chart_css()
    assert ".viz {" in cc and ".vbar " in cc and ".blue{--ac" in cc, "chart css slice incomplete"
    # full prompt text present; angle-bracket placeholders are HTML-escaped
    for s in SECTIONS:
        for p in s["prompts"]:
            assert _e(p["text"]) in deck, f"prompt missing from deck: {p['text'][:40]}"
    assert "<topic>" not in deck and "&lt;topic&gt;" in deck, "placeholders must be escaped"
    assert not leakage(deck), f"LEAKAGE in deck: {leakage(deck)}"
    print(f"  OK: Marp deck — {n_slides} slides (1 contents + 20 charts), shared chart CSS, no leakage")
    print("\nAll tests passed.")


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--html", metavar="PATH")
    ap.add_argument("--md", metavar="PATH")
    ap.add_argument("--pdf", metavar="PATH", help="print-ready PDF (TOC + every report expanded; needs Chrome)")
    ap.add_argument("--marp", metavar="PATH", help="Marp slide deck (markdown): contents agenda + one slide per prompt")
    ap.add_argument("--theme", default="aistack", choices=list(THEMES), help="palette for --html/--pdf/--marp (default: midnight)")
    ap.add_argument("--expand", action="store_true", help="emit every report expanded (implied by --pdf)")
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()
    if args.test:
        run_tests(); sys.exit(0)
    if not (args.html or args.md or args.pdf or args.marp):
        ap.error("pass --html PATH, --md PATH, --pdf PATH, and/or --marp PATH (or --test)")
    if args.html:
        Path(args.html).write_text(render_html(args.theme, expand=args.expand), encoding="utf-8")
        print(f"Wrote {args.html} [{args.theme}]")
    if args.md:
        Path(args.md).write_text(render_md(), encoding="utf-8"); print(f"Wrote {args.md}")
    if args.pdf:
        render_pdf(args.pdf, args.theme)
    if args.marp:
        Path(args.marp).write_text(render_marp(args.theme), encoding="utf-8")
        print(f"Wrote {args.marp} [{args.theme}] — render: marp {args.marp} --html -o deck.pdf")
