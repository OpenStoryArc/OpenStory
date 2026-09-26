#!/usr/bin/env python3
"""Render the Kubernetes manifests and check the node shape (K-02 to K-06).

One pod is one node is one principal. The check renders an overlay with
`kubectl kustomize` and asserts:

- K-02 probes: liveness `GET /health`, readiness `GET /api/health`, a
  startupProbe with failureThreshold 180 and periodSeconds 5;
- K-03 the server does not manage NATS; `NATS_URL` points at the sidecar
  on the pod's loopback; the sidecar mounts `nats-leaf.conf` from a
  ConfigMap;
- K-04 logs are JSON (`OPEN_STORY_LOG_FORMAT=json`);
- K-05 no Deployment has `replicas` above 1; the two-principal overlay
  yields two Deployments with distinct names, PVCs, and `OPEN_STORY_HOST`;
- K-06 an ops-agent pod carries no service-account token.

    python3 scripts/k8s_manifest_check.py                 # deploy/k8s overlays
    python3 scripts/k8s_manifest_check.py --overlay deploy/k8s/overlays/a1
    python3 scripts/k8s_manifest_check.py --test          # fixtures, no kubectl
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parent.parent
ONE_NODE = REPO / "deploy" / "k8s" / "overlays" / "a1"
TWO_NODES = REPO / "deploy" / "k8s" / "overlays" / "a1-two-principals"


def render(overlay: Path) -> list[dict]:
    """The overlay's resources, via kubectl's kustomize."""
    out = subprocess.run(["kubectl", "kustomize", str(overlay)], capture_output=True, text=True, check=False)
    if out.returncode != 0:
        raise RuntimeError(f"kubectl kustomize {overlay}: {out.stderr.strip()}")
    return [d for d in yaml.safe_load_all(out.stdout) if d]


def deployments(docs: list[dict]) -> list[dict]:
    return [d for d in docs if d.get("kind") == "Deployment"]


def containers(dep: dict) -> list[dict]:
    return dep.get("spec", {}).get("template", {}).get("spec", {}).get("containers", []) or []


def container(dep: dict, name: str) -> dict | None:
    return next((c for c in containers(dep) if c.get("name") == name), None)


def env_of(c: dict) -> dict[str, str | None]:
    return {e.get("name"): e.get("value") for e in c.get("env", []) or []}


def is_node(dep: dict) -> bool:
    return container(dep, "server") is not None


# ── checks: (name, ok, detail) ───────────────────────────────────────────────

def check_probes(docs: list[dict]) -> list[tuple[str, bool, str]]:
    out = []
    for dep in filter(is_node, deployments(docs)):
        name = dep["metadata"]["name"]
        c = container(dep, "server") or {}
        live = (c.get("livenessProbe") or {}).get("httpGet", {}).get("path")
        ready = (c.get("readinessProbe") or {}).get("httpGet", {}).get("path")
        start = c.get("startupProbe") or {}
        ok = live == "/health" and ready == "/api/health" and start.get("failureThreshold") == 180 and start.get("periodSeconds") == 5
        out.append(("probes", ok, f"{name}: liveness={live} readiness={ready} startup={start.get('failureThreshold')}x{start.get('periodSeconds')}s"))
    return out


def check_sidecar(docs: list[dict]) -> list[tuple[str, bool, str]]:
    out = []
    for dep in filter(is_node, deployments(docs)):
        name = dep["metadata"]["name"]
        server = container(dep, "server") or {}
        args = " ".join((server.get("command") or []) + (server.get("args") or []))
        env = env_of(server)
        nats = container(dep, "nats") or {}
        mounts = nats.get("volumeMounts") or []
        conf_mounted = any(m.get("subPath") == "nats-leaf.conf" or str(m.get("mountPath", "")).endswith("nats-leaf.conf") for m in mounts)
        ok = "--manage-nats" not in args and env.get("NATS_URL") == "nats://127.0.0.1:4222" and bool(nats) and conf_mounted
        out.append(("sidecar", ok, f"{name}: manage-nats={'--manage-nats' in args} NATS_URL={env.get('NATS_URL')} nats-container={bool(nats)} leaf-conf-mounted={conf_mounted}"))
    return out


def check_json_logs(docs: list[dict]) -> list[tuple[str, bool, str]]:
    return [("json_logs", env_of(container(d, "server") or {}).get("OPEN_STORY_LOG_FORMAT") == "json", d["metadata"]["name"]) for d in filter(is_node, deployments(docs))]


def check_single_replica(docs: list[dict]) -> list[tuple[str, bool, str]]:
    out = []
    for dep in deployments(docs):
        replicas = dep.get("spec", {}).get("replicas", 1)
        out.append(("single_replica", replicas <= 1, f"{dep['metadata']['name']}: replicas={replicas} (a node scales by principal, never by replica)"))
    return out


def check_ops_agent(docs: list[dict]) -> list[tuple[str, bool, str]]:
    out = []
    for dep in deployments(docs):
        if "ops-agent" not in dep["metadata"]["name"]:
            continue
        pod = dep.get("spec", {}).get("template", {}).get("spec", {})
        ok = pod.get("automountServiceAccountToken") is False and not pod.get("serviceAccountName")
        out.append(("ops_agent_no_token", ok, f"{dep['metadata']['name']}: automountServiceAccountToken={pod.get('automountServiceAccountToken')} serviceAccountName={pod.get('serviceAccountName')}"))
    return out


def check_two_principals(docs: list[dict]) -> list[tuple[str, bool, str]]:
    nodes = [d for d in deployments(docs) if is_node(d)]
    names = [d["metadata"]["name"] for d in nodes]
    hosts = [env_of(container(d, "server") or {}).get("OPEN_STORY_HOST") for d in nodes]
    claims = []
    for d in nodes:
        vols = d.get("spec", {}).get("template", {}).get("spec", {}).get("volumes", []) or []
        claims.append(tuple(sorted(v.get("persistentVolumeClaim", {}).get("claimName", "") for v in vols if "persistentVolumeClaim" in v)))
    ok = len(nodes) == 2 and len(set(names)) == 2 and len(set(hosts)) == 2 and None not in hosts and len(set(claims)) == 2 and not set(claims[0]) & set(claims[1]) if len(claims) == 2 else False
    return [("two_principals", ok, f"nodes={names} hosts={hosts} claims={claims}")]


def one_node_checks(docs: list[dict]) -> list[tuple[str, bool, str]]:
    results = check_probes(docs) + check_sidecar(docs) + check_json_logs(docs) + check_single_replica(docs) + check_ops_agent(docs)
    if not any(is_node(d) for d in deployments(docs)):
        results.append(("has_a_node", False, "no Deployment with a `server` container"))
    return results


def report(label: str, results: list[tuple[str, bool, str]]) -> bool:
    print(f"== {label}")
    for name, ok, detail in results:
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {detail}")
    return all(ok for _, ok, _ in results)


# ── self-tests ────────────────────────────────────────────────────────────────

def _node(name: str, host: str, claims: tuple[str, str], **over) -> dict:
    server = {
        "name": "server",
        "command": ["open-story", "serve", "--host", "0.0.0.0", "--port", "3002"],
        "env": [{"name": "NATS_URL", "value": "nats://127.0.0.1:4222"}, {"name": "OPEN_STORY_LOG_FORMAT", "value": "json"}, {"name": "OPEN_STORY_HOST", "value": host}],
        "livenessProbe": {"httpGet": {"path": "/health", "port": 3002}},
        "readinessProbe": {"httpGet": {"path": "/api/health", "port": 3002}},
        "startupProbe": {"httpGet": {"path": "/health", "port": 3002}, "failureThreshold": 180, "periodSeconds": 5},
    }
    nats = {"name": "nats", "volumeMounts": [{"name": "nats-conf", "mountPath": "/etc/nats/nats-leaf.conf", "subPath": "nats-leaf.conf"}]}
    dep = {"kind": "Deployment", "metadata": {"name": name}, "spec": {"replicas": over.get("replicas", 1), "template": {"spec": {
        "containers": [server, nats],
        "volumes": [{"name": "store", "persistentVolumeClaim": {"claimName": claims[0]}}, {"name": "jetstream", "persistentVolumeClaim": {"claimName": claims[1]}}],
    }}}}
    if over.get("manage_nats"):
        server["command"].append("--manage-nats")
    if over.get("no_startup"):
        del server["startupProbe"]
    return dep


def _test() -> int:
    good = [_node("openstory", "node-a", ("store", "jetstream")), {"kind": "Deployment", "metadata": {"name": "openstory-ops-agent"}, "spec": {"template": {"spec": {"automountServiceAccountToken": False, "containers": [{"name": "mcp"}]}}}}]
    assert all(ok for _, ok, _ in one_node_checks(good)), one_node_checks(good)
    bad_replicas = [_node("openstory", "a", ("s", "j"), replicas=2)]
    assert [r[1] for r in check_single_replica(bad_replicas)] == [False]
    assert [r[1] for r in check_probes([_node("x", "a", ("s", "j"), no_startup=True)])] == [False]
    assert [r[1] for r in check_sidecar([_node("x", "a", ("s", "j"), manage_nats=True)])] == [False]
    token_pod = [{"kind": "Deployment", "metadata": {"name": "ops-agent"}, "spec": {"template": {"spec": {"automountServiceAccountToken": True, "containers": []}}}}]
    assert [r[1] for r in check_ops_agent(token_pod)] == [False]
    two = [_node("dev-openstory", "dev-node", ("dev-store", "dev-jetstream")), _node("hub-openstory", "hub-node", ("hub-store", "hub-jetstream"))]
    assert check_two_principals(two)[0][1], check_two_principals(two)
    same_host = [_node("a", "same", ("s1", "j1")), _node("b", "same", ("s2", "j2"))]
    assert not check_two_principals(same_host)[0][1]
    shared_claim = [_node("a", "h1", ("s", "j1")), _node("b", "h2", ("s", "j2"))]
    assert not check_two_principals(shared_claim)[0][1]
    assert any(n == "has_a_node" and not ok for n, ok, _ in one_node_checks([]))
    print("ok: 9 assertions")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--overlay", type=Path, default=ONE_NODE, help="the one-node overlay to render")
    ap.add_argument("--two", type=Path, default=TWO_NODES, help="the two-principal overlay to render")
    ap.add_argument("--test", action="store_true")
    a = ap.parse_args(argv)
    if a.test:
        return _test()
    if not shutil.which("kubectl"):
        print("SKIP: kubectl not found; the manifests were not rendered (CI renders them)")
        return 0
    ok = True
    for label, overlay, checks in [("one node", a.overlay, one_node_checks), ("two principals", a.two, lambda d: check_single_replica(d) + check_two_principals(d))]:
        try:
            docs = render(overlay)
        except (RuntimeError, FileNotFoundError) as e:
            print(f"== {label}\n  FAIL render: {e}")
            ok = False
            continue
        ok = report(label, checks(docs)) and ok
    print("all manifest checks passed" if ok else "manifest checks failed")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
