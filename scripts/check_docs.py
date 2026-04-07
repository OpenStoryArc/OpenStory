"""Validate docs against the live codebase. Catch stale claims with assertions.

Each check is a small pure function that returns (ok: bool, message: str).
The validator runs them all, prints a TAP-ish report, and exits non-zero on
any failure. Run it locally before committing docs changes; wire it into CI
to gate against future doc rot.

Usage:
    python3 scripts/check_docs.py            # validate the repo
    python3 scripts/check_docs.py --test     # run self-tests on synthetic fixtures
    python3 scripts/check_docs.py --quiet    # only print failures + summary
"""

from __future__ import annotations

import argparse
import re
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Files we validate. Each entry: (label, path-relative-to-repo).
DOCS = {
    "tour": "docs/architecture-tour.md",
    "soul_arch": "docs/soul/architecture.md",
    "soul_uses": "docs/soul/use-cases.md",
    "two_streams": "docs/design-two-streams.md",
    "claude_md": "CLAUDE.md",
    "readme": "README.md",
    "backlog": "docs/BACKLOG.md",
}


# -- Data models ------------------------------------------------------

@dataclass
class CheckResult:
    name: str
    ok: bool
    detail: str = ""


# -- Codebase fact extraction (pure-ish — reads files, no mutation) ---

def list_pattern_detectors(repo: Path) -> list[str]:
    """Detector source files in rs/patterns/src, excluding lib.rs."""
    pat_dir = repo / "rs" / "patterns" / "src"
    if not pat_dir.is_dir():
        return []
    return sorted(p.stem for p in pat_dir.glob("*.rs") if p.stem != "lib")


def list_consumers(repo: Path) -> list[str]:
    """Consumer actor source files in rs/server/src/consumers, excluding mod.rs."""
    cons_dir = repo / "rs" / "server" / "src" / "consumers"
    if not cons_dir.is_dir():
        return []
    return sorted(p.stem for p in cons_dir.glob("*.rs") if p.stem != "mod")


def list_crates(repo: Path) -> list[str]:
    """Workspace member crates from rs/Cargo.toml.

    Reads the [workspace] members array. The workspace root (`.`) counts as
    a member because it is the `open-story` lib package itself. Falls back
    to scanning rs/* for Cargo.toml if no workspace declaration is found.
    """
    cargo = repo / "rs" / "Cargo.toml"
    if cargo.is_file():
        text = cargo.read_text(encoding="utf-8", errors="replace")
        m = re.search(r"\[workspace\][^\[]*?members\s*=\s*\[([^\]]*)\]", text, re.S)
        if m:
            members = re.findall(r'"([^"]+)"', m.group(1))
            if members:
                return sorted(members)
    rs = repo / "rs"
    if not rs.is_dir():
        return []
    return sorted(d.name for d in rs.iterdir() if (d / "Cargo.toml").is_file())


PLACEHOLDER_NAMES = {"foo.py", "bar.py", "baz.py", "yourscript.py"}


def referenced_scripts(text: str) -> set[str]:
    """Extract scripts/foo.py references from doc text. Tolerant of backticks/inline.

    Filters out common placeholder names (foo.py, bar.py) used as illustrative
    examples — those aren't claims that the script exists.
    """
    # match scripts/<name>.py — not preceded by another path char
    pattern = re.compile(r"(?<![A-Za-z0-9_/])scripts/([A-Za-z0-9_./-]+\.py)")
    return {m.group(1) for m in pattern.finditer(text) if m.group(1) not in PLACEHOLDER_NAMES}


# -- Generic predicates -----------------------------------------------

MERGE_MARKERS = ("<<<<<<<", "=======", ">>>>>>>")


def has_merge_marker(text: str) -> bool:
    for line in text.splitlines():
        stripped = line.strip()
        for m in MERGE_MARKERS:
            if stripped.startswith(m):
                return True
    return False


def mentions(text: str, needle: str) -> bool:
    return needle.lower() in text.lower()


def read_doc(repo: Path, key: str) -> str:
    path = repo / DOCS[key]
    if not path.is_file():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


# -- Individual checks ------------------------------------------------
#
# Each check is named so the failing list is greppable. Add new checks
# at the bottom — keep them small and independent.

def check_no_merge_markers(repo: Path) -> CheckResult:
    """Assertion 1 — no markdown file in tracked docs/ has a merge marker."""
    bad: list[str] = []
    for key, rel in DOCS.items():
        path = repo / rel
        if not path.is_file():
            continue
        if has_merge_marker(path.read_text(encoding="utf-8", errors="replace")):
            bad.append(rel)
    return CheckResult(
        "no_merge_markers",
        ok=not bad,
        detail=f"merge markers in: {', '.join(bad)}" if bad else "clean",
    )


def check_pattern_detector_count(repo: Path) -> CheckResult:
    """Assertion 2 — docs that name a detector count must be accurate."""
    actual = list_pattern_detectors(repo)
    n = len(actual)
    fails: list[str] = []
    for key in ("tour", "soul_arch"):
        text = read_doc(repo, key)
        if not text:
            continue
        # Look for "N detectors" or "N streaming detectors" or "(N detectors)"
        for m in re.finditer(r"(\d+)\s+(?:streaming\s+)?detectors?", text, re.I):
            claimed = int(m.group(1))
            if claimed != n:
                fails.append(f"{DOCS[key]} claims {claimed}, actual {n}")
    return CheckResult(
        "pattern_detector_count",
        ok=not fails,
        detail="; ".join(fails) if fails else f"{n} detectors agreed: {actual}",
    )


def check_crate_count(repo: Path) -> CheckResult:
    """Assertion 3 — docs that name a crate count must match reality."""
    actual = list_crates(repo)
    n = len(actual)
    fails: list[str] = []
    for key in ("claude_md", "soul_arch", "tour", "readme"):
        text = read_doc(repo, key)
        if not text:
            continue
        for m in re.finditer(r"(\d+)\s+crates?", text, re.I):
            claimed = int(m.group(1))
            if claimed != n:
                fails.append(f"{DOCS[key]} claims {claimed}, actual {n}")
    return CheckResult(
        "crate_count",
        ok=not fails,
        detail="; ".join(fails) if fails else f"{n} crates agreed: {actual}",
    )


def check_tour_mentions_nats(repo: Path) -> CheckResult:
    """Assertion 4 — architecture-tour.md must mention NATS."""
    text = read_doc(repo, "tour")
    return CheckResult(
        "tour_mentions_nats",
        ok=mentions(text, "NATS"),
        detail="ok" if mentions(text, "NATS") else "no NATS reference in tour",
    )


def check_tour_mentions_consumers(repo: Path) -> CheckResult:
    """Assertion 5 — architecture-tour.md must mention the consumers/ directory."""
    text = read_doc(repo, "tour")
    has = "consumers/" in text or "consumers/mod.rs" in text or "consumers/persist" in text
    return CheckResult(
        "tour_mentions_consumers",
        ok=has,
        detail="ok" if has else "no consumers/ reference in tour",
    )


def check_soul_arch_mentions_nats(repo: Path) -> CheckResult:
    """Assertion 6 — soul/architecture.md pipeline must mention NATS."""
    text = read_doc(repo, "soul_arch")
    return CheckResult(
        "soul_arch_mentions_nats",
        ok=mentions(text, "NATS"),
        detail="ok" if mentions(text, "NATS") else "no NATS reference in soul/architecture",
    )


def check_no_phantom_crates(repo: Path) -> CheckResult:
    """Assertion 7 — docs must not list crates that aren't in the workspace.

    Catches the inverse of crate_count: a doc that names a crate which exists
    on disk but isn't a workspace member (e.g., orphaned `semantic` crate).
    Looks at architectural docs that enumerate crate names in code blocks or
    inline lists. Allowed exception: prose may explain why a directory is
    *not* a workspace member (e.g., "rs/semantic/ is vestigial").
    """
    members = set(list_crates(repo))
    # Crate-name candidates: rs/<name>/ directories that are NOT workspace members
    rs = repo / "rs"
    orphans: set[str] = set()
    if rs.is_dir():
        for d in rs.iterdir():
            if d.is_dir() and d.name not in members and d.name not in {"target", "tests"}:
                if (d / "Cargo.toml").is_file():
                    orphans.add(d.name)
    if not orphans:
        return CheckResult("no_phantom_crates", ok=True, detail="no orphaned crates on disk")

    fails: list[str] = []
    for key in ("tour", "soul_arch", "claude_md", "readme"):
        text = read_doc(repo, key)
        if not text:
            continue
        for orphan in orphans:
            # Look for the orphan as a "real" crate listing line:
            #  - "  semantic/   — ..."
            #  - "│   ├── semantic/"
            #  - "open-story-semantic"
            patterns = [
                rf"^\s*{re.escape(orphan)}/\s*[—\-│├└]",
                rf"├──\s*{re.escape(orphan)}/",
                rf"open-story-{re.escape(orphan)}\b",
            ]
            for pat in patterns:
                if re.search(pat, text, re.M):
                    # Allow if the surrounding paragraph explains it's vestigial / not a member
                    excerpt_start = max(text.lower().find(orphan) - 200, 0)
                    excerpt = text[excerpt_start : excerpt_start + 600].lower()
                    if any(
                        marker in excerpt
                        for marker in ("vestigial", "orphan", "not a workspace member", "not a member", "slated for removal")
                    ):
                        continue
                    fails.append(f"{DOCS[key]} lists orphan crate '{orphan}' without explanation")
                    break
    return CheckResult(
        "no_phantom_crates",
        ok=not fails,
        detail="; ".join(fails) if fails else f"orphans on disk ({sorted(orphans)}) acknowledged in prose",
    )


def check_use_case_4_no_ingest_events_fanout(repo: Path) -> CheckResult:
    """Assertion 8 — Use Case 4 must not still claim ingest_events() is the fan-out point."""
    text = read_doc(repo, "soul_uses")
    bad = re.search(
        r"`?ingest_events\(\)`?\s+(function\s+)?is\s+the\s+fan-?out\s+point",
        text,
        re.I,
    )
    return CheckResult(
        "use_case_4_no_ingest_events_fanout",
        ok=bad is None,
        detail="ok" if bad is None else f"stale phrase still present: {bad.group(0)!r}",
    )


def check_referenced_scripts_exist(repo: Path) -> CheckResult:
    """Assertion 9 — all scripts/foo.py paths referenced in docs must exist.

    Skips BACKLOG.md because the backlog describes future scripts that don't
    exist yet by design. Skips placeholder names like foo.py.
    """
    backlog_keys = {"backlog"}
    missing: list[tuple[str, str]] = []
    seen: dict[str, set[str]] = {}
    for key, rel in DOCS.items():
        if key in backlog_keys:
            continue
        text = read_doc(repo, key)
        if not text:
            continue
        for ref in referenced_scripts(text):
            seen.setdefault(ref, set()).add(rel)
    for script, sources in seen.items():
        if not (repo / "scripts" / script).is_file():
            for src in sources:
                missing.append((script, src))
    return CheckResult(
        "referenced_scripts_exist",
        ok=not missing,
        detail=(
            "; ".join(f"{s} (referenced in {src})" for s, src in missing)
            if missing
            else f"all {len(seen)} referenced scripts exist"
        ),
    )


def check_readme_mentions_sessionstory(repo: Path) -> CheckResult:
    """Assertion 10a — README.md must mention sessionstory.py."""
    text = read_doc(repo, "readme")
    return CheckResult(
        "readme_mentions_sessionstory",
        ok="sessionstory.py" in text,
        detail="ok" if "sessionstory.py" in text else "README missing sessionstory.py",
    )


def check_claude_md_mentions_sessionstory(repo: Path) -> CheckResult:
    """Assertion 10b — CLAUDE.md must mention sessionstory.py."""
    text = read_doc(repo, "claude_md")
    return CheckResult(
        "claude_md_mentions_sessionstory",
        ok="sessionstory.py" in text,
        detail="ok" if "sessionstory.py" in text else "CLAUDE.md missing sessionstory.py",
    )


def check_tour_references_sessionstory(repo: Path) -> CheckResult:
    """Assertion 11 — architecture-tour.md must point at sessionstory.py."""
    text = read_doc(repo, "tour")
    return CheckResult(
        "tour_references_sessionstory",
        ok="sessionstory.py" in text,
        detail="ok" if "sessionstory.py" in text else "tour missing sessionstory.py pointer",
    )


def check_consumer_count(repo: Path) -> CheckResult:
    """Assertion 12 — docs claiming N consumers must match reality."""
    actual = list_consumers(repo)
    n = len(actual)
    fails: list[str] = []
    for key in ("tour", "soul_arch", "claude_md"):
        text = read_doc(repo, key)
        if not text:
            continue
        for m in re.finditer(r"(\d+)\s+(?:independent\s+)?(?:actor[- ]?)?consumers?", text, re.I):
            claimed = int(m.group(1))
            if claimed != n:
                fails.append(f"{DOCS[key]} claims {claimed}, actual {n}")
    return CheckResult(
        "consumer_count",
        ok=not fails,
        detail="; ".join(fails) if fails else f"{n} consumers agreed: {actual}",
    )


CHECKS: list[Callable[[Path], CheckResult]] = [
    check_no_merge_markers,
    check_pattern_detector_count,
    check_crate_count,
    check_tour_mentions_nats,
    check_tour_mentions_consumers,
    check_soul_arch_mentions_nats,
    check_no_phantom_crates,
    check_use_case_4_no_ingest_events_fanout,
    check_referenced_scripts_exist,
    check_readme_mentions_sessionstory,
    check_claude_md_mentions_sessionstory,
    check_tour_references_sessionstory,
    check_consumer_count,
]


# -- Runner -----------------------------------------------------------

def run(repo: Path, quiet: bool = False) -> int:
    results = [check(repo) for check in CHECKS]
    passed = sum(1 for r in results if r.ok)
    failed = sum(1 for r in results if not r.ok)

    if not quiet:
        for r in results:
            mark = "ok  " if r.ok else "FAIL"
            print(f"  {mark} {r.name}: {r.detail}")
    else:
        for r in results:
            if not r.ok:
                print(f"  FAIL {r.name}: {r.detail}")

    print()
    print(f"{passed}/{len(results)} passed, {failed} failed")
    return 0 if failed == 0 else 1


# -- Self-tests -------------------------------------------------------

def selftest() -> int:
    failures = 0

    def expect(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok   {name}")
        else:
            failures += 1
            print(f"  FAIL {name}: {detail}")

    print("== predicates ==")
    expect("merge_marker detected", has_merge_marker("foo\n>>>>>>> master\nbar"))
    expect("merge_marker absent", not has_merge_marker("clean text"))
    expect(
        "merge_marker not in code prose",
        not has_merge_marker("the >>>>>>> in this sentence is mid-line"),
    )

    print()
    print("== referenced_scripts ==")
    text = "see `scripts/sessionstory.py` and scripts/query_store.py for more"
    refs = referenced_scripts(text)
    expect("backtick script extracted", "sessionstory.py" in refs)
    expect("plain script extracted", "query_store.py" in refs)
    refs2 = referenced_scripts("/some/scripts/notreal.py and /scripts/x.py")
    expect("nested path NOT extracted", "notreal.py" not in refs2)

    print()
    print("== mentions ==")
    expect("case-insensitive nats", mentions("uses nats jetstream", "NATS"))
    expect("absent", not mentions("plain text", "NATS"))

    print()
    print("== synthetic repo: failing fixture ==")
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        fake = Path(td)
        # Build a fake repo: 9 crates, 7 detectors, 4 consumers, but doc claims wrong counts
        for crate in ("core", "bus", "store", "views", "patterns", "semantic", "server", "src", "cli"):
            (fake / "rs" / crate).mkdir(parents=True)
            (fake / "rs" / crate / "Cargo.toml").write_text("")
        (fake / "rs" / "patterns" / "src").mkdir(parents=True)
        for det in ("agent_delegation", "error_recovery", "eval_apply", "git_flow", "sentence", "test_cycle", "turn_phase", "lib"):
            (fake / "rs" / "patterns" / "src" / f"{det}.rs").write_text("")
        (fake / "rs" / "server" / "src" / "consumers").mkdir(parents=True)
        for c in ("persist", "patterns", "projections", "broadcast", "mod"):
            (fake / "rs" / "server" / "src" / "consumers" / f"{c}.rs").write_text("")
        # Stale doc claiming 5 detectors, no NATS, with merge marker
        (fake / "docs").mkdir()
        (fake / "docs" / "architecture-tour.md").write_text(
            "Pipeline with 5 detectors and no event bus.\n>>>>>>> master\n"
        )
        (fake / "docs" / "soul").mkdir()
        (fake / "docs" / "soul" / "architecture.md").write_text("8 crates, 5 detectors, no event bus")
        (fake / "docs" / "soul" / "use-cases.md").write_text(
            "The `ingest_events()` function is the fan-out point. See scripts/ghost.py."
        )
        (fake / "docs" / "design-two-streams.md").write_text("ok")
        (fake / "docs" / "BACKLOG.md").write_text("ok")
        (fake / "CLAUDE.md").write_text("9 crates")
        (fake / "README.md").write_text("nothing about story")

        results = [c(fake) for c in CHECKS]
        names_failing = {r.name for r in results if not r.ok}
        expect("merge marker check fails", "no_merge_markers" in names_failing)
        expect("detector count check fails", "pattern_detector_count" in names_failing)
        expect("crate count check fails", "crate_count" in names_failing)
        expect("tour-NATS check fails", "tour_mentions_nats" in names_failing)
        expect("tour-consumers check fails", "tour_mentions_consumers" in names_failing)
        expect("soul-NATS check fails", "soul_arch_mentions_nats" in names_failing)
        # phantom-crate check: in failing fixture, no orphan crates exist on disk
        # so this check should pass (it only fails when an orphan IS on disk and unexplained)
        expect("use case 4 check fails", "use_case_4_no_ingest_events_fanout" in names_failing)
        expect("dead script ref fails", "referenced_scripts_exist" in names_failing)
        expect("readme-sessionstory fails", "readme_mentions_sessionstory" in names_failing)
        expect("claude-md-sessionstory fails", "claude_md_mentions_sessionstory" in names_failing)
        expect("tour-sessionstory fails", "tour_references_sessionstory" in names_failing)

    print()
    print("== synthetic repo: passing fixture ==")
    with tempfile.TemporaryDirectory() as td:
        fake = Path(td)
        for crate in ("core", "bus", "store", "views", "patterns", "semantic", "server", "src", "cli"):
            (fake / "rs" / crate).mkdir(parents=True)
            (fake / "rs" / crate / "Cargo.toml").write_text("")
        (fake / "rs" / "patterns" / "src").mkdir(parents=True)
        for det in ("agent_delegation", "error_recovery", "eval_apply", "git_flow", "sentence", "test_cycle", "turn_phase", "lib"):
            (fake / "rs" / "patterns" / "src" / f"{det}.rs").write_text("")
        (fake / "rs" / "server" / "src" / "consumers").mkdir(parents=True)
        for c in ("persist", "patterns", "projections", "broadcast", "mod"):
            (fake / "rs" / "server" / "src" / "consumers" / f"{c}.rs").write_text("")
        (fake / "scripts").mkdir()
        (fake / "scripts" / "real.py").write_text("")
        (fake / "docs").mkdir()
        (fake / "docs" / "architecture-tour.md").write_text(
            "9 crates, 7 detectors, NATS event bus, 4 consumers, see consumers/persist.rs and scripts/sessionstory.py"
        )
        (fake / "docs" / "soul").mkdir()
        (fake / "docs" / "soul" / "architecture.md").write_text(
            "9 crates, 7 detectors, includes semantic crate and NATS event bus"
        )
        (fake / "docs" / "soul" / "use-cases.md").write_text(
            "Independent actor consumers handle the fan-out. See scripts/real.py."
        )
        (fake / "docs" / "design-two-streams.md").write_text("ok")
        (fake / "docs" / "BACKLOG.md").write_text("ok")
        (fake / "CLAUDE.md").write_text("9 crates with 4 consumers and sessionstory.py")
        (fake / "README.md").write_text("Run sessionstory.py for the story")
        # Need a sessionstory.py reference to satisfy script existence check
        (fake / "scripts" / "sessionstory.py").write_text("")

        results = [c(fake) for c in CHECKS]
        names_failing = {r.name for r in results if not r.ok}
        expect("all checks pass on green fixture", not names_failing,
               f"unexpected failures: {names_failing}")

    print()
    if failures:
        print(f"FAILED: {failures}")
        return 1
    print("all tests passed")
    return 0


# -- CLI --------------------------------------------------------------

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
