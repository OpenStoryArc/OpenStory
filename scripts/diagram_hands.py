#!/usr/bin/env python3
"""
diagram_hands.py — prototype diagram libraries as agent "hands" for OpenStory.

Explores:
  · Mermaid CLI (mmdc)  — text → SVG
  · D2                 — text → SVG
  · Pure layered grid  — nodes/edges → pen strokes (0..1) without native deps
  · Optional push to agent pen via POST /api/control

Philosophy (OpenStory):
  Libraries can be hands (emit structure) and eventually eyes (read diagrams).
  Output stays ui.* when pushed to the pen — never observed history.

Usage:
  python3 scripts/diagram_hands.py --demo              # render all demos to scripts/out/diagram-hands/
  python3 scripts/diagram_hands.py --demo --push       # also draw best vector board on pen
  python3 scripts/diagram_hands.py --session SID --push
  python3 scripts/diagram_hands.py --test

Requires: mmdc (mermaid-cli), d2 optional; curl not required (urllib).
"""

from __future__ import annotations

import argparse
import base64
import json
import math
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "scripts" / "out" / "diagram-hands"
API = "http://127.0.0.1:3002"


# ── pure layered layout → pen strokes ──────────────────────────────────────


@dataclass
class Node:
    id: str
    label: str
    layer: int
    fill: str = "#dbeafe"
    stroke: str = "#1e3a5f"


@dataclass
class Edge:
    src: str
    dst: str
    label: str = ""


@dataclass
class Graph:
    title: str
    subtitle: str = ""
    nodes: list[Node] = field(default_factory=list)
    edges: list[Edge] = field(default_factory=list)


def layered_layout(
    g: Graph,
    *,
    top: float = 0.12,
    bottom: float = 0.88,
    left: float = 0.06,
    right: float = 0.94,
    box_h: float = 0.07,
) -> dict[str, tuple[float, float, float, float]]:
    """Return id -> (x, y, w, h) in unit space, layers top→bottom, left→right."""
    layers: dict[int, list[Node]] = {}
    for n in g.nodes:
        layers.setdefault(n.layer, []).append(n)
    if not layers:
        return {}
    max_layer = max(layers)
    min_layer = min(layers)
    span = max(1, max_layer - min_layer)
    out: dict[str, tuple[float, float, float, float]] = {}
    for li, layer in sorted(layers.items()):
        y = top + (bottom - top - box_h) * ((li - min_layer) / span if span else 0)
        n = len(layer)
        gap = 0.02
        usable = right - left
        w = min(0.28, (usable - gap * (n - 1)) / max(n, 1))
        total = n * w + (n - 1) * gap
        x0 = left + (usable - total) / 2
        for i, node in enumerate(layer):
            x = x0 + i * (w + gap)
            out[node.id] = (x, y, w, box_h)
    return out


def pen_box(
    x: float, y: float, w: float, h: float, fill: str, stroke: str, label: str, font: int = 12
) -> list[dict[str, Any]]:
    return [
        {
            "type": "path",
            "points": [
                {"x": x, "y": y},
                {"x": x + w, "y": y},
                {"x": x + w, "y": y + h},
                {"x": x, "y": y + h},
            ],
            "closed": True,
            "fill": fill,
            "stroke": stroke,
            "strokeWidth": 2,
        },
        {
            "type": "text",
            "x": x + w / 2,
            "y": y + h / 2 + 0.008,
            "text": label[:40],
            "fill": stroke,
            "fontSize": font,
        },
    ]


def pen_arrow(
    x1: float, y1: float, x2: float, y2: float, color: str = "#334155"
) -> list[dict[str, Any]]:
    ang = math.atan2(y2 - y1, x2 - x1)
    ah = 0.012
    return [
        {
            "type": "line",
            "x1": x1,
            "y1": y1,
            "x2": x2,
            "y2": y2,
            "stroke": color,
            "strokeWidth": 1.8,
        },
        {
            "type": "path",
            "points": [
                {"x": x2, "y": y2},
                {"x": x2 + ah * math.cos(ang + 2.6), "y": y2 + ah * math.sin(ang + 2.6)},
                {"x": x2 + ah * math.cos(ang - 2.6), "y": y2 + ah * math.sin(ang - 2.6)},
            ],
            "closed": True,
            "fill": color,
            "stroke": color,
            "strokeWidth": 1,
        },
    ]


def graph_to_pen_strokes(g: Graph) -> list[dict[str, Any]]:
    """Hand: pure layout → DrawStroke wire JSON."""
    pos = layered_layout(g)
    strokes: list[dict[str, Any]] = []
    strokes.append(
        {
            "type": "text",
            "x": 0.5,
            "y": 0.04,
            "text": g.title[:60],
            "fill": "#0f172a",
            "fontSize": 18,
        }
    )
    if g.subtitle:
        strokes.append(
            {
                "type": "text",
                "x": 0.5,
                "y": 0.07,
                "text": g.subtitle[:80],
                "fill": "#64748b",
                "fontSize": 11,
            }
        )
    # edges first (under boxes)
    for e in g.edges:
        if e.src not in pos or e.dst not in pos:
            continue
        x1, y1, w1, h1 = pos[e.src]
        x2, y2, w2, h2 = pos[e.dst]
        # bottom-center → top-center
        sx, sy = x1 + w1 / 2, y1 + h1
        dx, dy = x2 + w2 / 2, y2
        strokes.extend(pen_arrow(sx, sy, dx, dy))
        if e.label:
            strokes.append(
                {
                    "type": "text",
                    "x": (sx + dx) / 2,
                    "y": (sy + dy) / 2,
                    "text": e.label[:20],
                    "fill": "#64748b",
                    "fontSize": 9,
                }
            )
    by_id = {n.id: n for n in g.nodes}
    for nid, (x, y, w, h) in pos.items():
        n = by_id[nid]
        strokes.extend(pen_box(x, y, w, h, n.fill, n.stroke, n.label))
    strokes.append(
        {
            "type": "text",
            "x": 0.5,
            "y": 0.96,
            "text": "diagram_hands · layered grid (no Mermaid/D2 in this layer)",
            "fill": "#94a3b8",
            "fontSize": 10,
        }
    )
    return strokes


# ── OpenStory demo graphs ──────────────────────────────────────────────────


def openstory_arch_graph() -> Graph:
    return Graph(
        title="OpenStory architecture",
        subtitle="libraries as hands → pen · observe, never interfere",
        nodes=[
            Node("cc", "Claude Code", 0, "#dbeafe", "#1e3a5f"),
            Node("pi", "pi-mono", 0, "#dbeafe", "#1e3a5f"),
            Node("jsonl", "JSONL", 0, "#dbeafe", "#1e3a5f"),
            Node("watch", "watcher", 1, "#ede9fe", "#5b21b6"),
            Node("tx", "translate", 1, "#ede9fe", "#5b21b6"),
            Node("nats", "NATS JetStream", 2, "#ccfbf1", "#0f766e"),
            Node("persist", "persist", 3, "#ffedd5", "#9a3412"),
            Node("pat", "patterns", 3, "#ffedd5", "#9a3412"),
            Node("bc", "broadcast", 3, "#ffedd5", "#9a3412"),
            Node("store", "EventStore", 4, "#dcfce7", "#166534"),
            Node("ui", "UI + pen eyes", 4, "#fce7f3", "#9d174d"),
        ],
        edges=[
            Edge("cc", "watch"),
            Edge("pi", "watch"),
            Edge("jsonl", "watch"),
            Edge("watch", "tx"),
            Edge("tx", "nats"),
            Edge("nats", "persist"),
            Edge("nats", "pat"),
            Edge("nats", "bc"),
            Edge("persist", "store"),
            Edge("bc", "ui"),
        ],
    )


def session_tool_graph(steps: list[str], title: str) -> Graph:
    """Linear story of tool names as a vertical flow (history → diagram hand)."""
    nodes = []
    edges = []
    colors = [
        ("#dbeafe", "#1e3a5f"),
        ("#ede9fe", "#5b21b6"),
        ("#ffedd5", "#9a3412"),
        ("#dcfce7", "#166534"),
        ("#fce7f3", "#9d174d"),
    ]
    for i, name in enumerate(steps[:8]):
        fill, stroke = colors[i % len(colors)]
        nid = f"s{i}"
        nodes.append(Node(nid, name[:32], i, fill, stroke))
        if i > 0:
            edges.append(Edge(f"s{i-1}", nid))
    return Graph(
        title=title[:50],
        subtitle="tool journey · collapsed · named tools only",
        nodes=nodes,
        edges=edges,
    )


# ── History → journey labels (declarative pure core) ───────────────────────


def _basename(path: str | None) -> str | None:
    if not path or not isinstance(path, str):
        return None
    p = path.strip()
    if not p:
        return None
    # strip query / trailing slash
    p = p.rstrip("/").split("?")[0]
    name = p.rsplit("/", 1)[-1]
    return name[:24] if name else None


def normalize_tool_journey(
    entries: list[dict[str, Any]] | list[Any],
    *,
    max_steps: int = 8,
) -> list[str]:
    """
    Pure: API tool-journey rows (or loose dicts) → pen labels.

    Contract:
    - Prefer named tools (`tool` / `name`); drop bare `tool_result` noise.
    - Optional file basename or short detail when present.
    - Collapse consecutive identical base tools into `Bash ×3`.
    - Cap at max_steps for a readable layered board.
    """
    raw_labels: list[str] = []
    for e in entries:
        if not isinstance(e, dict):
            continue
        tool = e.get("tool") or e.get("name") or e.get("tool_name")
        if not tool or not isinstance(tool, str):
            continue
        tool = tool.strip()
        if not tool or tool in ("tool_result", "tool_use", "unknown"):
            continue
        # strip mcp__ prefix noise for display
        display = tool
        if display.startswith("mcp__"):
            parts = display.split("__")
            display = parts[-1] if parts else display
        file_bit = _basename(e.get("file") if isinstance(e.get("file"), str) else None)
        detail = e.get("detail") or e.get("description")
        if isinstance(detail, str) and detail.strip() and not file_bit:
            # keep short
            d = detail.strip().replace("\n", " ")
            if len(d) > 22:
                d = d[:20] + "…"
            label = f"{display}: {d}"
        elif file_bit:
            label = f"{display} · {file_bit}"
        else:
            label = display
        raw_labels.append(label[:36])

    # Collapse consecutive same *base tool* (before colon/dot detail)
    def base(lab: str) -> str:
        return lab.split(" · ")[0].split(":")[0].strip()

    collapsed: list[str] = []
    i = 0
    while i < len(raw_labels):
        b = base(raw_labels[i])
        j = i + 1
        while j < len(raw_labels) and base(raw_labels[j]) == b:
            j += 1
        count = j - i
        # Prefer the most specific label in the run (with file/detail)
        best = max(raw_labels[i:j], key=len)
        if count > 1:
            # "Bash · foo ×3" or "Bash ×3"
            if " · " in best or ":" in best:
                collapsed.append(f"{best} ×{count}"[:36])
            else:
                collapsed.append(f"{b} ×{count}"[:36])
        else:
            collapsed.append(best)
        i = j

    return collapsed[:max_steps]


def journey_from_records(records: list[Any], *, max_steps: int = 8) -> list[str]:
    """Pure: ViewRecord-ish list → journey (tool_call only)."""
    entries: list[dict[str, Any]] = []
    for r in records:
        if not isinstance(r, dict):
            continue
        rt = r.get("record_type") or r.get("type") or ""
        payload = r.get("payload") if isinstance(r.get("payload"), dict) else {}
        if rt == "tool_call" or rt == "tool_use":
            name = payload.get("name") or payload.get("tool")
            file = None
            inp = payload.get("typed_input") or payload.get("input") or {}
            if isinstance(inp, dict):
                file = inp.get("file_path") or inp.get("path") or inp.get("file")
            entries.append({"tool": name, "file": file})
        elif rt == "tool_result":
            continue  # noise — named by the preceding call
    return normalize_tool_journey(entries, max_steps=max_steps)


# ── Mermaid / D2 emitters ──────────────────────────────────────────────────


def mermaid_arch() -> str:
    return """flowchart TB
  subgraph sources["Sources read-only"]
    CC[Claude Code]
    PI[pi-mono]
    JL[JSONL]
  end
  subgraph core["core"]
    W[watcher]
    T[translate]
  end
  NATS[NATS JetStream]
  subgraph actors["actors"]
    P[persist]
    PAT[patterns]
    B[broadcast]
  end
  STORE[(EventStore)]
  UI[UI + pen]
  CC --> W
  PI --> W
  JL --> W
  W --> T --> NATS
  NATS --> P --> STORE
  NATS --> PAT
  NATS --> B --> UI
"""


def d2_arch() -> str:
    return """direction: down
sources: {
  label: Sources
  claude: Claude Code
  pi: pi-mono
}
core: {
  watcher
  translate
}
bus: NATS JetStream
actors: {
  persist
  patterns
  broadcast
}
store: EventStore
ui: UI + pen eyes
sources.claude -> core.watcher
sources.pi -> core.watcher
core.watcher -> core.translate -> bus
bus -> actors.persist -> store
bus -> actors.patterns
bus -> actors.broadcast -> ui
"""


def mermaid_session_flow(steps: list[str], title: str) -> str:
    lines = ["flowchart LR", f"  %% {title}"]
    for i, s in enumerate(steps[:10]):
        safe = "".join(c if c.isalnum() else "_" for c in s)[:20] or f"step{i}"
        label = s.replace('"', "'")[:24]
        lines.append(f'  n{i}["{label}"]')
        if i:
            lines.append(f"  n{i-1} --> n{i}")
    return "\n".join(lines) + "\n"


# ── shell renderers ────────────────────────────────────────────────────────


def which(cmd: str) -> str | None:
    return shutil.which(cmd)


def run_mmdc(mmd: str, out_svg: Path) -> bool:
    mmdc = which("mmdc")
    if not mmdc:
        print("skip mermaid: mmdc not found", file=sys.stderr)
        return False
    out_svg.parent.mkdir(parents=True, exist_ok=True)
    src = out_svg.with_suffix(".mmd")
    src.write_text(mmd, encoding="utf-8")
    r = subprocess.run(
        [mmdc, "-i", str(src), "-o", str(out_svg), "-b", "transparent"],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        print(r.stderr or r.stdout, file=sys.stderr)
        return False
    print(f"mermaid → {out_svg} ({out_svg.stat().st_size} bytes)")
    return True


def run_d2(src: str, out_svg: Path) -> bool:
    d2 = which("d2")
    if not d2:
        print("skip d2: d2 not found", file=sys.stderr)
        return False
    out_svg.parent.mkdir(parents=True, exist_ok=True)
    d2src = out_svg.with_suffix(".d2")
    d2src.write_text(src, encoding="utf-8")
    r = subprocess.run([d2, str(d2src), str(out_svg)], capture_output=True, text=True)
    if r.returncode != 0:
        print(r.stderr or r.stdout, file=sys.stderr)
        return False
    print(f"d2 → {out_svg} ({out_svg.stat().st_size} bytes)")
    return True


def svg_to_data_url(path: Path) -> str:
    raw = path.read_bytes()
    b64 = base64.b64encode(raw).decode("ascii")
    return f"data:image/svg+xml;base64,{b64}"


# ── OpenStory control / API ────────────────────────────────────────────────


def http_json(method: str, url: str, body: dict | None = None) -> Any:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"} if body is not None else {},
    )
    with urllib.request.urlopen(req, timeout=15) as r:
        return json.loads(r.read().decode())


def push_pen(strokes: list[dict[str, Any]], label: str = "diagram-hands") -> dict:
    http_json(
        "POST",
        f"{API}/api/control",
        {"action": "open_view", "params": {"route": "#/draw"}, "issuer": "diagram-hands"},
    )
    return http_json(
        "POST",
        f"{API}/api/control",
        {
            "action": "draw",
            "params": {"clear": True, "label": label, "mode": "replace", "strokes": strokes},
            "issuer": "diagram-hands",
        },
    )


def push_pen_image(svg_path: Path, label: str) -> dict:
    href = svg_to_data_url(svg_path)
    # data URLs can be large — pen allows data:image up to 4000 chars in normalize!
    # Check length — if too long, skip with message
    if len(href) > 3900:
        print(
            f"warn: {svg_path.name} data URL is {len(href)} chars > pen limit ~4000; skipping image push",
            file=sys.stderr,
        )
        return {"ok": False, "error": "data_url_too_large"}
    strokes = [
        {
            "type": "image",
            "href": href,
            "x": 0.05,
            "y": 0.08,
            "w": 0.9,
            "h": 0.82,
            "opacity": 1,
        },
        {
            "type": "text",
            "x": 0.5,
            "y": 0.04,
            "text": label[:50],
            "fill": "#0f172a",
            "fontSize": 16,
        },
    ]
    return push_pen(strokes, label=label)


def fetch_tool_steps(session_id: str, limit: int = 8) -> list[str]:
    """
    History eyes → journey labels.
    Prefer GET /api/sessions/{id}/tool-journey (named tools);
    fall back to records with journey_from_records.
    """
    # 1) dedicated tool-journey API
    try:
        data = http_json("GET", f"{API}/api/sessions/{session_id}/tool-journey")
        rows = data if isinstance(data, list) else data.get("journey") or data.get("tools") or []
        if isinstance(rows, list) and rows:
            steps = normalize_tool_journey(rows, max_steps=limit)
            if steps:
                return steps
    except urllib.error.URLError as e:
        print(f"tool-journey api: {e}", file=sys.stderr)
    except Exception as e:
        print(f"tool-journey parse: {e}", file=sys.stderr)

    # 2) records fallback
    try:
        data = http_json("GET", f"{API}/api/sessions/{session_id}/records?limit=120")
    except urllib.error.URLError as e:
        print(f"records api error: {e}", file=sys.stderr)
        return []
    recs = data if isinstance(data, list) else data.get("records") or []
    return journey_from_records(recs, max_steps=limit)


# ── demos ──────────────────────────────────────────────────────────────────


def demo(push: bool) -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    print("=== diagram hands demo ===\n")

    # 1 pure layered → pen
    g = openstory_arch_graph()
    strokes = graph_to_pen_strokes(g)
    (OUT / "arch-layered.json").write_text(json.dumps(strokes, indent=2), encoding="utf-8")
    print(f"layered pen strokes: {len(strokes)} → {OUT / 'arch-layered.json'}")

    # 2 mermaid
    ok_m = run_mmdc(mermaid_arch(), OUT / "arch-mermaid.svg")
    # 3 d2
    ok_d = run_d2(d2_arch(), OUT / "arch-d2.svg")

    # 4 session story if API up
    steps: list[str] = []
    try:
        sessions = http_json("GET", f"{API}/api/sessions")
        ss = sessions if isinstance(sessions, list) else sessions.get("sessions") or []
        sid = None
        for s in ss:
            n = s.get("event_count") or 0
            if 30 <= n <= 500:
                sid = s.get("session_id") or s.get("id")
                break
        if not sid and ss:
            sid = ss[0].get("session_id") or ss[0].get("id")
        if sid:
            steps = fetch_tool_steps(str(sid))
            print(f"session {sid[:8]}… tool steps: {steps[:8]}")
            if steps:
                sg = session_tool_graph(steps, f"session {str(sid)[:8]}")
                sstrokes = graph_to_pen_strokes(sg)
                (OUT / "session-layered.json").write_text(
                    json.dumps(sstrokes, indent=2), encoding="utf-8"
                )
                run_mmdc(
                    mermaid_session_flow(steps, f"session {sid}"),
                    OUT / "session-mermaid.svg",
                )
    except Exception as e:
        print(f"(session demo skipped: {e})")

    print("\n--- library comparison ---")
    print(f"  pure layered : always · exact pen coords · pen-eyes readable labels")
    print(f"  mermaid      : {'ok' if ok_m else 'missing'} · auto layout · SVG file")
    print(f"  d2           : {'ok' if ok_d else 'missing'} · auto layout · SVG file")
    print(f"  pen image    : limited by data: URL size (~4k in normalizeStroke)")

    if push:
        print("\npushing pure layered architecture to pen…")
        res = push_pen(strokes, label="diagram-hands-layered")
        print("control:", res)
        # try mermaid image if small enough after minify — usually too big
        if ok_m:
            r2 = push_pen_image(OUT / "arch-mermaid.svg", "mermaid (if fits)")
            print("mermaid image push:", r2)

    print(f"\nartifacts in {OUT}")
    return 0


def run_tests() -> int:
    g = openstory_arch_graph()
    pos = layered_layout(g)
    assert "nats" in pos and "ui" in pos
    # layers increase downward
    assert pos["cc"][1] < pos["nats"][1] < pos["ui"][1]
    strokes = graph_to_pen_strokes(g)
    assert len(strokes) > 10
    assert any(s.get("type") == "text" and "OpenStory" in str(s.get("text")) for s in strokes)
    # boxes don't wildly overlap in x within same layer for 3 sources
    xs = sorted(pos[n][0] for n in ("cc", "pi", "jsonl"))
    assert xs[1] > xs[0]

    # normalize_tool_journey — no tool_result spam; collapse; files
    noisy = [
        {"tool": "tool_result"},
        {"tool": "Bash", "file": None},
        {"tool": "Bash"},
        {"tool": "Bash"},
        {"tool": "Edit", "file": "/x/y/auth.rs"},
        {"tool": "Read", "file": "/x/y/auth.rs"},
        {"tool": "mcp__openstory__session_story"},
    ]
    labels = normalize_tool_journey(noisy, max_steps=8)
    assert "tool_result" not in " ".join(labels), labels
    assert any("Bash" in L and "×" in L for L in labels), labels
    assert any("Edit" in L and "auth.rs" in L for L in labels), labels
    assert any("session_story" in L for L in labels), labels
    assert len(labels) <= 8

    recs = [
        {"record_type": "tool_result", "payload": {}},
        {
            "record_type": "tool_call",
            "payload": {"name": "Read", "typed_input": {"file_path": "src/a.ts"}},
        },
        {"record_type": "tool_call", "payload": {"name": "Read", "input": {"file_path": "src/a.ts"}}},
        {"record_type": "tool_call", "payload": {"name": "Bash"}},
    ]
    from_recs = journey_from_records(recs, max_steps=5)
    assert from_recs[0].startswith("Read"), from_recs
    assert "tool_result" not in from_recs
    print("diagram_hands tests ok")
    return 0


def main(argv: Iterable[str] | None = None) -> int:
    global API
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--demo", action="store_true", help="render mermaid/d2/layered demos")
    p.add_argument("--push", action="store_true", help="push layered diagram to agent pen")
    p.add_argument("--session", metavar="SID", help="tool journey for session → pen")
    p.add_argument("--test", action="store_true")
    p.add_argument("--api", default=API)
    args = p.parse_args(list(argv) if argv is not None else None)

    API = args.api.rstrip("/")

    if args.test:
        return run_tests()
    if args.session:
        steps = fetch_tool_steps(args.session)
        if not steps:
            print("no steps found", file=sys.stderr)
            return 1
        g = session_tool_graph(steps, f"session {args.session[:8]}")
        strokes = graph_to_pen_strokes(g)
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / f"session-{args.session[:8]}.json").write_text(
            json.dumps(strokes, indent=2), encoding="utf-8"
        )
        run_mmdc(mermaid_session_flow(steps, args.session), OUT / f"session-{args.session[:8]}.svg")
        if args.push:
            print(push_pen(strokes, label=f"session-{args.session[:8]}"))
        return 0
    if args.demo or args.push:
        return demo(push=args.push)
    p.print_help()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
