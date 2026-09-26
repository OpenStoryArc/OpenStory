#!/usr/bin/env python3
"""Fail the build on a swallowed fallible write.

`let _ = store.upsert(...).await;` throws away the only signal that a
persist, index, append, insert, or publish failed. The node ops audit
(docs/research/openstory-as-node/2026-09-23-logging-and-ops-hands.md) found
the persist consumer dropping plan saves, JSONL appends, FTS indexing, and
session upserts this way. REQUIREMENTS E-01: each such failure must log
`event=<op>_failed` and count. This script is the gate; `just test` runs it.

A line is a violation when it assigns a fallible operation to `_`:

    let _ = <anything>.<op>(...)        where <op> starts with one of OPS

Escape hatch, for calls whose Err is not a failure (a broadcast send with
no receivers, say): put `// audit-ok: <reason>` on the same line or the
line above. Lines after a `#[cfg(test)]` marker in a file are not scanned;
files under a `tests/` directory or named `*_proptest.rs` are skipped.

    python3 scripts/swallowed_errors.py                 # scan the default roots
    python3 scripts/swallowed_errors.py rs/server/src   # scan given roots
    python3 scripts/swallowed_errors.py --test          # self-tests
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

DEFAULT_ROOTS = ["rs/server/src", "rs/src/server"]
OPS = ("save", "upsert", "append", "index", "insert", "publish", "write", "persist", "send", "flush", "put", "set_", "delete", "remove")
SWALLOW = re.compile(r"^\s*let\s+_\s*=\s*(?P<expr>.+?);?\s*(//.*)?$")
CALL = re.compile(r"(?:\.|::)(?P<op>[a-z_][a-z0-9_]*)\s*\(")
OK = re.compile(r"//\s*audit-ok:")
TEST_MARKER = re.compile(r"^\s*#\[cfg\(test\)\]")


def fallible_op(expr: str) -> str | None:
    """The first method call in `expr` whose name starts with a fallible op, else None."""
    for m in CALL.finditer(expr):
        op = m.group("op")
        if op.startswith(OPS):
            return op
    return None


def scan_text(text: str) -> list[tuple[int, str, str]]:
    """Violations in one file's text: (line_no, op, line)."""
    out = []
    prev = ""
    for no, line in enumerate(text.splitlines(), start=1):
        if TEST_MARKER.match(line):
            break
        m = SWALLOW.match(line)
        if m and not OK.search(line) and not OK.search(prev):
            op = fallible_op(m.group("expr"))
            if op:
                out.append((no, op, line.strip()))
        prev = line
    return out


def skip_file(path: Path) -> bool:
    parts = set(path.parts)
    return "tests" in parts or path.name.endswith("_proptest.rs")


def scan_roots(roots: list[str], base: Path) -> list[tuple[Path, int, str, str]]:
    found = []
    for root in roots:
        for path in sorted((base / root).rglob("*.rs")):
            if skip_file(path.relative_to(base)):
                continue
            for no, op, line in scan_text(path.read_text(encoding="utf-8", errors="replace")):
                found.append((path.relative_to(base), no, op, line))
    return found


def _test() -> int:
    fixture = """
use x;
fn a() {
    let _ = store.upsert_session(&row).await;
    let _ = tx.send(msg); // audit-ok: no receivers is not a failure
    // audit-ok: same, on the line above
    let _ = tx.send(other);
    let _ = ephemeral;
    let _ = child.kill();
    let r = store.insert_turn(&t).await;
    let _ = self.plan_store.save(id, &c, ts);
    let _ = event_store.index_fts_batch(&fts).await;
    let _ = std::fs::write(p, b);
}
#[cfg(test)]
mod tests {
    fn t() { let _ = store.upsert(x).await; }
}
"""
    got = scan_text(fixture)
    ops = [op for _, op, _ in got]
    assert ops == ["upsert_session", "save", "index_fts_batch", "write"], ops
    assert [n for n, _, _ in got] == [4, 11, 12, 13], got
    assert fallible_op("tx.send(msg)") == "send"
    assert fallible_op("child.kill()") is None
    assert fallible_op("ephemeral") is None
    assert skip_file(Path("rs/tests/helpers/mod.rs"))
    assert skip_file(Path("rs/server/src/consumers/broadcast_proptest.rs"))
    assert not skip_file(Path("rs/server/src/consumers/persist.rs"))
    print("ok: 8 assertions")
    return 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("roots", nargs="*", default=DEFAULT_ROOTS)
    p.add_argument("--test", action="store_true")
    a = p.parse_args(argv)
    if a.test:
        return _test()
    base = Path(__file__).resolve().parent.parent
    found = scan_roots(a.roots, base)
    for path, no, op, line in found:
        print(f"{path}:{no}: swallowed `{op}`: {line}")
    if found:
        print(f"\n{len(found)} swallowed fallible write(s). Log `event=<op>_failed` and count, or mark `// audit-ok: <reason>`.", file=sys.stderr)
        return 1
    print("ok: no swallowed fallible writes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
