#!/usr/bin/env python3
"""Static audit: who publishes what subject onto the bus (K-07, G-01, G-02).

Every `.publish(`, `.publish_bytes(`, and `.publish_one(` call under `rs/`
(sources only, not tests or benches) is mapped to the subject prefix it
writes. The sovereignty rules are then checked:

- `events.` and `local.` (observed history) may be published only by the
  watcher egress in `rs/src/server/mod.rs` and the peer re-injection in
  `rs/server/src/catch_up.rs`, which carry translated history and nothing
  else. Nothing else may write history.
- `rs/mcp/` may publish only `ops.proposal.` and `ui.` (authored).
- Every site must resolve to a prefix. A subject nobody can read is a
  subject nobody can audit.

    python3 scripts/subject_publishers.py          # audit rs/, table + verdict
    python3 scripts/subject_publishers.py --test   # self-tests on fixtures
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Files that may publish observed history, and why.
HISTORY_PUBLISHERS = {
    "rs/src/server/mod.rs": "the watcher egress: translated events, host-scoped",
    "rs/server/src/catch_up.rs": "peer re-injection of already-observed history",
}
HISTORY_PREFIXES = ("events.", "local.")
MCP_ALLOWED = ("ops.proposal.", "ui.")
# The transport implementation itself: it forwards whatever subject it is
# handed and is not a publisher in the audit's sense.
TRANSPORT_FILES = {"rs/bus/src/nats_bus.rs"}

CALL = re.compile(r"\.publish(?:_bytes|_one)?\(")
TEST_MARKER = re.compile(r"^\s*#\[cfg\(test\)\]", re.M)
LITERAL = re.compile(r'"([^"\\]*)"')

# How a computed subject resolves: (pattern on the argument or its binding) -> prefixes.
RESOLVERS = [
    (re.compile(r"egress_subject\("), ("events.", "local.")),
    (re.compile(r"command_subject\("), ("ops.command.",)),
    (re.compile(r"proposal_subject\("), ("ops.proposal.",)),
    (re.compile(r"\bui_subject\(|ui_events::"), ("ui.",)),
    (re.compile(r"\bsubject\(host, &principal_id\)|presence::subject\("), ("presence.",)),
]


def first_arg(text: str) -> str:
    """The first argument of a call whose `(` was just consumed."""
    depth = 0
    for i, c in enumerate(text):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return text[:i].strip()
            depth -= 1
        elif c == "," and depth == 0:
            return text[:i].strip()
    return text.strip()


def binding_of(src: str, name: str, before: int) -> str:
    """The text of the nearest `let <name> = …;` above `before`."""
    head = src[:before]
    m = None
    for m in re.finditer(rf"let\s+(?:mut\s+)?{re.escape(name)}\s*(?::[^=]+)?=\s*", head):
        pass
    if not m:
        return ""
    tail = src[m.end():]
    end = tail.find(";")
    return tail[: end if end >= 0 else 200]


SUBJECT_ROOTS = ("events", "local", "ops", "ui", "presence", "patterns", "changes")


def subject_literal(text: str) -> str | None:
    """The first string literal in `text` that reads as a subject."""
    for lit in LITERAL.findall(text):
        if lit.startswith(SUBJECT_ROOTS):
            return lit
    return None


def resolve_text(text: str) -> tuple[str, ...] | None:
    """Prefixes an expression writes under: a known helper first, else a
    literal that reads as a subject."""
    for pat, prefixes in RESOLVERS:
        if pat.search(text):
            return prefixes
    lit = subject_literal(text)
    return (lit,) if lit else None


def resolve(src: str, at: int, arg: str) -> tuple[str, ...] | None:
    """Prefixes a publish argument writes under, or None when unreadable."""
    direct = resolve_text(arg)
    if direct:
        return direct
    name = arg.lstrip("&").split(".")[0].strip()
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
        bound = binding_of(src, name, at)
        if bound:
            return resolve_text(bound)
    return None


def scan_source(src: str, rel: str) -> list[dict]:
    """Publish sites in one file: {file, line, arg, prefixes}."""
    marker = TEST_MARKER.search(src)
    if marker:
        src = src[: marker.start()]
    out = []
    for m in CALL.finditer(src):
        # Skip definitions (`fn publish(`) — the regex needs a leading dot so those never match.
        arg = first_arg(src[m.end():])
        line = src[: m.start()].count("\n") + 1
        out.append({"file": rel, "line": line, "arg": arg, "prefixes": resolve(src, m.start(), arg)})
    return out


def scan_tree(repo: Path) -> list[dict]:
    sites = []
    for path in sorted((repo / "rs").rglob("*.rs")):
        rel = path.relative_to(repo).as_posix()
        parts = set(path.relative_to(repo).parts)
        if "target" in parts or "tests" in parts or "benches" in parts or "/tests/" in rel:
            continue
        if rel in TRANSPORT_FILES:
            continue
        sites.extend(scan_source(path.read_text(encoding="utf-8", errors="replace"), rel))
    return sites


def violations(sites: list[dict]) -> list[str]:
    out = []
    for s in sites:
        where = f"{s['file']}:{s['line']}"
        if s["prefixes"] is None:
            out.append(f"{where}: cannot read the subject of `{s['arg']}`; use a literal or a known helper")
            continue
        for p in s["prefixes"]:
            if p.startswith(HISTORY_PREFIXES) and s["file"] not in HISTORY_PUBLISHERS:
                out.append(f"{where}: publishes observed history `{p}` outside the allowed files")
            if s["file"].startswith("rs/mcp/") and not p.startswith(MCP_ALLOWED):
                out.append(f"{where}: the MCP publishes `{p}`; only {MCP_ALLOWED} are its lanes")
    return out


def table(sites: list[dict]) -> str:
    rows = [(f"{s['file']}:{s['line']}", "|".join(s["prefixes"]) if s["prefixes"] else "?", HISTORY_PUBLISHERS.get(s["file"], "")) for s in sites]
    w = max((len(r[0]) for r in rows), default=10)
    return "\n".join(f"{a.ljust(w)}  {b.ljust(22)}  {c}" for a, b, c in rows)


def _test() -> int:
    good = '''
        let subject = crate::ui_events::ui_subject("control", &sid);
        s.bus.publish(&subject, &batch).await;
        bus.publish(&format!("ops.proposal.{hand}"), &b).await;
        let subject = format!(
            "changes.store.{}",
            sid
        );
        bus.publish_bytes(&subject, &payload).await;
        watcher_bus.publish(&egress_subject(subject, publish_sessions), &batch);
    #[cfg(test)]
        bus.publish("events.in.tests", &batch);
    '''
    sites = scan_source(good, "rs/src/server/mod.rs")
    assert [s["prefixes"] for s in sites] == [("ui.",), ("ops.proposal.{hand}",), ("changes.store.{}",), ("events.", "local.")], sites
    assert violations(sites) == [], violations(sites)

    bad_history = scan_source('bus.publish(&format!("events.{sid}"), &batch).await;', "rs/server/src/sneaky.rs")
    v = violations(bad_history)
    assert len(v) == 1 and "observed history" in v[0], v
    ok_history = scan_source('bus.publish(&format!("events.{sid}"), &batch).await;', "rs/server/src/catch_up.rs")
    assert violations(ok_history) == []

    mcp_bad = scan_source('bus.publish("events.x", &b).await;', "rs/mcp/src/tools/x.rs")
    v = violations(mcp_bad)
    assert any("the MCP publishes" in x for x in v) and any("observed history" in x for x in v), v
    unreadable = scan_source("bus.publish(&whatever, &b).await;", "rs/server/src/x.rs")
    v = violations(unreadable)
    assert len(v) == 1 and "cannot read" in v[0], v
    assert scan_source("fn publish(&self) {}", "x.rs") == [], "definitions are not sites"
    print("ok: 8 assertions")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--test", action="store_true")
    a = ap.parse_args(argv)
    if a.test:
        return _test()
    sites = scan_tree(REPO)
    print(table(sites))
    v = violations(sites)
    if v:
        print("\n" + "\n".join(v), file=sys.stderr)
        print(f"\n{len(v)} subject violation(s)", file=sys.stderr)
        return 1
    print(f"\nok: {len(sites)} publish sites, every subject readable, history only from the watcher and catch-up, MCP only ops.proposal./ui.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
