#!/usr/bin/env python3
"""Bucket a branch's commits by concern, so a kitchen-sink branch can be
read (and potentially split) one theme at a time.

Reads `git log <base>..<head>` and assigns each commit to exactly one
concern. Assignment is rule-based on the commit subject, with per-hash
overrides for the handful of commits whose subject prefix lies about
which concern they really serve (e.g. an `admin:`-prefixed commit that is
actually part of the share-policy invariant sequence).

Usage:
    python3 scripts/map_branch_commits.py [--base master] [--head HEAD]
    python3 scripts/map_branch_commits.py --test
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections import OrderedDict

# Concern buckets in the order they should be reported. Each bucket has a
# short key, a human title, and a one-line rationale for what belongs.
BUCKETS: "OrderedDict[str, str]" = OrderedDict([
    ("identity",   "Person / Principal identity — 'your fleet' (PersonId v1)"),
    ("codex",      "Codex multi-agent support + host/user/origin stamping"),
    ("homebrew",   "Homebrew distribution"),
    ("federation", "Federation transport — host-in-subject + JetStream sources (Ph 1-2)"),
    ("boot",       "Node boot / health / reproject correctness"),
    ("admin",      "Admin UI + topology (Admin v0 / v0.2)"),
    ("sharepolicy","Share-policy + invariants — private sessions (Phase 4)"),
    ("accounts",   "Person isolation via NATS accounts (Phase 5)"),
    ("roles",      "Permissions / roles (Phase 6)"),
    ("security",   "Security — red-team / SAST / supply-chain"),
    ("perf",       "Performance"),
    ("chore",      "Chores / build / cross-cutting fixes"),
])

# Per-hash overrides — applied first, win over keyword rules. Keyed by the
# short hash as printed by `git log --format=%h`.
OVERRIDES = {
    "64f9ed6": "codex",       # origin_agent fixtures came with host-stamping
    "3293925": "sharepolicy", # "admin: share_policy storage" — really Phase 4
    "9db312a": "sharepolicy", # derive_live_sources fix, tagged Phase 4.5-4.6
    "6765486": "sharepolicy", # Cargo.lock for the 4.3-4.4 dev-dep
    "dded6db": "identity",    # merge of feat/person-id-fleet-view (PR #54)
    "9677a0d": "identity",    # Person/Principal config types ("fleet identity")
    "e66ba8c": "identity",    # expose person_id/principal_id + GET /api/fleet
    "37c6858": "identity",    # sidebar "your fleet" view
    "a9d14d6": "identity",    # first-boot [person] config bootstrap
    "f70f052": "identity",    # PersonId v1 shipped docs
    "47fc440": "admin",       # NatsLeafnodeHub evidence (Admin v0.2)
    "742a210": "admin",       # auto-discover leafnode hub via /leafz (Admin v0.2)
    "056296a": "chore",       # "two pre-existing PR #54 merge regressions"
    "8d7810a": "chore",       # mongo upsert delimiter fix
    "de9b433": "chore",       # nats_integration test isolation
    "c7ecc9f": "accounts",    # init-accounts-conf / up-multi-account (Phase 5 infra)
    "7fc64e8": "accounts",    # boot-wire AccountConfigWriter (Phase 5 polish)
    "5b4c4ea": "roles",       # Participants panel + grant-role bootstrap
    "5493ca9": "perf",        # perf test harness
    "3525a07": "chore",       # docker copy mcp/benches
    "03bd153": "chore",       # justfile NATS fallback + pi fixture
    "6a6e095": "chore",       # architecture audit consolidation doc
}

# Ordered keyword rules: (bucket, regex against the subject, case-insensitive).
# First match wins. Order matters — more specific themes come before generic.
RULES = [
    ("homebrew",    r"\bbrew\b|homebrew|formula|bottle"),
    ("codex",       r"\bcodex\b|multi-agent|host/user stamping|host-stamping|origin_agent"),
    ("roles",       r"Phase 6|admin_token|role-based|role-required|permission profile|per-role"),
    ("accounts",    r"Phase 5|multi-account|accounts-config|AccountConfigWriter|cross-person|cross-account|share-with-person|person_id\)|clusters by person"),
    ("sharepolicy", r"Phase 4|share[_ ]policy|invariant|RequirePublicSession|private session|skips private|fail-closed"),
    ("federation",  r"federation|fleet|leaf|hub|JetStream sources|host token|host-prefixed|host in (the )?subject|digest|catch-up|anti-entropy|mirror|Idea A|topology HTML|scale[- ]ramp|scale harness"),
    ("boot",        r"reproject|/api/health|boot scan|stored-vs-live|projection divergence"),
    ("admin",       r"\badmin\b|topology|live_sources|leafnode|parse_js_api_prefix|derive_live_sources|jetstream\(\) "),
    ("identity",    r"person_id|principal|personhood|fleet view|your fleet|Person/Principal|directory.*conformance|Keycloak"),
    ("security",    r"security|red-team|red_team|cargo-vet|cargo-geiger|geiger|gitleaks|semgrep|bandit|hadolint|CVE|supply-chain|npm signature"),
    ("perf",        r"\bperf\b|batch the persist|batch reconcile|fsync"),
    ("chore",       r"^chore|^docs|^fix|^style|^test|Cargo\.lock|justfile|docker"),
]


def classify(short_hash: str, subject: str) -> str:
    if short_hash in OVERRIDES:
        return OVERRIDES[short_hash]
    for bucket, pattern in RULES:
        if re.search(pattern, subject, re.IGNORECASE):
            return bucket
    return "chore"  # last-resort catch-all


def git_commits(base: str, head: str):
    out = subprocess.check_output(
        ["git", "log", f"{base}..{head}", "--format=%h\x1f%s", "--reverse"],
        text=True,
    )
    for line in out.splitlines():
        if not line.strip():
            continue
        h, subj = line.split("\x1f", 1)
        yield h, subj


def build_report(commits):
    grouped: "OrderedDict[str, list]" = OrderedDict((k, []) for k in BUCKETS)
    for h, subj in commits:
        grouped[classify(h, subj)].append((h, subj))
    return grouped


def render(grouped) -> str:
    total = sum(len(v) for v in grouped.values())
    lines = [f"# Branch commit map — {total} commits across "
             f"{sum(1 for v in grouped.values() if v)} concerns\n"]
    for key, title in BUCKETS.items():
        rows = grouped[key]
        if not rows:
            continue
        lines.append(f"## {title}  ({len(rows)})")
        for h, subj in rows:
            lines.append(f"  {h}  {subj}")
        lines.append("")
    return "\n".join(lines)


def _test() -> int:
    cases = [
        ("8540b13", "feat(brew): Homebrew formula + bottle workflow", "homebrew"),
        ("da8cecb", "feat(multi-agent): Codex support + host/user stamping", "codex"),
        ("64f9ed6", "fix(schemas): regenerate drifted schemas + fill origin_agent", "codex"),
        ("c186f4e", "feat(server): admin_token tier separation (Phase 6.1+6.2)", "roles"),
        ("c812abd", "test(bus): multi-account NATS isolation smoke (Phase 5.1)", "accounts"),
        ("3293925", "feat(admin): share_policy storage + GET/PUT endpoints + UI toggle", "sharepolicy"),
        ("61f4f72", "feat(api): RequirePublicSession extractor (Phase 4.2)", "sharepolicy"),
        ("5cfd473", "feat(federation): host token in NATS subject (Phase 1)", "federation"),
        ("0e8d731", "feat(state): reproject — rebuild projections at boot", "boot"),
        ("742a210", "feat(admin): auto-discover leafnode hub via NATS /leafz", "admin"),
        ("26831c6", "feat(core): add person_id and principal_id to CloudEvent", "identity"),
        ("07f8561", "security: comprehensive red-team audit — fix 6 CVEs", "security"),
        ("08f3ed9", "perf(ingest): batch the persist write path", "perf"),
        ("de9b433", "test(bus): isolate nats_integration tests", "chore"),
    ]
    ok = True
    for h, subj, want in cases:
        got = classify(h, subj)
        flag = "ok" if got == want else "FAIL"
        if got != want:
            ok = False
        print(f"  [{flag}] {h} → {got} (want {want}): {subj}")
    print("PASS" if ok else "FAILURES")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="master")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--test", action="store_true", help="run classifier self-tests")
    args = ap.parse_args()
    if args.test:
        return _test()
    grouped = build_report(git_commits(args.base, args.head))
    print(render(grouped))
    return 0


if __name__ == "__main__":
    sys.exit(main())
