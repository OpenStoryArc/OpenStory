"""Per-call model-aware cost estimation.

Extracts the actual model string from each assistant message's raw payload,
maps it to a pricing tier, and computes cost at the correct rate per call.
This gives actual spend rather than hypothetical "what if everything was Sonnet."

Usage:
    uv run python scripts/cost_by_model.py                  # total summary
    uv run python scripts/cost_by_model.py --by-day         # daily breakdown
    uv run python scripts/cost_by_model.py --by-week        # weekly breakdown
    uv run python scripts/cost_by_model.py --by-model       # per-model totals
    uv run python scripts/cost_by_model.py --days 7         # last 7 days
    uv run python scripts/cost_by_model.py --format json    # JSON output
    uv run python scripts/cost_by_model.py --test           # self-tests
"""

import argparse
import json
import re
import sqlite3
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional


# ── Pricing (per million tokens) ──
# Source: https://docs.anthropic.com/en/docs/about-claude/pricing
PRICING = {
    "opus": {
        "base": {"input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75},
    },
    "opus-4-6": {
        "base": {"input": 15.00, "output": 75.00, "cache_read": 1.50, "cache_creation": 18.75},
        "tier_threshold": 200_000,
        "tier_multiplier": 2.0,
    },
    "sonnet": {
        "base": {"input": 3.00, "output": 15.00, "cache_read": 0.30, "cache_creation": 3.75},
    },
    "haiku": {
        "base": {"input": 0.80, "output": 4.00, "cache_read": 0.08, "cache_creation": 1.00},
    },
}


def model_to_tier(model_str: str) -> str:
    """Map a raw model string to a pricing tier.

    Examples:
        claude-opus-4-6           -> opus-4-6
        claude-opus-4-0-20250115  -> opus
        claude-sonnet-4-20250514  -> sonnet
        claude-haiku-4-5-20251001 -> haiku
        <synthetic>               -> sonnet (fallback)
    """
    if not model_str or model_str == "<synthetic>":
        return "sonnet"  # conservative fallback
    m = model_str.lower()
    if "opus-4-6" in m or "opus-4.6" in m:
        return "opus-4-6"
    if "opus" in m:
        return "opus"
    if "haiku" in m:
        return "haiku"
    if "sonnet" in m:
        return "sonnet"
    return "sonnet"  # unknown model, fallback


def call_prompt_size(usage: dict) -> int:
    return (
        usage.get("input_tokens", 0)
        + usage.get("cache_read_input_tokens", 0)
        + usage.get("cache_creation_input_tokens", 0)
    )


def call_cost(usage: dict, tier: str) -> float:
    """Compute cost in USD for one API call at the given pricing tier.

    If usage carries an authoritative pre-computed ``cost`` field (pi-mono
    payloads do this — the agent records the cost at call time), trust that
    over the rate-card calculation. Falls back to per-token math otherwise.
    """
    if isinstance(usage.get("cost"), (int, float)):
        return float(usage["cost"])
    spec = PRICING.get(tier, PRICING["sonnet"])
    base = spec["base"]
    multiplier = 1.0
    threshold = spec.get("tier_threshold")
    if threshold is not None and call_prompt_size(usage) > threshold:
        multiplier = spec["tier_multiplier"]
    rates = {k: v * multiplier for k, v in base.items()}
    return (
        usage.get("input_tokens", 0) * rates["input"] / 1_000_000
        + usage.get("output_tokens", 0) * rates["output"] / 1_000_000
        + usage.get("cache_read_input_tokens", 0) * rates["cache_read"] / 1_000_000
        + usage.get("cache_creation_input_tokens", 0) * rates["cache_creation"] / 1_000_000
    )


def normalize_usage(usage: dict) -> Optional[dict]:
    """Normalize an assistant-message ``usage`` dict to the canonical shape.

    Two shapes are seen in the wild:

    - claude-code: ``input_tokens``, ``output_tokens``, ``cache_read_input_tokens``,
      ``cache_creation_input_tokens``
    - pi-mono: ``input``, ``output``, ``cacheRead``, ``cacheWrite``, plus an
      authoritative pre-computed ``cost`` in USD

    Returns a dict with the canonical claude-code keys (so downstream rate-card
    math works uniformly), and preserves pi-mono's ``cost`` when present so
    ``call_cost`` can prefer it. Returns ``None`` if the dict has neither shape.
    """
    if not usage:
        return None
    if "input_tokens" in usage:
        # already canonical
        out = {
            "input_tokens": usage.get("input_tokens", 0),
            "output_tokens": usage.get("output_tokens", 0),
            "cache_read_input_tokens": usage.get("cache_read_input_tokens", 0),
            "cache_creation_input_tokens": usage.get("cache_creation_input_tokens", 0),
        }
        if isinstance(usage.get("cost"), (int, float)):
            out["cost"] = float(usage["cost"])
        return out
    if "input" in usage and ("cacheRead" in usage or "cacheWrite" in usage or "output" in usage):
        out = {
            "input_tokens": usage.get("input", 0),
            "output_tokens": usage.get("output", 0),
            "cache_read_input_tokens": usage.get("cacheRead", 0),
            "cache_creation_input_tokens": usage.get("cacheWrite", 0),
        }
        if isinstance(usage.get("cost"), (int, float)):
            out["cost"] = float(usage["cost"])
        return out
    return None


@dataclass
class Bucket:
    """Accumulates token counts and model-aware cost."""
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_creation_tokens: int = 0
    message_count: int = 0
    cost: float = 0.0
    model_counts: dict = field(default_factory=lambda: defaultdict(int))

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_creation_tokens

    def add(self, usage: dict, tier: str, model_str: str) -> None:
        self.input_tokens += usage.get("input_tokens", 0)
        self.output_tokens += usage.get("output_tokens", 0)
        self.cache_read_tokens += usage.get("cache_read_input_tokens", 0)
        self.cache_creation_tokens += usage.get("cache_creation_input_tokens", 0)
        self.message_count += 1
        self.cost += call_cost(usage, tier)
        self.model_counts[model_str] += 1

    def merge(self, other: "Bucket") -> None:
        self.input_tokens += other.input_tokens
        self.output_tokens += other.output_tokens
        self.cache_read_tokens += other.cache_read_tokens
        self.cache_creation_tokens += other.cache_creation_tokens
        self.message_count += other.message_count
        self.cost += other.cost
        for k, v in other.model_counts.items():
            self.model_counts[k] += v

    def to_dict(self) -> dict:
        return {
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "cache_creation_tokens": self.cache_creation_tokens,
            "total_tokens": self.total_tokens,
            "message_count": self.message_count,
            "cost": round(self.cost, 2),
            "models": dict(self.model_counts),
        }


def query_calls(db_path: str, days: Optional[int] = None) -> list[dict]:
    """Query all assistant messages with usage data, returning per-call records.

    Each record has: timestamp, session_id, model, usage dict.
    """
    db = sqlite3.connect(db_path)
    cur = db.cursor()

    where = ""
    params = []
    if days:
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
        where = "AND e.timestamp > ?"
        params.append(cutoff)

    # Match either the claude-code shape (``input_tokens``) or the pi-mono shape
    # (``cacheRead`` / ``cacheWrite``). The LIKE filter is just a pre-screen —
    # ``normalize_usage`` does the authoritative shape check below.
    cur.execute(
        f"""
        SELECT e.timestamp, e.session_id, e.payload
        FROM events e
        WHERE e.subtype IN (
            'message.assistant.text',
            'message.assistant.tool_use',
            'message.assistant.thinking'
        )
        AND (e.payload LIKE '%input_tokens%'
             OR e.payload LIKE '%cacheRead%'
             OR e.payload LIKE '%cacheWrite%')
        {where}
        ORDER BY e.timestamp
        """,
        params,
    )

    calls = []
    for timestamp, session_id, payload_str in cur.fetchall():
        try:
            d = json.loads(payload_str)
            raw = d.get("data", {}).get("raw", {})
            msg = raw.get("message", {})
            usage = msg.get("usage", {})
            model = msg.get("model", "")
            normalized = normalize_usage(usage)
            if normalized is not None:
                calls.append({
                    "timestamp": timestamp,
                    "session_id": session_id,
                    "model": model,
                    "usage": normalized,
                })
        except (json.JSONDecodeError, AttributeError):
            pass

    db.close()
    return calls


def aggregate_total(calls: list[dict]) -> Bucket:
    b = Bucket()
    for c in calls:
        tier = model_to_tier(c["model"])
        b.add(c["usage"], tier, c["model"])
    return b


def aggregate_by_day(calls: list[dict]) -> dict[str, Bucket]:
    by_day: dict[str, Bucket] = {}
    for c in calls:
        day = c["timestamp"][:10]
        if day not in by_day:
            by_day[day] = Bucket()
        tier = model_to_tier(c["model"])
        by_day[day].add(c["usage"], tier, c["model"])
    return by_day


def aggregate_by_week(calls: list[dict]) -> dict[str, Bucket]:
    by_week: dict[str, Bucket] = {}
    for c in calls:
        ts = c["timestamp"][:10]
        try:
            dt = datetime.strptime(ts, "%Y-%m-%d")
            # Monday of that week
            monday = dt - timedelta(days=dt.weekday())
            week_key = monday.strftime("%Y-%m-%d")
        except ValueError:
            continue
        if week_key not in by_week:
            by_week[week_key] = Bucket()
        tier = model_to_tier(c["model"])
        by_week[week_key].add(c["usage"], tier, c["model"])
    return by_week


def aggregate_by_model(calls: list[dict]) -> dict[str, Bucket]:
    by_model: dict[str, Bucket] = {}
    for c in calls:
        tier = model_to_tier(c["model"])
        if tier not in by_model:
            by_model[tier] = Bucket()
        by_model[tier].add(c["usage"], tier, c["model"])
    return by_model


def fmt(n: int) -> str:
    return f"{n:,}"


def fmt_cost(c: float) -> str:
    return f"${c:,.2f}"


def fmt_tokens_short(n: int) -> str:
    if n >= 1_000_000_000:
        return f"{n/1_000_000_000:.1f}B"
    if n >= 1_000_000:
        return f"{n/1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n/1_000:.0f}K"
    return str(n)


def print_total(b: Bucket) -> None:
    print(f"Messages:      {fmt(b.message_count)}")
    print(f"Total tokens:  {fmt(b.total_tokens)}")
    print(f"Actual cost:   {fmt_cost(b.cost)}")
    print()
    print("Models used:")
    for model, count in sorted(b.model_counts.items(), key=lambda x: -x[1]):
        tier = model_to_tier(model)
        print(f"  {model:<35} {count:>6} calls  (tier: {tier})")
    print()
    print("Token breakdown:")
    print(f"  Input:          {fmt(b.input_tokens):>16}")
    print(f"  Output:         {fmt(b.output_tokens):>16}")
    print(f"  Cache read:     {fmt(b.cache_read_tokens):>16}")
    print(f"  Cache creation: {fmt(b.cache_creation_tokens):>16}")


def print_time_table(buckets: dict[str, Bucket], label: str = "Period") -> None:
    print(f"{label:<14} {'Msgs':>6} {'Tokens':>10} {'Cost':>10} {'Models'}")
    print("─" * 70)
    for key in sorted(buckets.keys()):
        b = buckets[key]
        models = ", ".join(
            f"{model_to_tier(m)}×{c}"
            for m, c in sorted(b.model_counts.items(), key=lambda x: -x[1])
        )
        print(f"{key:<14} {b.message_count:>6} {fmt_tokens_short(b.total_tokens):>10} {fmt_cost(b.cost):>10}  {models}")
    # Total row
    total = Bucket()
    for b in buckets.values():
        total.merge(b)
    print("─" * 70)
    print(f"{'TOTAL':<14} {total.message_count:>6} {fmt_tokens_short(total.total_tokens):>10} {fmt_cost(total.cost):>10}")


def print_by_model(buckets: dict[str, Bucket]) -> None:
    print(f"{'Tier':<12} {'Msgs':>6} {'Tokens':>12} {'Cost':>10}")
    print("─" * 44)
    total_cost = 0.0
    for tier in sorted(buckets.keys()):
        b = buckets[tier]
        total_cost += b.cost
        print(f"{tier:<12} {b.message_count:>6} {fmt_tokens_short(b.total_tokens):>12} {fmt_cost(b.cost):>10}")
    print("─" * 44)
    print(f"{'TOTAL':<12} {'':>6} {'':>12} {fmt_cost(total_cost):>10}")


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

    print("Running cost_by_model tests...\n")

    # model_to_tier mapping
    check("opus-4-6 model", model_to_tier("claude-opus-4-6") == "opus-4-6")
    check("opus-4-0 model", model_to_tier("claude-opus-4-0-20250115") == "opus")
    check("sonnet model", model_to_tier("claude-sonnet-4-20250514") == "sonnet")
    check("haiku model", model_to_tier("claude-haiku-4-5-20251001") == "haiku")
    check("synthetic fallback", model_to_tier("<synthetic>") == "sonnet")
    check("empty fallback", model_to_tier("") == "sonnet")
    check("unknown fallback", model_to_tier("gpt-4") == "sonnet")

    # call_cost at different tiers
    usage = {"input_tokens": 1_000_000, "output_tokens": 100_000}
    sonnet_c = call_cost(usage, "sonnet")
    opus_c = call_cost(usage, "opus")
    check("sonnet cost = $3 input + $1.50 output", abs(sonnet_c - 4.50) < 0.01)
    check("opus cost = $15 input + $7.50 output", abs(opus_c - 22.50) < 0.01)

    # opus-4-6 tier breach
    small = {"input_tokens": 50_000, "output_tokens": 1_000}
    big = {"input_tokens": 250_000, "output_tokens": 1_000}
    small_c = call_cost(small, "opus-4-6")
    big_c = call_cost(big, "opus-4-6")
    check("opus-4-6 small: base rate", abs(small_c - (50_000 * 15 + 1_000 * 75) / 1_000_000) < 0.01)
    check("opus-4-6 big: 2x rate", abs(big_c - (250_000 * 30 + 1_000 * 150) / 1_000_000) < 0.01)

    # Bucket accumulation
    b = Bucket()
    b.add({"input_tokens": 100, "output_tokens": 200, "cache_read_input_tokens": 300}, "sonnet", "claude-sonnet-4")
    check("bucket input", b.input_tokens == 100)
    check("bucket output", b.output_tokens == 200)
    check("bucket cache_read", b.cache_read_tokens == 300)
    check("bucket message_count", b.message_count == 1)
    check("bucket cost > 0", b.cost > 0)
    check("bucket model tracked", b.model_counts["claude-sonnet-4"] == 1)

    # Mixed model bucket
    b2 = Bucket()
    b2.add({"input_tokens": 100, "output_tokens": 100}, "sonnet", "claude-sonnet-4")
    b2.add({"input_tokens": 100, "output_tokens": 100}, "opus", "claude-opus-4")
    check("mixed bucket: 2 models", len(b2.model_counts) == 2)
    # opus call costs 5x more than sonnet for same tokens
    sonnet_part = call_cost({"input_tokens": 100, "output_tokens": 100}, "sonnet")
    opus_part = call_cost({"input_tokens": 100, "output_tokens": 100}, "opus")
    check("mixed bucket cost is sum of per-call costs", abs(b2.cost - (sonnet_part + opus_part)) < 0.001)

    # normalize_usage: claude-code shape passes through
    cc = normalize_usage({"input_tokens": 10, "output_tokens": 20, "cache_read_input_tokens": 30, "cache_creation_input_tokens": 40})
    check("normalize cc: input_tokens preserved", cc["input_tokens"] == 10)
    check("normalize cc: output_tokens preserved", cc["output_tokens"] == 20)
    check("normalize cc: cache_read preserved", cc["cache_read_input_tokens"] == 30)
    check("normalize cc: cache_creation preserved", cc["cache_creation_input_tokens"] == 40)

    # normalize_usage: pi-mono shape gets remapped
    pi = normalize_usage({"input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "cost": 0.0042, "totalTokens": 100})
    check("normalize pi: input → input_tokens", pi["input_tokens"] == 10)
    check("normalize pi: output → output_tokens", pi["output_tokens"] == 20)
    check("normalize pi: cacheRead → cache_read_input_tokens", pi["cache_read_input_tokens"] == 30)
    check("normalize pi: cacheWrite → cache_creation_input_tokens", pi["cache_creation_input_tokens"] == 40)
    check("normalize pi: cost preserved", pi["cost"] == 0.0042)

    # normalize_usage: empty / non-matching shapes return None
    check("normalize empty → None", normalize_usage({}) is None)
    check("normalize unknown → None", normalize_usage({"foo": 1}) is None)

    # call_cost prefers authoritative pi-mono cost when present
    pi_with_cost = {"input_tokens": 1_000_000, "output_tokens": 1_000_000, "cost": 0.42}
    check("call_cost trusts authoritative cost", abs(call_cost(pi_with_cost, "opus") - 0.42) < 1e-9)
    pi_no_cost = {"input_tokens": 1_000, "output_tokens": 1_000}
    check("call_cost falls back to rate card", call_cost(pi_no_cost, "sonnet") > 0)

    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Model-aware cost estimation for Open Story")
    parser.add_argument("--data-dir", default="./data", help="Path to data directory")
    parser.add_argument("--days", type=int, help="Only include last N days")
    parser.add_argument("--by-day", action="store_true", help="Daily breakdown")
    parser.add_argument("--by-week", action="store_true", help="Weekly breakdown")
    parser.add_argument("--by-model", action="store_true", help="Per-model totals")
    parser.add_argument("--format", choices=["text", "json"], default="text")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    db_path = find_db(args.data_dir)
    calls = query_calls(db_path, days=args.days)

    if not calls:
        print("No assistant messages with usage data found.")
        sys.exit(0)

    if args.format == "json":
        if args.by_day:
            data = {k: v.to_dict() for k, v in sorted(aggregate_by_day(calls).items())}
        elif args.by_week:
            data = {k: v.to_dict() for k, v in sorted(aggregate_by_week(calls).items())}
        elif args.by_model:
            data = {k: v.to_dict() for k, v in sorted(aggregate_by_model(calls).items())}
        else:
            data = aggregate_total(calls).to_dict()
        print(json.dumps(data, indent=2))
    elif args.by_day:
        print_time_table(aggregate_by_day(calls), "Date")
    elif args.by_week:
        print_time_table(aggregate_by_week(calls), "Week of")
    elif args.by_model:
        print_by_model(aggregate_by_model(calls))
    else:
        print_total(aggregate_total(calls))
