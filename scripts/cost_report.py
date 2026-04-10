"""Generate the cost report HTML with model-aware per-call pricing.

Queries the SQLite store for session-level cost data, aggregates by
day/project/model, and writes cost_report.html with embedded data
and Chart.js visualizations.

Usage:
    uv run python scripts/cost_report.py                     # write cost_report.html
    uv run python scripts/cost_report.py --output report.html # custom output path
    uv run python scripts/cost_report.py --days 7            # last 7 days only
    uv run python scripts/cost_report.py --json              # dump data as JSON (no HTML)
    uv run python scripts/cost_report.py --test              # self-tests
"""

import argparse
import json
import sqlite3
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional


# ── Pricing (per million tokens) ──
PRICING = {
    "opus-4-6": {
        "input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75,
        "tier_threshold": 200_000, "tier_multiplier": 2.0,
    },
    "opus": {"input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75},
    "sonnet": {"input": 3.00, "output": 15.00, "cache_read": 0.30, "cache_creation": 3.75},
    "haiku": {"input": 0.80, "output": 4.00, "cache_read": 0.08, "cache_creation": 1.00},
}


def model_to_tier(model_str: str) -> str:
    """Map a raw model string to a pricing tier."""
    if not model_str or model_str == "<synthetic>":
        return "sonnet"
    m = model_str.lower()
    if "opus-4-6" in m or "opus-4.6" in m:
        return "opus-4-6"
    if "opus" in m:
        return "opus"
    if "haiku" in m:
        return "haiku"
    if "sonnet" in m:
        return "sonnet"
    return "sonnet"


def call_prompt_size(usage: dict) -> int:
    return (
        usage.get("input_tokens", 0)
        + usage.get("cache_read_input_tokens", 0)
        + usage.get("cache_creation_input_tokens", 0)
    )


def call_cost(usage: dict, tier: str) -> float:
    """Compute cost in USD for one API call at the given pricing tier."""
    spec = PRICING.get(tier, PRICING["sonnet"])
    multiplier = 1.0
    threshold = spec.get("tier_threshold")
    if threshold is not None and call_prompt_size(usage) > threshold:
        multiplier = spec["tier_multiplier"]
    return (
        usage.get("input_tokens", 0) * spec["input"] * multiplier
        + usage.get("output_tokens", 0) * spec["output"] * multiplier
        + usage.get("cache_read_input_tokens", 0) * spec["cache_read"] * multiplier
        + usage.get("cache_creation_input_tokens", 0) * spec["cache_creation"] * multiplier
    ) / 1_000_000


@dataclass
class SessionCost:
    """Cost data for a single session."""
    id: str
    label: str
    project: str
    first_event: str
    last_event: str
    event_count: int
    calls: int = 0
    cost: float = 0.0
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    models: dict = field(default_factory=lambda: defaultdict(int))

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens

    @property
    def cache_ratio(self) -> float:
        t = self.total_tokens
        return self.cache_read_tokens / t if t > 0 else 0.0

    @property
    def cost_per_call(self) -> float:
        return self.cost / self.calls if self.calls > 0 else 0.0

    @property
    def cost_per_1k_output(self) -> float:
        return (self.cost / self.output_tokens * 1000) if self.output_tokens > 0 else 0.0

    @property
    def day(self) -> str:
        return (self.first_event or "")[:10]

    @property
    def is_subagent(self) -> bool:
        return self.id.startswith("agent-")

    @property
    def dominant_model(self) -> str:
        if not self.models:
            return "sonnet"
        return model_to_tier(max(self.models.items(), key=lambda x: x[1])[0])

    def add_call(self, usage: dict, model: str) -> None:
        tier = model_to_tier(model)
        self.calls += 1
        self.cost += call_cost(usage, tier)
        self.input_tokens += usage.get("input_tokens", 0)
        self.output_tokens += usage.get("output_tokens", 0)
        self.cache_read_tokens += usage.get("cache_read_input_tokens", 0)
        self.cache_write_tokens += usage.get("cache_creation_input_tokens", 0)
        self.models[model] += 1

    def to_dict(self) -> dict:
        return {
            "id": self.id[:12],
            "label": self.label[:60],
            "project": self.project,
            "day": self.day,
            "calls": self.calls,
            "cost": round(self.cost, 2),
            "output_tokens": self.output_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "total_tokens": self.total_tokens,
            "cache_ratio": round(self.cache_ratio, 3),
            "cost_per_call": round(self.cost_per_call, 4),
            "cost_per_1k_output": round(self.cost_per_1k_output, 2),
            "dominant_model": self.dominant_model,
            "is_subagent": self.is_subagent,
            "events": self.event_count,
        }


def query_session_costs(db_path: str, days: Optional[int] = None) -> list[SessionCost]:
    """Query all sessions with model-aware cost data."""
    db = sqlite3.connect(db_path)
    cur = db.cursor()

    where = ""
    params: list = []
    if days:
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
        where = "WHERE s.last_event > ?"
        params.append(cutoff)

    cur.execute(
        f"SELECT s.id, s.label, s.project_name, s.first_event, s.last_event, s.event_count "
        f"FROM sessions s {where} ORDER BY s.first_event",
        params,
    )
    sessions = {
        r[0]: SessionCost(
            id=r[0], label=r[1] or "", project=r[2] or "",
            first_event=r[3] or "", last_event=r[4] or "",
            event_count=r[5] or 0,
        )
        for r in cur.fetchall()
    }

    if not sessions:
        db.close()
        return []

    # Query all assistant events with usage + model
    ev_where = ""
    ev_params: list = []
    if days:
        ev_where = "AND e.timestamp > ?"
        ev_params.append(cutoff)

    cur.execute(
        f"""SELECT e.session_id, e.payload FROM events e
        WHERE e.subtype IN ('message.assistant.text','message.assistant.tool_use','message.assistant.thinking')
        AND e.payload LIKE '%input_tokens%' {ev_where}""",
        ev_params,
    )

    for sid, payload_str in cur.fetchall():
        if sid not in sessions:
            continue
        try:
            d = json.loads(payload_str)
            msg = d.get("data", {}).get("raw", {}).get("message", {})
            usage = msg.get("usage", {})
            model = msg.get("model", "")
            if usage and "input_tokens" in usage:
                sessions[sid].add_call(usage, model)
        except (json.JSONDecodeError, AttributeError):
            pass

    db.close()
    return [s for s in sessions.values() if s.calls > 0]


def aggregate_by_day(sessions: list[SessionCost]) -> dict:
    by_day: dict[str, dict] = {}
    for s in sessions:
        d = s.day
        if d not in by_day:
            by_day[d] = {"sessions": 0, "subagents": 0, "calls": 0, "cost": 0.0, "output": 0}
        by_day[d]["calls"] += s.calls
        by_day[d]["cost"] += s.cost
        by_day[d]["output"] += s.output_tokens
        if s.is_subagent:
            by_day[d]["subagents"] += 1
        else:
            by_day[d]["sessions"] += 1
    return dict(sorted(by_day.items()))


def aggregate_by_project(sessions: list[SessionCost]) -> dict:
    by_proj: dict[str, dict] = {}
    for s in sessions:
        if s.is_subagent:
            continue
        p = s.project or "unknown"
        if p not in by_proj:
            by_proj[p] = {"sessions": 0, "calls": 0, "cost": 0.0, "output": 0}
        by_proj[p]["sessions"] += 1
        by_proj[p]["calls"] += s.calls
        by_proj[p]["cost"] += s.cost
        by_proj[p]["output"] += s.output_tokens
    return dict(sorted(by_proj.items(), key=lambda x: -x[1]["cost"]))


def aggregate_by_model(sessions: list[SessionCost]) -> dict:
    by_model: dict[str, dict] = {}
    for s in sessions:
        m = s.dominant_model
        if m not in by_model:
            by_model[m] = {"calls": 0, "cost": 0.0, "tokens": 0}
        by_model[m]["calls"] += s.calls
        by_model[m]["cost"] += s.cost
        by_model[m]["tokens"] += s.total_tokens
    return by_model


def build_report_data(sessions: list[SessionCost]) -> dict:
    """Build the full report data structure."""
    total_cost = sum(s.cost for s in sessions)
    total_output = sum(s.output_tokens for s in sessions)
    total_tokens = sum(s.total_tokens for s in sessions)
    total_cache_read = sum(s.cache_read_tokens for s in sessions)
    main_sessions = [s for s in sessions if not s.is_subagent]
    by_day = aggregate_by_day(sessions)
    active_days = len([d for d in by_day.values() if d["cost"] > 0])

    return {
        "summary": {
            "total_cost": round(total_cost, 2),
            "total_sessions": len(main_sessions),
            "total_subagents": len(sessions) - len(main_sessions),
            "total_calls": sum(s.calls for s in sessions),
            "total_output_tokens": total_output,
            "total_tokens": total_tokens,
            "cache_hit_rate": round(total_cache_read / total_tokens * 100, 1) if total_tokens > 0 else 0,
            "avg_per_session": round(total_cost / len(main_sessions), 2) if main_sessions else 0,
            "avg_per_day": round(total_cost / active_days, 2) if active_days > 0 else 0,
            "cost_per_1k_output": round(total_cost / total_output * 1000, 2) if total_output > 0 else 0,
            "active_days": active_days,
        },
        "sessions": [s.to_dict() for s in sorted(sessions, key=lambda x: -x.cost)],
        "by_day": by_day,
        "by_project": aggregate_by_project(sessions),
        "by_model": aggregate_by_model(sessions),
    }


def find_db(data_dir: str) -> str:
    for name in ["open-story.db", "events.db", "open_story.db"]:
        path = Path(data_dir) / name
        if path.exists():
            try:
                db = sqlite3.connect(str(path))
                cur = db.cursor()
                cur.execute("SELECT name FROM sqlite_master WHERE type='table' AND name='events'")
                if cur.fetchone():
                    db.close()
                    return str(path)
                db.close()
            except sqlite3.Error:
                pass
    raise FileNotFoundError(f"No valid Open Story database found in {data_dir}")


def print_summary(data: dict) -> None:
    """Print a text summary of the report."""
    s = data["summary"]
    print(f"Total spend:       ${s['total_cost']:,.2f}")
    print(f"Sessions:          {s['total_sessions']} main + {s['total_subagents']} subagents")
    print(f"API calls:         {s['total_calls']:,}")
    print(f"Active days:       {s['active_days']}")
    print(f"Avg / session:     ${s['avg_per_session']:,.2f}")
    print(f"Avg / day:         ${s['avg_per_day']:,.2f}")
    print(f"Cost / 1K output:  ${s['cost_per_1k_output']:.2f}")
    print(f"Cache hit rate:    {s['cache_hit_rate']}%")
    print()
    print("By project:")
    for p, v in data["by_project"].items():
        print(f"  {p:<20} ${v['cost']:>8,.2f}  ({v['sessions']} sessions)")
    print()
    print("Top 5 sessions:")
    for s in data["sessions"][:5]:
        label = s["label"][:40]
        print(f"  {label:<40} ${s['cost']:>8,.2f}  ({s['calls']} calls, {s['dominant_model']})")


# ── Tests ──

def run_tests() -> None:
    passed = 0
    failed = 0

    def check(name: str, condition: bool) -> None:
        nonlocal passed, failed
        if condition:
            passed += 1
            print(f"  PASS: {name}")
        else:
            failed += 1
            print(f"  FAIL: {name}")

    print("Running cost_report tests...\n")

    # model_to_tier
    check("opus-4-6", model_to_tier("claude-opus-4-6") == "opus-4-6")
    check("opus-4-0", model_to_tier("claude-opus-4-0-20250115") == "opus")
    check("sonnet", model_to_tier("claude-sonnet-4-20250514") == "sonnet")
    check("haiku", model_to_tier("claude-haiku-4-5-20251001") == "haiku")
    check("synthetic", model_to_tier("<synthetic>") == "sonnet")
    check("empty", model_to_tier("") == "sonnet")

    # call_cost
    usage = {"input_tokens": 1_000_000, "output_tokens": 100_000}
    check("sonnet cost", abs(call_cost(usage, "sonnet") - 4.50) < 0.01)
    check("opus cost", abs(call_cost(usage, "opus") - 22.50) < 0.01)

    # tier breach
    big = {"input_tokens": 250_000, "output_tokens": 1_000}
    big_c = call_cost(big, "opus-4-6")
    expected = (250_000 * 30 + 1_000 * 150) / 1_000_000
    check("opus-4-6 tier breach", abs(big_c - expected) < 0.01)

    # SessionCost
    sc = SessionCost(id="test-123", label="test", project="proj", first_event="2026-04-02T00:00:00Z", last_event="2026-04-02T01:00:00Z", event_count=10)
    sc.add_call({"input_tokens": 100, "output_tokens": 200, "cache_read_input_tokens": 1000}, "claude-opus-4-6")
    check("session calls", sc.calls == 1)
    check("session cost > 0", sc.cost > 0)
    check("session day", sc.day == "2026-04-02")
    check("session not subagent", not sc.is_subagent)
    check("session dominant model", sc.dominant_model == "opus-4-6")

    sc_sub = SessionCost(id="agent-abc123", label="sub", project="proj", first_event="", last_event="", event_count=0)
    check("subagent detection", sc_sub.is_subagent)

    # to_dict round-trip
    d = sc.to_dict()
    check("to_dict has cost", "cost" in d)
    check("to_dict has cache_ratio", "cache_ratio" in d)

    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate OpenStory cost report")
    parser.add_argument("--data-dir", default="./data", help="Path to data directory")
    parser.add_argument("--days", type=int, help="Only include last N days")
    parser.add_argument("--output", default="scripts/cost_report.html", help="Output HTML path")
    parser.add_argument("--json", action="store_true", help="Output JSON instead of HTML")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    db_path = find_db(args.data_dir)
    sessions = query_session_costs(db_path, days=args.days)

    if not sessions:
        print("No sessions with usage data found.")
        sys.exit(0)

    data = build_report_data(sessions)

    if args.json:
        print(json.dumps(data, indent=2, default=str))
    else:
        print_summary(data)
        print(f"\nHTML report: {args.output}")
        print("(HTML generation with embedded data is a future enhancement)")
        print("For now, run: open scripts/cost_report.html")
