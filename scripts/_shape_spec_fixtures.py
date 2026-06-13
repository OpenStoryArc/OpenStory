#!/usr/bin/env python3
"""Capture the pure-function contracts of the shape builders as canonical
input→output fixtures.

The Rust port in `rs/shapes/` must reproduce these exactly. The build scripts
(`build_bash_shapes.py`, `build_path_shapes.py`, `build_change_shapes.py`) are
API-driven and have no `--test` flag, but their decomposition functions are
pure — this script exercises them on representative inputs and emits the
expected outputs, which become the `#[cfg(test)]` fixtures for the Rust
extractors.

Usage:
    python3 scripts/_shape_spec_fixtures.py          # print fixtures
    python3 scripts/_shape_spec_fixtures.py --json    # machine-readable
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent


def _load(mod_name: str):
    spec = importlib.util.spec_from_file_location(mod_name, SCRIPTS / f"{mod_name}.py")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[mod_name] = mod  # dataclass decorator needs the module registered
    spec.loader.exec_module(mod)
    return mod


def bash_fixtures(m) -> list[dict]:
    cases = [
        "git commit -m 'init'",
        "cargo test -p open-story-shapes",
        "/usr/bin/git status",
        "cat foo.txt | grep bar > out.txt",
        "ls -la",
        "echo \"a | b\" && npm run build",
    ]
    return [{"command": c, "decompose": m.decompose(c)} for c in cases]


def path_fixtures(m) -> list[dict]:
    cases = [
        "/Users/x/projects/OpenStory/rs/shapes/src/lib.rs",
        "rs/store/src/sqlite_store.rs",
        "ui/tests/streams/event-transforms.spec.ts",
        "scripts/build_bash_shapes.py",
        "docs/research/shape-layer-architecture.md",
        "Cargo.toml",
    ]
    return [{"path": p, "decompose": m.decompose(p)} for p in cases]


def change_fixtures(m) -> list[dict]:
    cases = [
        ("hello\nworld", ""),
        ("a\nb\nc\n", "a\n"),
        ("", "x"),
        ("single line no newline", ""),
    ]
    return [
        {"new": new, "old": old,
         "lines_new": m.count_lines(new), "lines_old": m.count_lines(old)}
        for new, old in cases
    ]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    bash = _load("build_bash_shapes")
    path = _load("build_path_shapes")
    change = _load("build_change_shapes")

    fixtures = {
        "bash": bash_fixtures(bash),
        "path": path_fixtures(path),
        "change": change_fixtures(change),
    }

    if args.json:
        print(json.dumps(fixtures, indent=2))
        return 0

    for layer, cases in fixtures.items():
        print(f"\n=== {layer}-shape fixtures ===")
        for c in cases:
            print(json.dumps(c, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
