#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Harness-based attribution for pi-mono sessions — measure Bobby's signature.

Path-based attribution (the rule used by all shape-layer build scripts under
`scripts/build_*_shapes.py`) excludes pi-mono sessions because they have no
`/Users/X/` filesystem paths. This script reads the CloudEvent JSONL backups
directly under `data/` and produces an analogous signature aggregation,
filtering by `agent: "pi-mono"` on the CloudEvent envelope.

Output: prints a structured fact sheet to stdout, and writes a JSON summary
to personal/reports/data/bobby-signature.json.
"""

from __future__ import annotations
import json
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

DATA_DIR = Path("data")
OUT_FILE = Path("personal/reports/data/bobby-signature.json")
OUT_FILE.parent.mkdir(parents=True, exist_ok=True)


def parse_iso(ts: str) -> datetime | None:
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except Exception:
        return None


def analyze() -> dict:
    sessions: dict[str, dict] = {}
    record_types = Counter()
    models_over_time: list[tuple[str, str]] = []  # (ts, model)
    tool_counter: Counter = Counter()
    prompts: list[dict] = []
    hourly: Counter = Counter()
    weekly: Counter = Counter()
    tokens = {"input": 0, "output": 0, "cache_creation": 0, "cache_read": 0}
    text_words: Counter = Counter()
    file_count = 0
    pi_file_count = 0

    for f in sorted(DATA_DIR.glob("*.jsonl")):
        file_count += 1
        try:
            with f.open() as fh:
                first = fh.readline()
                if not first.strip():
                    continue
                ev0 = json.loads(first)
                if ev0.get("agent") != "pi-mono":
                    continue
                pi_file_count += 1
                sid = ev0.get("data", {}).get("session_id") or f.stem
                fh.seek(0)
                lines = fh.readlines()
        except Exception as e:
            print(f"  warn: {f.name}: {e}", file=sys.stderr)
            continue

        sess: dict = {
            "id": sid,
            "events": 0,
            "first_ts": None,
            "last_ts": None,
            "tools": Counter(),
            "models": Counter(),
            "first_prompt": None,
        }

        for ln in lines:
            try:
                e = json.loads(ln)
            except Exception:
                continue
            ts = e.get("time")
            subtype = e.get("subtype", "")
            payload = e.get("data", {}).get("agent_payload", {}) or {}
            sess["events"] += 1
            record_types[subtype] += 1
            if sess["first_ts"] is None:
                sess["first_ts"] = ts
            sess["last_ts"] = ts

            dt = parse_iso(ts) if ts else None
            if dt:
                hourly[dt.hour] += 1
                weekly[f"W{dt.isocalendar()[1]:02d}"] += 1

            model = payload.get("model")
            if model:
                sess["models"][model] += 1
                if ts:
                    models_over_time.append((ts, model))

            if subtype == "message.user.prompt":
                text = payload.get("text") or ""
                if sess["first_prompt"] is None and text:
                    sess["first_prompt"] = text[:200]
                if text:
                    prompts.append({"ts": ts, "session": sid, "text": text[:300]})
                    for w in text.lower().split():
                        w = w.strip(".,!?;:\"'()[]{}/<>")
                        if 4 <= len(w) <= 30:
                            text_words[w] += 1

            tool = payload.get("tool")
            if tool and subtype.endswith("tool_use"):
                tool_counter[tool] += 1
                sess["tools"][tool] += 1

            tu = payload.get("token_usage") or {}
            if isinstance(tu, dict):
                # pi-mono uses native key names: input, output, totalTokens,
                # cacheRead, cacheWrite — see rs/core/src/translate_pi.rs:750
                tokens["input"] += tu.get("input") or tu.get("input_tokens") or 0
                tokens["output"] += tu.get("output") or tu.get("output_tokens") or 0
                tokens["cache_creation"] += (
                    tu.get("cacheWrite") or tu.get("cache_creation_input_tokens") or 0
                )
                tokens["cache_read"] += (
                    tu.get("cacheRead") or tu.get("cache_read_input_tokens") or 0
                )

        sessions[sid] = sess

    total_input = tokens["input"] + tokens["cache_creation"] + tokens["cache_read"]
    cache_hit = (tokens["cache_read"] / total_input * 100) if total_input else 0

    summary = {
        "files_scanned": file_count,
        "pi_mono_files": pi_file_count,
        "sessions": len(sessions),
        "total_events": sum(s["events"] for s in sessions.values()),
        "record_types": dict(record_types.most_common(15)),
        "tools_top": dict(tool_counter.most_common(15)),
        "tokens": tokens,
        "cache_hit_rate_pct": round(cache_hit, 1),
        "total_input_real": total_input,
        "hourly_distribution_utc": dict(sorted(hourly.items())),
        "weekly_event_volume": dict(sorted(weekly.items())),
        "top_words_in_prompts": dict(text_words.most_common(30)),
        "models_over_time_sample": models_over_time[::max(1, len(models_over_time)//20)][:20],
        "sessions_summary": [
            {
                "id": s["id"],
                "events": s["events"],
                "first_ts": s["first_ts"][:19] if s["first_ts"] else None,
                "last_ts": s["last_ts"][:19] if s["last_ts"] else None,
                "tools": dict(s["tools"].most_common(10)),
                "models": dict(s["models"]),
                "first_prompt": s["first_prompt"],
            }
            for s in sorted(sessions.values(), key=lambda x: -x["events"])
        ],
    }
    return summary


def print_facts(s: dict):
    print(f"# Bobby — pi-mono signature  (harness-based attribution)\n")
    print(f"corpus: {s['files_scanned']} session JSONL files in data/")
    print(f"pi-mono sessions: {s['pi_mono_files']}")
    print(f"total events:     {s['total_events']:,}")
    print(f"\n## record types")
    for k, v in s["record_types"].items():
        print(f"  {k:<30} {v}")
    print(f"\n## top tools used")
    for t, n in s["tools_top"].items():
        print(f"  {t:<28} {n}")
    print(f"\n## model usage")
    model_counts: Counter = Counter()
    for _, m in s["models_over_time_sample"]:
        model_counts[m] += 1
    print(f"  (sampled across the timeline)")
    for m, n in model_counts.most_common():
        print(f"  {m:<28} {n}")
    print(f"\n## token economics")
    t = s["tokens"]
    print(f"  output:               {t['output']:>14,}")
    print(f"  input (uncached):     {t['input']:>14,}")
    print(f"  cache_creation:       {t['cache_creation']:>14,}")
    print(f"  cache_read:           {t['cache_read']:>14,}")
    print(f"  TOTAL input (real):   {s['total_input_real']:>14,}")
    print(f"  cache_hit_rate:       {s['cache_hit_rate_pct']}%")
    print(f"\n## weekly volume")
    for w, n in s["weekly_event_volume"].items():
        bar = "█" * min(40, int(n / max(s["weekly_event_volume"].values()) * 40))
        print(f"  {w}  {n:>6}  {bar}")
    print(f"\n## hourly distribution (UTC)")
    for h, n in s["hourly_distribution_utc"].items():
        bar = "█" * min(40, int(n / max(s["hourly_distribution_utc"].values()) * 40))
        print(f"  {h:>2}:00  {n:>6}  {bar}")
    print(f"\n## top words in Bobby's prompts (incoming requests)")
    for w, n in list(s["top_words_in_prompts"].items())[:20]:
        print(f"  {w:<28} {n}")
    print(f"\n## per-session summary")
    for sess in s["sessions_summary"]:
        print(f"  {sess['id']}")
        print(f"    events:       {sess['events']:>6}")
        print(f"    span:         {sess['first_ts']} → {sess['last_ts']}")
        print(f"    tools:        {sess['tools']}")
        print(f"    models:       {sess['models']}")
        print(f"    first prompt: {(sess['first_prompt'] or '')[:120]!r}")
        print()


if __name__ == "__main__":
    summary = analyze()
    print_facts(summary)
    OUT_FILE.write_text(json.dumps(summary, indent=2, default=str))
    print(f"\n→ wrote {OUT_FILE}", file=sys.stderr)
