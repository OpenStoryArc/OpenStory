# Operating a node

How to read a node, roll a build onto it, and roll back, on each host
shape OpenStory runs on. Everything here reads the node's own signals:
`/api/health` with its verdict, `/metrics`, the log ring at `/api/logs`,
and the presence beat. Background and the full requirement list:
`docs/research/openstory-as-node/REQUIREMENTS.md`.

## Read the node first

```bash
python3 scripts/node_health_probe.py --api http://127.0.0.1:3002          # verdict + findings
python3 scripts/node_health_probe.py --api http://127.0.0.1:3002 --json   # one JSON object (the deploy gate reads this)
curl -s http://127.0.0.1:3002/api/health | python3 -m json.tool | head -40
curl -s 'http://127.0.0.1:3002/api/logs?level=WARN&limit=50'
curl -s http://127.0.0.1:3002/api/fleet/presence                          # who is alive, who is stale
```

The verdict is computed once, on the node (`node_health::verdict`), and
carried on `/api/health`, on every presence beat, on the header dot, and on
the MCP's `node_health` hand. `ok`, `warn`, `critical`; each finding has a
stable id (`bus_disconnected`, `leaf_down`, `stream_cap:events`,
`consumer_dead:persist`, `watcher_quiet:claude-code`, …) that a proposal can
cite as evidence.

## Roll a build through the gate

`scripts/deploy_gate.sh` refuses to roll while the running node is
critical, runs your roll command, waits for the node to serve again, reads
the verdict once more, and runs your rollback command when the new build is
critical. Exit codes: 0 rolled, 2 refused, 3 rolled back, 4 never served
(rolled back).

```bash
scripts/deploy_gate.sh --api http://127.0.0.1:3002 \
  --roll "<roll command for this host shape>" \
  --rollback "<rollback one-liner for this host shape>" \
  --startup-secs 300
```

Add `--dry-run` to print the steps without running them.

Before a build rolls anywhere, the boot-memory gate must be green: `just
test-container` runs `rs/tests/test_boot_memory_gate.rs`, which boots the
production image against a synthetic store inside a 512 MiB cgroup with a
NATS sidecar and asserts it serves with the read model bounded, no OOM kill,
and no `memory_pressure` finding (row B-10 in
`docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md`).

## Rollback per host shape

Each shape's rollback is one command once the previous build is at hand.
Which of these have been exercised end to end is stated honestly below.

### Homebrew (a laptop)

The tap is a git repository; installing the formula file from its previous
revision installs the previous build.

```bash
tap="$(brew --repo openstoryarc/openstory)"
git -C "$tap" log --oneline -5 -- Formula/openstory.rb          # pick the previous revision
brew services stop openstory && git -C "$tap" checkout <rev> -- Formula/openstory.rb && \
  brew reinstall "$tap/Formula/openstory.rb" && brew services run openstory && git -C "$tap" checkout HEAD -- Formula/openstory.rb
```

Roll command for the gate: `brew upgrade openstory && brew services restart openstory`.
Status: written, not yet exercised on a laptop.

### Docker Compose (a VPS, `docker-compose.prod.yml`)

Tag the running image before every build so the previous one is one tag
away, then rolling back is a retag and an `up`.

```bash
docker tag open-story:prod "open-story:prod-$(git rev-parse --short HEAD)"   # before building the new one
docker build -f Dockerfile.prod -t open-story:prod . && docker compose -f docker-compose.prod.yml up -d open-story
# rollback:
docker tag open-story:prod-<previous-sha> open-story:prod && docker compose -f docker-compose.prod.yml up -d open-story
```

Status: written, not yet exercised on the VPS.

### k3s (`deploy/k8s`)

The Deployment keeps its revision history; rolling back is one command.

```bash
kubectl -n <namespace> rollout undo deployment/openstory
kubectl -n <namespace> rollout status deployment/openstory --timeout=20m
```

Roll command for the gate: `kubectl -n <namespace> set image deployment/openstory server=ghcr.io/openstoryarc/openstory:<tag>`.
Smoke test of the whole shape, in an `os-loop-` namespace only:
`scripts/k3s_smoke.sh --context <ctx> --namespace os-loop-smoke`.
Status: manifests rendered and checked (`scripts/k8s_manifest_check.py`);
the a1 run is pending a kubeconfig.

## What a node keeps for you

| where | what |
|---|---|
| `{data_dir}/open-story.db` | the store (sessions, events, presence latest, patterns, turns, plans) |
| `{data_dir}/*.jsonl` | every session's events, append-only, grep-able |
| `{data_dir}/presence.jsonl` | one line per presence beat: time, host, sha, verdict |
| `{data_dir}/dora.json` | `scripts/dora.py --write data/dora.json`, read by the Admin tab |
| `{data_dir}/logs/`, `{data_dir}/nats/nats.log` | the server's and the managed NATS child's logs |

## The four DORA keys

```bash
python3 scripts/dora.py --days 30                 # table
python3 scripts/dora.py --write data/dora.json    # for the Admin tab (7, 30, 90 d)
```

Deployment frequency is distinct shas first seen in presence per day; lead
time is commit time to the first beat carrying that sha; change failure
rate is the share of shas with a critical beat in their first hour; time to
restore is a critical beat to the next ok beat on that host.
