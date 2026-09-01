#!/usr/bin/env python3
"""Config-table drift validator — sibling of check_docs.py.

The source of truth for configuration is the `Config` struct (and its
`Default` impl) in rs/server/src/config.rs. The CLAUDE.md "Configuration"
table is a curated, human-facing view of that struct. The two can drift:
a field gets renamed (boot_window_hours -> watch_backfill_hours), a field
gets removed but lingers in the table (semantic_enabled), or a default
changes in code but not in the docs.

Each side is internally coherent, so the drift is invisible until you
compare them mechanically. This script is that comparison.

Usage:
    python3 scripts/check_config.py            # validate, exit non-zero on failure
    python3 scripts/check_config.py --quiet    # only print failures + summary
    python3 scripts/check_config.py --test     # self-tests on synthetic fixtures
"""

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

CONFIG_RS = "rs/server/src/config.rs"
CONFIG_DOC = "CLAUDE.md"


# -- Data models ------------------------------------------------------

@dataclass
class CheckResult:
    name: str
    ok: bool
    detail: str = ""


@dataclass
class StructFacts:
    """What config.rs actually declares."""
    fields: set[str] = field(default_factory=set)
    # field -> normalized default (only for defaults we can map confidently)
    defaults: dict[str, str] = field(default_factory=dict)


# -- Rust parsing (pure: text in, facts out) --------------------------

def parse_struct_fields(rust: str) -> set[str]:
    """Field names declared in `pub struct Config { ... }`.

    Captures `pub <name>: <type>,` lines inside the struct body.
    """
    m = re.search(r"pub struct Config\s*\{(.*?)\n\}", rust, re.S)
    if not m:
        return set()
    body = m.group(1)
    return set(re.findall(r"^\s*pub\s+([a-z_][a-z0-9_]*)\s*:", body, re.M))


def normalize_rust_default(expr: str) -> str | None:
    """Map a Rust default expression to a comparable string, or None if
    the expression is too complex to compare confidently (e.g. it calls a
    helper like auto_detect_host()). None means "skip the default check
    for this field" — never a failure.
    """
    e = expr.strip().rstrip(",").strip()
    # Empties
    if e in ("String::new()", "String::default()"):
        return ""
    if e in ("Vec::new()", "Vec::default()", "vec![]"):
        return "[]"
    # String literals: "foo".to_string() / "foo".into() / "foo"
    sm = re.fullmatch(r'"([^"]*)"(?:\.to_string\(\)|\.to_owned\(\)|\.into\(\))?', e)
    if sm:
        return sm.group(1)
    # Booleans
    if e in ("true", "false"):
        return e
    # Numeric literals (allow underscores and type suffixes)
    nm = re.fullmatch(r"(\d[\d_]*)(?:_?[iu](?:8|16|32|64|size))?", e)
    if nm:
        return nm.group(1).replace("_", "")
    # Known enum variants
    enum_map = {
        "DataBackend::Sqlite": "sqlite",
        "DataBackend::Mongo": "mongo",
        "Role::Full": "full",
        "Role::Publisher": "publisher",
        "Role::Consumer": "consumer",
    }
    if e in enum_map:
        return enum_map[e]
    return None  # too complex — skip


def parse_struct_defaults(rust: str) -> dict[str, str]:
    """field -> normalized default, for the `impl Default for Config`.

    Only includes fields whose default expression we can normalize
    confidently. Complex expressions (helper calls) are intentionally
    omitted so they can't produce false positives.
    """
    m = re.search(
        r"impl Default for Config\s*\{.*?fn default\(\)\s*->\s*Self\s*\{\s*Self\s*\{(.*?)\n\s*\}\s*\n\s*\}",
        rust,
        re.S,
    )
    if not m:
        return {}
    body = m.group(1)
    out: dict[str, str] = {}
    for fname, expr in re.findall(r"([a-z_][a-z0-9_]*)\s*:\s*([^\n]+)", body):
        norm = normalize_rust_default(expr)
        if norm is not None:
            out[fname] = norm
    return out


def collect_struct_facts(rust: str) -> StructFacts:
    return StructFacts(
        fields=parse_struct_fields(rust),
        defaults=parse_struct_defaults(rust),
    )


# -- Markdown parsing -------------------------------------------------

def parse_doc_table(md: str) -> dict[str, str]:
    """field -> documented default, from the Configuration table.

    Reads markdown rows shaped like `| \`field\` | \`default\` | desc |`.
    Only rows whose first cell is a single backticked identifier count as
    field rows (skips header/separator rows and prose).
    """
    out: dict[str, str] = {}
    for line in md.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 2:
            continue
        fm = re.fullmatch(r"`([a-z_][a-z0-9_]*)`", cells[0])
        if not fm:
            continue
        out[fm.group(1)] = normalize_doc_default(cells[1])
    return out


def normalize_doc_default(cell: str) -> str:
    """Pull a comparable default out of a doc table cell.

    Cells look like: `3002`, `127.0.0.1`, `""` (no auth), `100000` (100KB),
    `sqlite`, `[]` (localhost). Take the first backticked token and
    normalize empties the same way the Rust side does.
    """
    bt = re.search(r"`([^`]*)`", cell)
    token = bt.group(1).strip() if bt else cell.strip()
    if token in ('""', "''", ""):
        return ""
    if token in ("[]",):
        return "[]"
    # numeric with thousands punctuation in prose (rare) — strip underscores
    return token.replace("_", "")


# -- Checks -----------------------------------------------------------

def check_no_ghost_fields(facts: StructFacts, doc: dict[str, str]) -> CheckResult:
    """Every field documented in the table must exist in the Config struct.

    Catches removed fields left in the docs (semantic_enabled) and renames
    (boot_window_hours -> watch_backfill_hours: the old name is now a ghost).
    """
    ghosts = sorted(f for f in doc if f not in facts.fields)
    return CheckResult(
        "no_ghost_fields",
        ok=not ghosts,
        detail=(
            f"documented but absent from Config struct: {', '.join(ghosts)}"
            if ghosts
            else f"all {len(doc)} documented fields exist in config.rs"
        ),
    )


def check_defaults_match(facts: StructFacts, doc: dict[str, str]) -> CheckResult:
    """Documented defaults must match code defaults — where both are
    simple enough to compare. Fields whose code default is a helper call
    (host, claude_watch_dir, ...) are skipped, never failed.
    """
    mismatches: list[str] = []
    compared = 0
    for fname, doc_default in doc.items():
        if fname not in facts.defaults:
            continue  # complex/skipped default, or ghost (caught elsewhere)
        compared += 1
        if facts.defaults[fname] != doc_default:
            mismatches.append(
                f"{fname}: doc={doc_default!r} code={facts.defaults[fname]!r}"
            )
    return CheckResult(
        "defaults_match",
        ok=not mismatches,
        detail=(
            "; ".join(mismatches)
            if mismatches
            else f"{compared} comparable defaults agree"
        ),
    )


def check_table_present(facts: StructFacts, doc: dict[str, str]) -> CheckResult:
    """Sanity: the doc actually has a parseable config table and the struct
    actually parsed. Guards against silent regex breakage.
    """
    problems = []
    if not facts.fields:
        problems.append("no Config struct fields parsed from config.rs")
    if not doc:
        problems.append("no config table rows parsed from CLAUDE.md")
    return CheckResult(
        "table_present",
        ok=not problems,
        detail="; ".join(problems) if problems else f"{len(doc)} rows / {len(facts.fields)} struct fields",
    )


CHECKS = [check_table_present, check_no_ghost_fields, check_defaults_match]


# -- Runner -----------------------------------------------------------

def evaluate(rust: str, md: str) -> list[CheckResult]:
    facts = collect_struct_facts(rust)
    doc = parse_doc_table(md)
    return [c(facts, doc) for c in CHECKS]


def run(repo: Path, quiet: bool = False) -> int:
    rust = (repo / CONFIG_RS).read_text(encoding="utf-8", errors="replace") if (repo / CONFIG_RS).is_file() else ""
    md = (repo / CONFIG_DOC).read_text(encoding="utf-8", errors="replace") if (repo / CONFIG_DOC).is_file() else ""
    results = evaluate(rust, md)
    failed = [r for r in results if not r.ok]
    for r in results:
        if quiet and r.ok:
            continue
        mark = "ok  " if r.ok else "FAIL"
        print(f"  {mark} {r.name}: {r.detail}")
    print(f"\n{len(results) - len(failed)}/{len(results)} passed, {len(failed)} failed")
    return 1 if failed else 0


# -- Self-tests -------------------------------------------------------

GOOD_RUST = '''
pub struct Config {
    pub host: String,
    pub port: u16,
    pub role: Role,
    pub data_backend: DataBackend,
    pub retention_days: u32,
    pub metrics_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: auto_detect_host(),
            port: 3002,
            role: Role::Full,
            data_backend: DataBackend::Sqlite,
            retention_days: 0,
            metrics_enabled: false,
        }
    }
}
'''

GOOD_DOC = """
| Field | Default | Description |
|-------|---------|-------------|
| `host` | `127.0.0.1` | bind addr |
| `port` | `3002` | listen port |
| `role` | `full` | server role |
| `data_backend` | `sqlite` | backend |
| `retention_days` | `0` (no cleanup) | cleanup |
| `metrics_enabled` | `false` | prometheus |
"""

GHOST_DOC = GOOD_DOC + "| `semantic_enabled` | `false` | gone |\n"

RENAME_DOC = GOOD_DOC + "| `boot_window_hours` | `24` | renamed |\n"

BAD_DEFAULT_DOC = GOOD_DOC.replace("| `port` | `3002` |", "| `port` | `8080` |")


def selftest() -> int:
    failures = 0

    def expect(label: str, results: list[CheckResult], name: str, want_ok: bool):
        nonlocal failures
        r = next(x for x in results if x.name == name)
        if r.ok != want_ok:
            print(f"  SELFTEST FAIL [{label}] {name}: ok={r.ok} want={want_ok} ({r.detail})")
            failures += 1
        else:
            print(f"  selftest ok [{label}] {name}")

    # Clean docs vs struct: everything passes.
    res = evaluate(GOOD_RUST, GOOD_DOC)
    expect("clean", res, "table_present", True)
    expect("clean", res, "no_ghost_fields", True)
    expect("clean", res, "defaults_match", True)

    # Ghost field (removed from code, lingering in docs) -> ghost check fires.
    res = evaluate(GOOD_RUST, GHOST_DOC)
    expect("ghost", res, "no_ghost_fields", False)

    # Renamed field: old name is now a ghost.
    res = evaluate(GOOD_RUST, RENAME_DOC)
    expect("rename", res, "no_ghost_fields", False)

    # Stale default: code says 3002, doc says 8080 -> default check fires.
    res = evaluate(GOOD_RUST, BAD_DEFAULT_DOC)
    expect("default", res, "defaults_match", False)
    expect("default", res, "no_ghost_fields", True)  # field still exists

    # Empty inputs -> table_present guards.
    res = evaluate("", "")
    expect("empty", res, "table_present", False)

    # Parsing spot-checks.
    facts = collect_struct_facts(GOOD_RUST)
    assert facts.defaults.get("port") == "3002", facts.defaults
    assert facts.defaults.get("data_backend") == "sqlite", facts.defaults
    assert facts.defaults.get("metrics_enabled") == "false", facts.defaults
    assert "host" not in facts.defaults, "auto_detect_host() should be skipped"
    doc = parse_doc_table(GOOD_DOC)
    assert doc.get("retention_days") == "0", doc
    print(f"\n{'all selftests passed' if not failures else f'{failures} selftest failures'}")
    return 1 if failures else 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--test", action="store_true", help="run self-tests and exit")
    parser.add_argument("--quiet", action="store_true", help="only print failures + summary")
    args = parser.parse_args()
    if args.test:
        sys.exit(selftest())
    sys.exit(run(REPO, quiet=args.quiet))


if __name__ == "__main__":
    main()
