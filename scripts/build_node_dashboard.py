#!/usr/bin/env python3
"""Emit the "Node" Grafana dashboard (O-04) from one panel list.

The dashboard is the operator's view of a single OpenStory node: the O-01
gauges from `/metrics` (events ingested by agent, consumer lag, restarts,
alive, stream bytes against caps, publish failures by watcher, presence
beats) plus the pipeline panels the March dashboard still had right.
Generated so the layout is one edit away and never drifts by hand;
`scripts/check_docs.py` verifies every metric it queries exists.

    python3 scripts/build_node_dashboard.py            # write observe/grafana/dashboards/node.json
    python3 scripts/build_node_dashboard.py --check    # exit 1 if the file on disk differs
    python3 scripts/build_node_dashboard.py --test     # self-tests
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "observe" / "grafana" / "dashboards" / "node.json"
W = 24  # grid width


def panel(title: str, kind: str, exprs: list[tuple[str, str]], *, w: int, h: int, unit: str | None = None,
          max_value: float | None = None, description: str = "") -> dict:
    """One panel; `exprs` are (expr, legend) pairs. Position is filled by `layout`."""
    defaults: dict = {}
    if unit:
        defaults["unit"] = unit
    if max_value is not None:
        defaults["max"] = max_value
        defaults["min"] = 0
    return {
        "title": title,
        "type": kind,
        "description": description,
        "gridPos": {"h": h, "w": w, "x": 0, "y": 0},
        "targets": [{"expr": e, "legendFormat": l, "refId": chr(65 + i)} for i, (e, l) in enumerate(exprs)],
        "fieldConfig": {"defaults": defaults, "overrides": []},
        "options": {"legend": {"displayMode": "list", "placement": "bottom"}} if kind == "timeseries" else {},
    }


def panels() -> list[dict]:
    """The Node dashboard, top to bottom: is it alive, is it keeping up, is
    it full, is it talking, what is it doing."""
    return [
        # ── row 1: alive
        panel("Consumers alive", "stat", [("openstory_consumer_alive", "{{actor}}")], w=8, h=4,
              description="1 while the supervised consumer task runs; 0 means the supervisor is restarting it."),
        panel("Presence beats", "stat", [("rate(openstory_presence_beats_total[5m]) * 60", "beats/min")], w=8, h=4,
              unit="short", description="Beats per minute; four at the default 15 s interval."),
        panel("Publish failures (1h)", "stat", [("sum by (watcher) (increase(openstory_publish_failures_total[1h]))", "{{watcher}}")],
              w=8, h=4, unit="short", description="Publishes to the bus that failed in the last hour, by watcher; presence is one."),
        # ── row 2: keeping up
        panel("Events ingested by agent", "timeseries",
              [("sum by (agent) (rate(openstory_events_ingested_total[5m]))", "{{agent}}")], w=12, h=8, unit="ops",
              description="Events that landed in the store per second, by the agent that produced them."),
        panel("Consumer lag", "timeseries", [("openstory_consumer_lag", "{{actor}}")], w=12, h=8, unit="short",
              description="Batches waiting in each consumer's channel at its last receive."),
        # ── row 3: full
        panel("Stream fill", "timeseries",
              [("openstory_stream_bytes / openstory_stream_max_bytes", "{{stream}}")], w=12, h=8, unit="percentunit",
              max_value=1.0, description="Bytes held against the stream's cap; eviction starts at 1."),
        panel("Stream bytes", "timeseries", [("openstory_stream_bytes", "{{stream}}")], w=12, h=8, unit="bytes",
              description="Bytes held by each JetStream stream."),
        # ── row 4: restarts and failures over time
        panel("Consumer restarts (1h)", "timeseries",
              [("increase(openstory_consumer_restarts_total[1h])", "{{actor}}")], w=12, h=8, unit="short",
              description="Supervisor restarts per consumer over the trailing hour."),
        panel("Publish failures by watcher", "timeseries",
              [("sum by (watcher) (rate(openstory_publish_failures_total[5m]))", "{{watcher}}")], w=12, h=8, unit="ops",
              description="Failed publishes per second; the E-05 log line has the subject and the cause."),
        # ── row 5: the pipeline panels kept from the March dashboard
        panel("Dedup rate", "timeseries", [("rate(events_deduped_total[5m])", "deduped/s")], w=8, h=6, unit="ops"),
        panel("Patterns detected", "timeseries", [("rate(patterns_detected_total[5m])", "patterns/s")], w=8, h=6, unit="ops"),
        panel("WebSocket messages sent", "timeseries", [("rate(ws_messages_sent_total[5m])", "msgs/s")], w=8, h=6, unit="ops"),
        # ── row 6: counts and caches
        panel("Active sessions", "stat", [("sessions_active", "active")], w=6, h=4, unit="short"),
        panel("Total sessions", "stat", [("sessions_total", "total")], w=6, h=4, unit="short"),
        panel("WebSocket clients", "stat", [("ws_clients_connected", "clients")], w=6, h=4, unit="short"),
        panel("Cache bytes", "stat",
              [("openstory_projection_cache_bytes", "projections"), ("openstory_payload_cache_bytes", "payloads")],
              w=6, h=4, unit="bytes"),
    ]


def layout(items: list[dict]) -> list[dict]:
    """Flow panels left to right, wrapping at the grid width; rows are as
    tall as their tallest panel. Pure."""
    x = y = row_h = 0
    out = []
    for p in items:
        w, h = p["gridPos"]["w"], p["gridPos"]["h"]
        if x + w > W:
            x, y, row_h = 0, y + row_h, 0
        q = json.loads(json.dumps(p))
        q["gridPos"].update({"x": x, "y": y})
        q["id"] = len(out) + 1
        out.append(q)
        x += w
        row_h = max(row_h, h)
    return out


def dashboard() -> dict:
    return {
        "annotations": {"list": []},
        "editable": True,
        "fiscalYearStartMonth": 0,
        "graphTooltip": 1,
        "id": None,
        "links": [],
        "panels": layout(panels()),
        "refresh": "15s",
        "schemaVersion": 39,
        "tags": ["open-story", "node"],
        "templating": {"list": []},
        "time": {"from": "now-1h", "to": "now"},
        "title": "Node",
        "uid": "openstory-node",
        "description": "One OpenStory node: alive, keeping up, full, talking. Every panel queries /metrics (O-01).",
    }


def render() -> str:
    return json.dumps(dashboard(), indent=2) + "\n"


def _test() -> int:
    ps = layout(panels())
    assert all(p["gridPos"]["x"] + p["gridPos"]["w"] <= W for p in ps), "no panel spills past the grid"
    ys = [p["gridPos"]["y"] for p in ps]
    assert ys == sorted(ys), "rows flow downward"
    assert len({p["id"] for p in ps}) == len(ps), "ids unique"
    exprs = " ".join(t["expr"] for p in ps for t in p["targets"])
    for g in ("openstory_events_ingested_total", "openstory_consumer_lag", "openstory_consumer_restarts_total",
              "openstory_stream_bytes", "openstory_publish_failures_total"):
        assert g in exprs, f"node dashboard queries {g}"
    assert dashboard()["title"] == "Node"
    assert "hooks_received_total" not in exprs, "the retired metric is gone"
    # Layout check: the first three stats share a row, the next two panels start the second row.
    assert [p["gridPos"]["y"] for p in ps[:5]] == [0, 0, 0, 4, 4], [p["gridPos"] for p in ps[:5]]
    print("ok: 8 assertions")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="exit 1 if node.json differs from the generator")
    ap.add_argument("--test", action="store_true")
    a = ap.parse_args(argv)
    if a.test:
        return _test()
    text = render()
    if a.check:
        on_disk = OUT.read_text() if OUT.exists() else ""
        if on_disk != text:
            print(f"{OUT} differs from the generator; run scripts/build_node_dashboard.py", file=sys.stderr)
            return 1
        print(f"ok: {OUT.name} matches the generator")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text)
    print(f"wrote {OUT} ({len(dashboard()['panels'])} panels)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
