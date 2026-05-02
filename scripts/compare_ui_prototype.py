"""Compare the openstory-ui-prototype against the real ui/ codebase.

Extracts factual deltas across six dimensions:
1. Scale  (file count, LOC, test count)
2. Stack  (dependencies — shared / unique to each)
3. Data model  (TypeScript interfaces & types — shared / unique)
4. Components  (tsx component files)
5. Tests  (presence, count, framework)
6. Architecture  (folder layout — monolithic vs. domain-partitioned)

Emits a markdown report on stdout. Exit code 0 on success, 2 on missing inputs.

Usage:
    python3 scripts/compare_ui_prototype.py
    python3 scripts/compare_ui_prototype.py --test
    python3 scripts/compare_ui_prototype.py \\
        --prototype /path/to/openstory-ui-prototype \\
        --ui /path/to/OpenStory/ui
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_PROTOTYPE = REPO.parent / "openstory-ui-prototype"
DEFAULT_UI = REPO / "ui"

# Matches `interface Foo` / `export interface Foo` at any indentation.
INTERFACE_RE = re.compile(r"^\s*(?:export\s+)?interface\s+(\w+)", re.M)
# Matches `type Foo =` but not `typeof` or `type: ...`.
TYPE_RE = re.compile(r"^\s*(?:export\s+)?type\s+(\w+)\s*[=<]", re.M)


@dataclass
class Codebase:
    name: str
    root: Path
    # Subdirs under root to scan for source (e.g. ["src"] or ["src/app"]).
    src_subdirs: list[str] = field(default_factory=lambda: ["src"])

    @property
    def package_json_path(self) -> Path:
        return self.root / "package.json"

    @property
    def pkg(self) -> dict:
        return json.loads(self.package_json_path.read_text())

    @property
    def deps(self) -> set[str]:
        return set(self.pkg.get("dependencies", {}))

    @property
    def dev_deps(self) -> set[str]:
        return set(self.pkg.get("devDependencies", {}))

    def src_roots(self) -> list[Path]:
        return [self.root / sub for sub in self.src_subdirs if (self.root / sub).exists()]

    def ts_files(self) -> list[Path]:
        out: list[Path] = []
        for src in self.src_roots():
            out.extend(p for p in src.rglob("*.ts") if "node_modules" not in p.parts)
            out.extend(p for p in src.rglob("*.tsx") if "node_modules" not in p.parts)
        return sorted(out)

    def tsx_component_files(self) -> list[Path]:
        return [p for p in self.ts_files() if p.suffix == ".tsx"]

    def loc(self) -> int:
        return sum(sum(1 for _ in p.open(encoding="utf-8", errors="replace")) for p in self.ts_files())

    def types_declared(self) -> set[str]:
        names: set[str] = set()
        for p in self.ts_files():
            text = p.read_text(encoding="utf-8", errors="replace")
            names.update(INTERFACE_RE.findall(text))
            names.update(TYPE_RE.findall(text))
        return names

    def test_files(self) -> list[Path]:
        candidates: list[Path] = []
        for d in [self.root / "tests", self.root / "test"]:
            if d.exists():
                candidates.extend(p for p in d.rglob("*.test.*"))
                candidates.extend(p for p in d.rglob("*.spec.*"))
        for src in self.src_roots():
            candidates.extend(p for p in src.rglob("*.test.*"))
            candidates.extend(p for p in src.rglob("*.spec.*"))
        return sorted(set(candidates))

    def test_framework(self) -> str:
        scripts = self.pkg.get("scripts", {})
        test_cmd = scripts.get("test", "")
        if "vitest" in test_cmd:
            return "vitest"
        if "jest" in test_cmd:
            return "jest"
        if "playwright" in test_cmd:
            return "playwright"
        return "none"

    def top_level_src_dirs(self) -> list[str]:
        """Top-level folders under each src root (domain partitioning signal)."""
        dirs: set[str] = set()
        for src in self.src_roots():
            for p in src.iterdir():
                if p.is_dir() and not p.name.startswith("."):
                    dirs.add(p.name)
        return sorted(dirs)


def shared(a: set[str], b: set[str]) -> list[str]:
    return sorted(a & b)


def only_in(a: set[str], b: set[str]) -> list[str]:
    return sorted(a - b)


def render_report(proto: Codebase, ui: Codebase) -> str:
    lines: list[str] = []
    lines.append(f"# UI comparison: `{proto.name}` vs `{ui.name}`\n")
    lines.append("## 1. Scale\n")
    lines.append("| Metric | Prototype | Real UI | Delta |")
    lines.append("| --- | ---: | ---: | ---: |")

    p_files = len(proto.ts_files())
    u_files = len(ui.ts_files())
    p_loc = proto.loc()
    u_loc = ui.loc()
    p_tsx = len(proto.tsx_component_files())
    u_tsx = len(ui.tsx_component_files())
    p_tests = len(proto.test_files())
    u_tests = len(ui.test_files())

    lines.append(f"| TS/TSX files | {p_files} | {u_files} | {u_files - p_files:+d} |")
    lines.append(f"| TSX components | {p_tsx} | {u_tsx} | {u_tsx - p_tsx:+d} |")
    lines.append(f"| Source LOC | {p_loc} | {u_loc} | {u_loc - p_loc:+d} |")
    lines.append(f"| Test files | {p_tests} | {u_tests} | {u_tests - p_tests:+d} |")
    lines.append(f"| Test framework | {proto.test_framework()} | {ui.test_framework()} | — |")
    lines.append("")

    lines.append("## 2. Stack (dependencies)\n")
    shared_deps = shared(proto.deps, ui.deps)
    proto_only = only_in(proto.deps, ui.deps)
    ui_only = only_in(ui.deps, proto.deps)
    lines.append(f"- **Shared runtime deps ({len(shared_deps)}):** {', '.join(f'`{d}`' for d in shared_deps) or '_none_'}")
    lines.append(f"- **Prototype-only runtime deps ({len(proto_only)}):** {', '.join(f'`{d}`' for d in proto_only) or '_none_'}")
    lines.append(f"- **Real-UI-only runtime deps ({len(ui_only)}):** {', '.join(f'`{d}`' for d in ui_only) or '_none_'}")
    lines.append("")

    lines.append("## 3. Data model (declared types & interfaces)\n")
    proto_types = proto.types_declared()
    ui_types = ui.types_declared()
    shared_types = shared(proto_types, ui_types)
    proto_only_types = only_in(proto_types, ui_types)
    ui_only_types = only_in(ui_types, proto_types)
    lines.append(f"- **Prototype declares:** {len(proto_types)} names")
    lines.append(f"- **Real UI declares:** {len(ui_types)} names")
    lines.append(f"- **Shared names ({len(shared_types)}):** {', '.join(f'`{t}`' for t in shared_types) or '_none_'}")
    if proto_only_types:
        sample = proto_only_types[:20]
        more = f" _(+{len(proto_only_types) - len(sample)} more)_" if len(proto_only_types) > len(sample) else ""
        lines.append(f"- **Prototype-only ({len(proto_only_types)}):** " + ", ".join(f"`{t}`" for t in sample) + more)
    if ui_only_types:
        sample = ui_only_types[:20]
        more = f" _(+{len(ui_only_types) - len(sample)} more)_" if len(ui_only_types) > len(sample) else ""
        lines.append(f"- **Real-UI-only ({len(ui_only_types)}):** " + ", ".join(f"`{t}`" for t in sample) + more)
    lines.append("")

    lines.append("## 4. Architecture (top-level source folders)\n")
    p_dirs = proto.top_level_src_dirs()
    u_dirs = ui.top_level_src_dirs()
    lines.append(f"- **Prototype:** {', '.join(f'`{d}/`' for d in p_dirs) or '_flat_'}")
    lines.append(f"- **Real UI:** {', '.join(f'`{d}/`' for d in u_dirs) or '_flat_'}")
    lines.append("")

    lines.append("## 5. Observations\n")
    obs: list[str] = []
    if p_tests == 0 and u_tests > 0:
        obs.append(f"- **Testing:** Prototype has zero tests; real UI has {u_tests} ({ui.test_framework()}). Any logic moved from prototype to real UI needs to grow tests.")
    if p_tsx == 1 or (p_tsx > 0 and u_tsx >= p_tsx * 3):
        obs.append("- **Structure:** Prototype is effectively monolithic (one App.tsx + UI primitives); real UI is partitioned by domain (events, session, story, etc.).")
    stack_shift = len(ui_only) + len(proto_only)
    if stack_shift:
        obs.append(f"- **Stack divergence:** {len(shared_deps)} deps in common, {stack_shift} unique across both. Integration requires picking a canonical stack.")
    if "rxjs" in ui.deps and "rxjs" not in proto.deps:
        obs.append("- **State model:** Real UI uses RxJS streams; prototype uses local React state. Any data-flow logic from the prototype must be ported to observables.")
    if proto_types and ui_types:
        if "Event" in shared_types or "Session" in shared_types:
            obs.append("- **Domain overlap:** Core shapes like `Event`/`Session` exist in both, but the prototype's definitions are ad-hoc (mock-driven) — the real types should win.")
    if not obs:
        obs.append("- _(no obvious red flags)_")
    lines.extend(obs)
    lines.append("")

    return "\n".join(lines)


# --------------------------------------------------------------------------- #
# --test mode
# --------------------------------------------------------------------------- #


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _make_fixture_proto(root: Path) -> None:
    _write(root / "package.json", json.dumps({
        "name": "fake-proto",
        "dependencies": {"react": "18", "motion": "1", "lucide-react": "0"},
        "devDependencies": {"vite": "6"},
        "scripts": {},
    }))
    _write(root / "src" / "app" / "App.tsx", """
interface Event { id: string; type: string; }
interface Session { id: string; events: Event[]; }
interface Person { sessions: Session[]; }
type FooOnly = number;
""".strip())
    _write(root / "src" / "app" / "components" / "ui" / "button.tsx", "export const Button = () => null;")


def _make_fixture_ui(root: Path) -> None:
    _write(root / "package.json", json.dumps({
        "name": "fake-ui",
        "dependencies": {"react": "19", "rxjs": "7"},
        "devDependencies": {"vitest": "3"},
        "scripts": {"test": "vitest run"},
    }))
    _write(root / "src" / "components" / "events" / "EventCard.tsx", """
interface Event { id: string; subtype: string; }
interface ViewRecord { id: string; }
type StreamKey = string;
""".strip())
    _write(root / "src" / "components" / "session" / "SessionList.tsx", "export const SessionList = () => null;")
    _write(root / "src" / "types" / "events.ts", "export interface Session { id: string; }")
    _write(root / "tests" / "EventCard.test.tsx", "// vitest")


def run_self_tests() -> int:
    """Verify the comparator logic against synthetic fixtures. Returns exit code."""
    failures: list[str] = []

    def check(name: str, cond: bool, detail: str = "") -> None:
        if not cond:
            failures.append(f"FAIL {name}: {detail}")
        else:
            print(f"ok   {name}")

    with tempfile.TemporaryDirectory() as tmp:
        proto_root = Path(tmp) / "proto"
        ui_root = Path(tmp) / "ui"
        _make_fixture_proto(proto_root)
        _make_fixture_ui(ui_root)

        proto = Codebase("fake-proto", proto_root, src_subdirs=["src"])
        ui = Codebase("fake-ui", ui_root, src_subdirs=["src"])

        check("proto.ts_files finds App.tsx + button.tsx", len(proto.ts_files()) == 2, f"got {len(proto.ts_files())}")
        check("ui.ts_files finds 3 src files", len(ui.ts_files()) == 3, f"got {len(ui.ts_files())}")
        check("proto.tsx_component_files == 2", len(proto.tsx_component_files()) == 2)
        check("ui.tsx_component_files == 2", len(ui.tsx_component_files()) == 2)

        p_types = proto.types_declared()
        u_types = ui.types_declared()
        check("proto finds Event/Session/Person/FooOnly", p_types == {"Event", "Session", "Person", "FooOnly"}, f"got {p_types}")
        check("ui finds Event/ViewRecord/StreamKey/Session", u_types == {"Event", "ViewRecord", "StreamKey", "Session"}, f"got {u_types}")
        check("shared types == {Event, Session}", set(shared(p_types, u_types)) == {"Event", "Session"})

        check("proto test_files empty", proto.test_files() == [])
        check("ui finds 1 test file", len(ui.test_files()) == 1)

        check("proto test_framework == none", proto.test_framework() == "none")
        check("ui test_framework == vitest", ui.test_framework() == "vitest")

        check("shared deps == {react}", set(shared(proto.deps, ui.deps)) == {"react"})
        check("proto-only deps includes motion", "motion" in only_in(proto.deps, ui.deps))
        check("ui-only deps includes rxjs", "rxjs" in only_in(ui.deps, proto.deps))

        # Integration smoke: render a report without blowing up.
        report = render_report(proto, ui)
        check("report contains '## 1. Scale'", "## 1. Scale" in report)
        check("report contains shared deps section", "Shared runtime deps" in report)
        check("report flags zero tests", "zero tests" in report)
        check("report flags RxJS vs React-state", "RxJS" in report)

    if failures:
        print("\n".join(failures), file=sys.stderr)
        print(f"\n{len(failures)} failure(s)", file=sys.stderr)
        return 1
    print("\nall self-tests passed")
    return 0


# --------------------------------------------------------------------------- #
# entry point
# --------------------------------------------------------------------------- #


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--prototype", type=Path, default=DEFAULT_PROTOTYPE,
                        help=f"path to prototype root (default: {DEFAULT_PROTOTYPE})")
    parser.add_argument("--ui", type=Path, default=DEFAULT_UI,
                        help=f"path to real UI root (default: {DEFAULT_UI})")
    parser.add_argument("--test", action="store_true", help="run self-tests on synthetic fixtures")
    args = parser.parse_args()

    if args.test:
        return run_self_tests()

    if not args.prototype.exists():
        print(f"error: prototype path does not exist: {args.prototype}", file=sys.stderr)
        return 2
    if not args.ui.exists():
        print(f"error: ui path does not exist: {args.ui}", file=sys.stderr)
        return 2

    proto = Codebase("openstory-ui-prototype", args.prototype, src_subdirs=["src"])
    ui = Codebase("open-story-ui", args.ui, src_subdirs=["src"])
    print(render_report(proto, ui))
    return 0


if __name__ == "__main__":
    sys.exit(main())
