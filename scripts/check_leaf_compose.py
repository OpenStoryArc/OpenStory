#!/usr/bin/env python3
"""Check that docker-compose.leaf.yml can actually form a working leaf.

The leaf stack is two containers — `nats` (a JetStream leaf pointed at the
hub) and `open-story` (the server) — and three things have to be wired
across that container boundary for federation to work:

  1. NATS_LEAF_URL reaches the nats container. deploy/nats-leaf.conf reads
     `url: $NATS_LEAF_URL`; compose does not pass host env into a container
     unless the service lists it, so without this NATS can't resolve the
     remote.
  2. NATS listens beyond the nats container's loopback. nats-leaf.conf says
     `listen: 127.0.0.1:4222` (correct for a native Mac install), but inside
     a container that makes it unreachable from open-story at nats:4222.
  3. open-story discovers the hub via the NATS monitor at nats:8222, not its
     own localhost:8222 (the default), or /api/admin/topology never shows
     the `nats-leafnode-hub` node.

It also guards the leaf's promise that its ports stay host-local.

Usage:
    python3 scripts/check_leaf_compose.py           # check the real file
    python3 scripts/check_leaf_compose.py --test    # run self-tests
"""

import json
import os
import subprocess
import sys

COMPOSE_FILE = "docker-compose.leaf.yml"
# Placeholder only — the real URL (with its token) lives in a gitignored .env.
DUMMY_LEAF_URL = "nats://token@hub.example:7422"


def _env(service):
    """Compose normalizes `environment` to a dict; tolerate list form too."""
    env = service.get("environment") or {}
    if isinstance(env, list):
        env = dict(e.split("=", 1) if "=" in e else (e, None) for e in env)
    return env


def problems(cfg, leaf_url=DUMMY_LEAF_URL):
    """Pure: return a list of human-readable problems in a resolved config."""
    out = []
    services = cfg.get("services", {})
    nats = services.get("nats")
    server = services.get("open-story")
    if nats is None or server is None:
        return ["expected services `nats` and `open-story`"]

    if _env(nats).get("NATS_LEAF_URL") != leaf_url:
        out.append("nats: NATS_LEAF_URL is not passed into the container")

    cmd = nats.get("command") or []
    listens_wide = any(
        a in ("-a", "--addr", "--net") and i + 1 < len(cmd) and cmd[i + 1] == "0.0.0.0"
        for i, a in enumerate(cmd)
    )
    if not listens_wide:
        out.append("nats: listens on container loopback only (add `-a 0.0.0.0`)")

    if _env(server).get("OPEN_STORY_NATS_MONITOR_URL") != "http://nats:8222":
        out.append("open-story: OPEN_STORY_NATS_MONITOR_URL must be http://nats:8222")

    for name, svc in (("nats", nats), ("open-story", server)):
        for p in svc.get("ports") or []:
            if p.get("published") and p.get("host_ip") != "127.0.0.1":
                out.append(f"{name}: port {p.get('published')} is not bound to 127.0.0.1")
    return out


def resolved_config(path=COMPOSE_FILE, leaf_url=DUMMY_LEAF_URL):
    """Side effect at the edge: ask docker compose for the resolved config.

    Pins every env var the file interpolates so a local .env can't leak into
    (or mask problems in) the check.
    """
    env = {**os.environ, "NATS_LEAF_URL": leaf_url,
           "OPEN_STORY_HOST": "", "OPEN_STORY_USER": ""}
    res = subprocess.run(
        ["docker", "compose", "--env-file", os.devnull, "-f", path,
         "config", "--format", "json", "--no-path-resolution"],
        capture_output=True, text=True, env=env,
    )
    if res.returncode != 0:
        sys.exit(f"docker compose config failed:\n{res.stderr}")
    return json.loads(res.stdout)


def _run_tests():
    good = {"services": {
        "nats": {
            "command": ["-c", "/etc/nats/nats-leaf.conf", "-a", "0.0.0.0"],
            "environment": {"NATS_LEAF_URL": DUMMY_LEAF_URL},
            "ports": [{"host_ip": "127.0.0.1", "published": "4222", "target": 4222}],
        },
        "open-story": {
            "environment": {"OPEN_STORY_NATS_MONITOR_URL": "http://nats:8222"},
            "ports": [{"host_ip": "127.0.0.1", "published": "3002", "target": 3002}],
        },
    }}
    assert problems(good) == [], problems(good)

    # List-form environment is accepted too.
    listy = json.loads(json.dumps(good))
    listy["services"]["nats"]["environment"] = [f"NATS_LEAF_URL={DUMMY_LEAF_URL}"]
    assert problems(listy) == [], problems(listy)

    def broken(mutate):
        cfg = json.loads(json.dumps(good))
        mutate(cfg["services"])
        return problems(cfg)

    assert broken(lambda s: s["nats"].pop("environment")) == [
        "nats: NATS_LEAF_URL is not passed into the container"]
    assert broken(lambda s: s["nats"].update(command=["-c", "/etc/nats/nats-leaf.conf"])) == [
        "nats: listens on container loopback only (add `-a 0.0.0.0`)"]
    assert broken(lambda s: s["open-story"].pop("environment")) == [
        "open-story: OPEN_STORY_NATS_MONITOR_URL must be http://nats:8222"]
    assert broken(lambda s: s["open-story"]["ports"][0].update(host_ip="0.0.0.0")) == [
        "open-story: port 3002 is not bound to 127.0.0.1"]
    assert problems({"services": {}}) == ["expected services `nats` and `open-story`"]
    print("check_leaf_compose: 7 tests passed")


def main():
    if "--test" in sys.argv:
        _run_tests()
        return
    found = problems(resolved_config())
    if found:
        print(f"{COMPOSE_FILE}: {len(found)} problem(s)")
        for p in found:
            print(f"  ✗ {p}")
        sys.exit(1)
    print(f"{COMPOSE_FILE}: OK — leaf wiring is complete")


if __name__ == "__main__":
    main()
