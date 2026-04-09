"""Token usage analysis for Open Story sessions.

Aggregates input/output/cache tokens and estimates cost from the
event store. Can report totals, per-session breakdown, or daily trends.

Usage:
    uv run python scripts/token_usage.py                     # summary
    uv run python scripts/token_usage.py --by-session        # per-session breakdown
    uv run python scripts/token_usage.py --by-day            # daily trend
    uv run python scripts/token_usage.py --session-id abc    # single session
    uv run python scripts/token_usage.py --days 7            # last 7 days only
    uv run python scripts/token_usage.py --format json       # JSON output
    uv run python scripts/token_usage.py --test              # run self-tests
"""

import argparse
import json
import sqlite3
import sys
from dataclasses import dataclass, field, asdict
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional


# ── Pricing (per million tokens) ──
#
# Each model has a `base` rate (prompt ≤ 200K tokens) and an optional
# `tier_threshold` + `tier_multiplier`. When any single API call's prompt
# size (input + cache_read + cache_creation) exceeds `tier_threshold`,
# THAT call is billed at base × tier_multiplier — Anthropic charges per
# request, not per session, so we apply the multiplier per-call when
# computing cost.
#
# Sources:
#   https://docs.anthropic.com/en/docs/about-claude/pricing
#   Opus 4.6 1M context: prompts >200K tokens are 2× standard rates
#
# Default: Sonnet 4. Override with --model.
PRICING = {
    "sonnet": {
        "base": {"input": 3.00, "output": 15.00, "cache_read": 0.30, "cache_creation": 3.75},
    },
    "opus": {
        # Opus 4 standard. No tier above 200K.
        "base": {"input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75},
    },
    "opus-4-6": {
        # Opus 4.6 with 1M context window. Prompts ≤200K bill at base
        # rate; prompts >200K bill at 2× base.
        "base": {"input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75},
        "tier_threshold": 200_000,
        "tier_multiplier": 2.0,
    },
    "haiku": {
        "base": {"input": 0.80, "output": 4.00, "cache_read": 0.08, "cache_creation": 1.00},
    },
}


def call_prompt_size(usage: dict) -> int:
    """How many tokens are in the *prompt* for one API call.

    Anthropic's tier threshold (200K for Opus 4.6 1M) is measured against
    the prompt — input + cache_read + cache_creation. Output tokens don't
    count toward the threshold; they're billed separately.
    """
    return (
        usage.get("input_tokens", 0)
        + usage.get("cache_read_input_tokens", 0)
        + usage.get("cache_creation_input_tokens", 0)
    )


def call_cost(usage: dict, model: str) -> tuple[dict, bool]:
    """Compute cost for ONE API call with per-call tier detection.

    Returns (cost_dict, breached_tier) where `breached_tier` is True if
    this call's prompt exceeded the model's tier threshold.
    """
    spec = PRICING.get(model, PRICING["sonnet"])
    base = spec["base"]
    multiplier = 1.0
    breached = False
    threshold = spec.get("tier_threshold")
    if threshold is not None and call_prompt_size(usage) > threshold:
        multiplier = spec["tier_multiplier"]
        breached = True
    rates = {k: v * multiplier for k, v in base.items()}
    costs = {
        "input": usage.get("input_tokens", 0) * rates["input"] / 1_000_000,
        "output": usage.get("output_tokens", 0) * rates["output"] / 1_000_000,
        "cache_read": usage.get("cache_read_input_tokens", 0) * rates["cache_read"] / 1_000_000,
        "cache_creation": usage.get("cache_creation_input_tokens", 0) * rates["cache_creation"] / 1_000_000,
    }
    costs["total"] = sum(costs.values())
    return costs, breached


@dataclass
class TokenUsage:
    """Aggregated token counts for a scope (session, day, or total).

    Holds the raw per-call usage records so cost can be computed
    per-call (with tier detection) at any model. The aggregate token
    fields are kept for backwards compatibility with the JSON output
    and for fast totals.
    """
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_creation_tokens: int = 0
    message_count: int = 0
    # Per-call records — needed for tier-aware cost estimation. Each
    # entry is the raw `usage` dict from one API response.
    calls: list[dict] = field(default_factory=list)

    @property
    def total_tokens(self) -> int:
        return (
            self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_creation_tokens
        )

    @property
    def biggest_prompt(self) -> int:
        """Largest single-call prompt size (for tier-breach reporting)."""
        if not self.calls:
            return 0
        return max(call_prompt_size(c) for c in self.calls)

    def add_usage(self, usage: dict) -> None:
        """Add a single message's usage dict to the running totals."""
        self.input_tokens += usage.get("input_tokens", 0)
        self.output_tokens += usage.get("output_tokens", 0)
        self.cache_read_tokens += usage.get("cache_read_input_tokens", 0)
        self.cache_creation_tokens += usage.get("cache_creation_input_tokens", 0)
        self.message_count += 1
        self.calls.append(usage)

    def merge(self, other: "TokenUsage") -> None:
        """Merge another TokenUsage into this one."""
        self.input_tokens += other.input_tokens
        self.output_tokens += other.output_tokens
        self.cache_read_tokens += other.cache_read_tokens
        self.cache_creation_tokens += other.cache_creation_tokens
        self.message_count += other.message_count
        self.calls.extend(other.calls)

    def estimate_cost(self, model: str = "sonnet") -> dict:
        """Estimate cost in USD with per-call tier detection.

        Anthropic bills per-request, so the tier multiplier (e.g. 2× for
        Opus 4.6 prompts >200K) applies only to the calls that breach
        the threshold. We sum each call's cost individually rather than
        applying a flat rate to the aggregate totals.

        Includes a `tier_breaches` count and `biggest_prompt` so callers
        can warn the user when prompt sizes are unusually large.

        Also computes `without_cache` — the hypothetical bill if every
        cache_read had been billed as fresh input. This is the value
        prompt caching saves you, and it's the honest way to express the
        Max-plan / caching benefit on a per-session basis.
        """
        totals = {"input": 0.0, "output": 0.0, "cache_read": 0.0, "cache_creation": 0.0}
        without_cache_total = 0.0
        breaches = 0
        for call in self.calls:
            costs, breached = call_cost(call, model)
            for k in totals:
                totals[k] += costs[k]
            if breached:
                breaches += 1
            # "Without cache": what if every cache_read was a fresh input
            # token? cache_creation stays as-is (you'd still pay the
            # cache-write price the first time). This is the ceiling on
            # what caching saved you for THIS call.
            spec = PRICING.get(model, PRICING["sonnet"])
            base = spec["base"]
            multiplier = 2.0 if breached else 1.0
            input_rate = base["input"] * multiplier
            output_rate = base["output"] * multiplier
            cache_creation_rate = base["cache_creation"] * multiplier
            no_cache_call = (
                (call.get("input_tokens", 0) + call.get("cache_read_input_tokens", 0)) * input_rate
                + call.get("cache_creation_input_tokens", 0) * cache_creation_rate
                + call.get("output_tokens", 0) * output_rate
            ) / 1_000_000
            without_cache_total += no_cache_call
        totals["total"] = sum(totals.values())
        totals["tier_breaches"] = breaches
        totals["biggest_prompt"] = self.biggest_prompt
        totals["without_cache"] = without_cache_total
        totals["cache_savings"] = without_cache_total - totals["total"]
        return totals

    def to_dict(self) -> dict:
        # Don't serialize the raw calls list — it's an internal
        # detail and would bloat JSON output.
        d = {
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "cache_creation_tokens": self.cache_creation_tokens,
            "message_count": self.message_count,
            "total_tokens": self.total_tokens,
            "biggest_prompt": self.biggest_prompt,
        }
        return d


@dataclass
class SessionInfo:
    """Session metadata paired with its token usage."""
    session_id: str
    label: Optional[str] = None
    project_name: Optional[str] = None
    first_event: Optional[str] = None
    last_event: Optional[str] = None
    event_count: int = 0
    usage: TokenUsage = field(default_factory=TokenUsage)


def extract_usage(payload_str: str) -> Optional[dict]:
    """Extract the usage dict from an event payload JSON string.

    Returns None if no usage data is found.
    """
    try:
        d = json.loads(payload_str)
        usage = (
            d.get("data", {})
            .get("raw", {})
            .get("message", {})
            .get("usage", {})
        )
        if usage and "input_tokens" in usage:
            return usage
    except (json.JSONDecodeError, AttributeError):
        pass
    return None


def query_usage_by_session(
    db_path: str,
    session_id: Optional[str] = None,
    days: Optional[int] = None,
) -> list[SessionInfo]:
    """Query token usage grouped by session.

    Args:
        db_path: Path to the SQLite database.
        session_id: If set, only return this session.
        days: If set, only return sessions with activity in the last N days.

    Returns:
        List of SessionInfo with usage populated.
    """
    db = sqlite3.connect(db_path)
    cur = db.cursor()

    # Build session filter
    where_clauses = []
    params = []
    if session_id:
        where_clauses.append("s.id = ?")
        params.append(session_id)
    if days:
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
        where_clauses.append("s.last_event > ?")
        params.append(cutoff)

    where_sql = f"WHERE {' AND '.join(where_clauses)}" if where_clauses else ""

    cur.execute(
        f"""
        SELECT s.id, s.label, s.project_name, s.first_event, s.last_event, s.event_count
        FROM sessions s
        {where_sql}
        ORDER BY s.last_event DESC
        """,
        params,
    )
    sessions = {
        row[0]: SessionInfo(
            session_id=row[0],
            label=row[1],
            project_name=row[2],
            first_event=row[3],
            last_event=row[4],
            event_count=row[5] or 0,
        )
        for row in cur.fetchall()
    }

    if not sessions:
        db.close()
        return []

    # Query all assistant events with usage data
    session_ids = list(sessions.keys())
    placeholders = ",".join("?" * len(session_ids))
    cur.execute(
        f"""
        SELECT session_id, payload FROM events
        WHERE session_id IN ({placeholders})
        AND subtype IN (
            'message.assistant.text',
            'message.assistant.tool_use',
            'message.assistant.thinking'
        )
        AND payload LIKE '%input_tokens%'
        """,
        session_ids,
    )

    for sid, payload_str in cur.fetchall():
        usage = extract_usage(payload_str)
        if usage and sid in sessions:
            sessions[sid].usage.add_usage(usage)

    db.close()
    return list(sessions.values())


def query_usage_by_day(
    db_path: str,
    days: Optional[int] = None,
) -> dict[str, TokenUsage]:
    """Query token usage grouped by calendar day.

    Returns:
        Dict mapping date string (YYYY-MM-DD) to TokenUsage.
    """
    db = sqlite3.connect(db_path)
    cur = db.cursor()

    where_clause = ""
    params = []
    if days:
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
        where_clause = "AND e.timestamp > ?"
        params.append(cutoff)

    cur.execute(
        f"""
        SELECT e.timestamp, e.payload FROM events e
        WHERE e.subtype IN (
            'message.assistant.text',
            'message.assistant.tool_use',
            'message.assistant.thinking'
        )
        AND e.payload LIKE '%input_tokens%'
        {where_clause}
        ORDER BY e.timestamp
        """,
        params,
    )

    by_day: dict[str, TokenUsage] = {}
    for timestamp, payload_str in cur.fetchall():
        usage = extract_usage(payload_str)
        if usage and timestamp:
            day = timestamp[:10]
            if day not in by_day:
                by_day[day] = TokenUsage()
            by_day[day].add_usage(usage)

    db.close()
    return by_day


def format_number(n: int) -> str:
    """Format a number with commas."""
    return f"{n:,}"


def format_cost(cost: float) -> str:
    """Format a cost as dollars."""
    return f"${cost:.2f}"


def print_summary(sessions: list[SessionInfo], model: str = "sonnet") -> None:
    """Print a human-readable summary of token usage."""
    total = TokenUsage()
    for s in sessions:
        total.merge(s.usage)

    dates = [s.first_event for s in sessions if s.first_event]
    date_range = ""
    if dates:
        first = min(dates)[:10]
        last = max(s.last_event for s in sessions if s.last_event)[:10]
        date_range = f"{first} to {last}"

    print(f"Sessions:  {len(sessions)}")
    print(f"Span:      {date_range}")
    print(f"Messages:  {format_number(total.message_count)}")
    print()
    print("--- Token Usage ---")
    print(f"  Input tokens:          {format_number(total.input_tokens):>16}")
    print(f"  Output tokens:         {format_number(total.output_tokens):>16}")
    print(f"  Cache read tokens:     {format_number(total.cache_read_tokens):>16}")
    print(f"  Cache creation tokens: {format_number(total.cache_creation_tokens):>16}")
    print(f"  {'':─<25}{'':─>16}")
    print(f"  Total:                 {format_number(total.total_tokens):>16}")
    print()

    costs = total.estimate_cost(model)
    spec = PRICING.get(model, PRICING["sonnet"])
    threshold = spec.get("tier_threshold")
    print(f"--- Estimated Cost ({model} rates) ---")
    print(f"  Input:          {format_cost(costs['input']):>10}")
    print(f"  Output:         {format_cost(costs['output']):>10}")
    print(f"  Cache read:     {format_cost(costs['cache_read']):>10}")
    print(f"  Cache creation: {format_cost(costs['cache_creation']):>10}")
    print(f"  {'':─<18}{'':─>10}")
    print(f"  Total:          {format_cost(costs['total']):>10}")
    print()
    print("--- The cache earned its keep ---")
    print(f"  Without caching:    {format_cost(costs['without_cache']):>10}")
    print(f"  Actual:             {format_cost(costs['total']):>10}")
    print(f"  Cache saved you:    {format_cost(costs['cache_savings']):>10}")
    if costs['without_cache'] > 0:
        pct = 100 * costs['cache_savings'] / costs['without_cache']
        print(f"  ({pct:.1f}% off retail)")
    print()
    print(f"--- Prompt size ---")
    print(f"  Biggest single call: {format_number(costs['biggest_prompt'])} prompt tokens")
    if threshold is not None:
        if costs['tier_breaches'] > 0:
            print(f"  ⚠  {costs['tier_breaches']} call(s) breached the {format_number(threshold)} tier — billed at 2× base")
        else:
            print(f"  ✓ all {total.message_count} calls stayed under the {format_number(threshold)} tier")


def print_by_session(sessions: list[SessionInfo], model: str = "sonnet") -> None:
    """Print per-session token usage breakdown."""
    # Sort by output tokens descending
    sessions = sorted(sessions, key=lambda s: s.usage.output_tokens, reverse=True)

    print(f"{'Label':<40} {'Input':>10} {'Output':>10} {'Cache Read':>12} {'Cost':>8}")
    print("─" * 84)
    for s in sessions:
        if s.usage.message_count == 0:
            continue
        label = (s.label or s.session_id[:12])[:40]
        costs = s.usage.estimate_cost(model)
        print(
            f"{label:<40} "
            f"{format_number(s.usage.input_tokens):>10} "
            f"{format_number(s.usage.output_tokens):>10} "
            f"{format_number(s.usage.cache_read_tokens):>12} "
            f"{format_cost(costs['total']):>8}"
        )


def print_by_day(by_day: dict[str, TokenUsage], model: str = "sonnet") -> None:
    """Print daily token usage trend."""
    print(f"{'Date':<12} {'Input':>10} {'Output':>10} {'Cache Read':>12} {'Cost':>8}")
    print("─" * 56)
    for day in sorted(by_day.keys()):
        u = by_day[day]
        costs = u.estimate_cost(model)
        print(
            f"{day:<12} "
            f"{format_number(u.input_tokens):>10} "
            f"{format_number(u.output_tokens):>10} "
            f"{format_number(u.cache_read_tokens):>12} "
            f"{format_cost(costs['total']):>8}"
        )


def output_json(data: dict) -> None:
    """Print JSON output."""
    print(json.dumps(data, indent=2, default=str))


def find_db(data_dir: str) -> str:
    """Find the SQLite database file."""
    candidates = ["open-story.db", "events.db", "open_story.db"]
    for name in candidates:
        path = Path(data_dir) / name
        if path.exists():
            # Verify it has the events table
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


# ── Tests ──

def run_tests() -> None:
    """Run self-tests."""
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

    print("Running token_usage tests...\n")

    # TokenUsage basics
    u = TokenUsage()
    check("empty usage has zero totals", u.total_tokens == 0)
    check("empty usage has zero messages", u.message_count == 0)

    # add_usage
    u.add_usage({"input_tokens": 100, "output_tokens": 200, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 50})
    check("add_usage input", u.input_tokens == 100)
    check("add_usage output", u.output_tokens == 200)
    check("add_usage cache_read", u.cache_read_tokens == 300)
    check("add_usage cache_creation", u.cache_creation_tokens == 50)
    check("add_usage total", u.total_tokens == 650)
    check("add_usage message_count", u.message_count == 1)

    # add_usage accumulates
    u.add_usage({"input_tokens": 10, "output_tokens": 20})
    check("add_usage accumulates input", u.input_tokens == 110)
    check("add_usage accumulates output", u.output_tokens == 220)
    check("add_usage accumulates message_count", u.message_count == 2)
    check("missing keys default to zero", u.cache_read_tokens == 300)

    # merge
    u2 = TokenUsage(input_tokens=5, output_tokens=10, cache_read_tokens=15, cache_creation_tokens=20, message_count=3)
    u.merge(u2)
    check("merge input", u.input_tokens == 115)
    check("merge output", u.output_tokens == 230)
    check("merge cache_read", u.cache_read_tokens == 315)
    check("merge cache_creation", u.cache_creation_tokens == 70)
    check("merge message_count", u.message_count == 5)

    # estimate_cost — note: estimate_cost now uses per-call records,
    # so we have to seed via add_usage rather than dataclass kwargs.
    cost_u = TokenUsage()
    cost_u.add_usage({"input_tokens": 1_000_000, "output_tokens": 1_000_000})
    sonnet_cost = cost_u.estimate_cost("sonnet")
    check("sonnet input cost $3/MTok", abs(sonnet_cost["input"] - 3.0) < 0.01)
    check("sonnet output cost $15/MTok", abs(sonnet_cost["output"] - 15.0) < 0.01)
    check("sonnet total = input + output", abs(sonnet_cost["total"] - 18.0) < 0.01)

    opus_cost = cost_u.estimate_cost("opus")
    check("opus input cost $15/MTok", abs(opus_cost["input"] - 15.0) < 0.01)
    check("opus output cost $75/MTok", abs(opus_cost["output"] - 75.0) < 0.01)

    haiku_cost = cost_u.estimate_cost("haiku")
    check("haiku input cost $0.80/MTok", abs(haiku_cost["input"] - 0.80) < 0.01)

    # ── Opus 4.6 tier detection ──
    # A single small call (under 200K) bills at base rate.
    small = TokenUsage()
    small.add_usage({"input_tokens": 50_000, "output_tokens": 1_000})
    small_cost = small.estimate_cost("opus-4-6")
    check("opus-4-6 small call uses base rate",
          abs(small_cost["input"] - 50_000 * 15 / 1_000_000) < 0.01)
    check("opus-4-6 small call has 0 tier breaches", small_cost["tier_breaches"] == 0)
    check("opus-4-6 small biggest_prompt is 50K", small_cost["biggest_prompt"] == 50_000)

    # A single large call (>200K prompt) bills at 2× base.
    big = TokenUsage()
    big.add_usage({"input_tokens": 250_000, "output_tokens": 1_000})
    big_cost = big.estimate_cost("opus-4-6")
    check("opus-4-6 big call uses 2× rate",
          abs(big_cost["input"] - 250_000 * 30 / 1_000_000) < 0.01)
    check("opus-4-6 big call has 1 tier breach", big_cost["tier_breaches"] == 1)

    # Mixed: tier multiplier applies per-call, not per-session.
    mixed = TokenUsage()
    mixed.add_usage({"input_tokens": 50_000, "output_tokens": 100})    # base
    mixed.add_usage({"input_tokens": 250_000, "output_tokens": 100})  # 2x
    mixed_cost = mixed.estimate_cost("opus-4-6")
    expected_input = (50_000 * 15 + 250_000 * 30) / 1_000_000
    check("opus-4-6 mixed: tier applies per-call",
          abs(mixed_cost["input"] - expected_input) < 0.01)
    check("opus-4-6 mixed: 1 of 2 calls breached", mixed_cost["tier_breaches"] == 1)

    # Threshold counts cache_read + cache_creation toward prompt size.
    cached_big = TokenUsage()
    cached_big.add_usage({
        "input_tokens": 100,
        "cache_read_input_tokens": 250_000,
        "output_tokens": 50,
    })
    cached_big_cost = cached_big.estimate_cost("opus-4-6")
    check("opus-4-6 prompt size includes cache_read",
          cached_big_cost["tier_breaches"] == 1)

    # without_cache: a fully-cached call costs much less than the
    # hypothetical "all input" cost.
    cache_savings = TokenUsage()
    cache_savings.add_usage({
        "input_tokens": 100,
        "cache_read_input_tokens": 100_000,
        "output_tokens": 100,
    })
    cs_cost = cache_savings.estimate_cost("opus")
    # Actual: input @ $15 + cache_read @ $1.50 + output @ $75
    expected_actual = (100 * 15 + 100_000 * 1.5 + 100 * 75) / 1_000_000
    check("without_cache > actual when cache_read is large",
          cs_cost["without_cache"] > cs_cost["total"])
    check("cache_savings = without_cache - actual",
          abs(cs_cost["cache_savings"] - (cs_cost["without_cache"] - cs_cost["total"])) < 0.001)
    check("opus actual cost matches manual calc",
          abs(cs_cost["total"] - expected_actual) < 0.001)

    # extract_usage
    payload = json.dumps({
        "data": {
            "raw": {
                "message": {
                    "usage": {
                        "input_tokens": 42,
                        "output_tokens": 99,
                        "cache_read_input_tokens": 1000,
                        "cache_creation_input_tokens": 500,
                    }
                }
            }
        }
    })
    usage = extract_usage(payload)
    check("extract_usage returns dict", usage is not None)
    check("extract_usage input_tokens", usage["input_tokens"] == 42)
    check("extract_usage output_tokens", usage["output_tokens"] == 99)

    # extract_usage with invalid JSON
    check("extract_usage invalid json returns None", extract_usage("not json") is None)
    check("extract_usage empty object returns None", extract_usage("{}") is None)
    check("extract_usage no usage returns None", extract_usage('{"data":{}}') is None)

    # to_dict
    u3 = TokenUsage(input_tokens=1, output_tokens=2, cache_read_tokens=3, cache_creation_tokens=4, message_count=5)
    d = u3.to_dict()
    check("to_dict has total_tokens", d["total_tokens"] == 10)
    check("to_dict has message_count", d["message_count"] == 5)

    # format helpers
    check("format_number", format_number(1234567) == "1,234,567")
    check("format_cost", format_cost(42.5) == "$42.50")

    # unknown model falls back to sonnet
    fallback = cost_u.estimate_cost("unknown_model")
    check("unknown model uses sonnet rates", abs(fallback["input"] - 3.0) < 0.01)

    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Token usage analysis for Open Story")
    parser.add_argument("--data-dir", default="./data", help="Path to data directory")
    parser.add_argument("--session-id", help="Analyze a single session")
    parser.add_argument("--days", type=int, help="Only include last N days")
    parser.add_argument("--by-session", action="store_true", help="Show per-session breakdown")
    parser.add_argument("--by-day", action="store_true", help="Show daily trend")
    parser.add_argument("--model", default="sonnet", choices=list(PRICING.keys()), help="Pricing model (default: sonnet)")
    parser.add_argument("--format", choices=["text", "json"], default="text", help="Output format")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    db_path = find_db(args.data_dir)

    if args.by_day:
        by_day = query_usage_by_day(db_path, days=args.days)
        if args.format == "json":
            output_json({day: u.to_dict() for day, u in sorted(by_day.items())})
        else:
            print_by_day(by_day, model=args.model)
    else:
        sessions = query_usage_by_session(db_path, session_id=args.session_id, days=args.days)
        if args.format == "json":
            total = TokenUsage()
            for s in sessions:
                total.merge(s.usage)
            data = {
                "summary": {
                    **total.to_dict(),
                    "cost": total.estimate_cost(args.model),
                    "session_count": len(sessions),
                },
            }
            if args.by_session or args.session_id:
                data["sessions"] = [
                    {
                        "session_id": s.session_id,
                        "label": s.label,
                        "project_name": s.project_name,
                        "first_event": s.first_event,
                        "last_event": s.last_event,
                        "usage": s.usage.to_dict(),
                        "cost": s.usage.estimate_cost(args.model),
                    }
                    for s in sessions
                    if s.usage.message_count > 0
                ]
            output_json(data)
        elif args.by_session:
            print_by_session(sessions, model=args.model)
        else:
            print_summary(sessions, model=args.model)
