#!/usr/bin/env python3
"""Post a reel spec to the OpenStory REST API, resolving image beats from a manifest.

The spec is the reel JSON as the wire format expects it (camelCase), except that
an image stop may say {"kind": "image", "figure": "<name>", ...} and the name is
resolved to a data URL from a manifest produced by scripts/arc_figures.py
(manifest.json maps figure name -> data URL). Spotlight stops are validated by
the server against the event store; invented ids come back as 422 invalid_stops.

    python3 scripts/post_reel.py --spec reel.json --manifest out/manifest.json
    python3 scripts/post_reel.py --test
"""
from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path


def resolve(spec: dict, manifest: dict[str, str]) -> dict:
    """Return a copy of spec with every {"figure": name} image stop turned into visual.imageHref."""
    out = dict(spec)
    stops = []
    for s in spec["stops"]:
        s = dict(s)
        name = s.pop("figure", None)
        if name is not None:
            if name not in manifest:
                raise KeyError(f"figure {name!r} not in manifest ({sorted(manifest)})")
            visual = dict(s.get("visual") or {})
            visual["kind"] = "image"
            visual["imageHref"] = manifest[name]
            s["kind"] = "image"
            s["visual"] = visual
        stops.append(s)
    out["stops"] = stops
    return out


def post(api: str, reel: dict) -> dict:
    req = urllib.request.Request(f"{api}/api/reels", data=json.dumps(reel).encode(),
                                 headers={"content-type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")
        raise SystemExit(f"HTTP {e.code}: {body[:2000]}")


def _test() -> int:
    spec = {"title": "t", "stops": [
        {"sessionId": "s", "eventId": "e", "line": "a"},
        {"figure": "fig1", "line": "b", "visual": {"title": "T"}},
    ]}
    r = resolve(spec, {"fig1": "data:image/png;base64,AAA"})
    assert r["stops"][0] == spec["stops"][0]
    assert r["stops"][1]["kind"] == "image"
    assert r["stops"][1]["visual"] == {"title": "T", "kind": "image", "imageHref": "data:image/png;base64,AAA"}
    assert "figure" not in r["stops"][1]
    assert spec["stops"][1].get("figure") == "fig1", "input must not be mutated"
    try:
        resolve(spec, {})
    except KeyError:
        pass
    else:
        raise AssertionError("missing figure must raise")
    print("ok: 6 assertions")
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--spec")
    p.add_argument("--manifest")
    p.add_argument("--api", default="http://localhost:3002")
    p.add_argument("--test", action="store_true")
    a = p.parse_args(argv)
    if a.test:
        return _test()
    if not a.spec:
        p.error("--spec is required")
    spec = json.loads(Path(a.spec).read_text())
    manifest = json.loads(Path(a.manifest).read_text()) if a.manifest else {}
    result = post(a.api, resolve(spec, manifest))
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
