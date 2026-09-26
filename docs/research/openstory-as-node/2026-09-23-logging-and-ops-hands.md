# Logging, presence, and ops hands: what a node needs to be operated by an agent

**Date:** 2026-09-23
**Status:** research spike, sequel to `2026-09-23-openstory-as-node.md`. Uses that note's vocabulary: tier 0 read, tier 1 derived state, tier 2 substance; `presence.*`, `ops.proposal.>`, `ops.command.>`.
**Question:** the owner asked how much of "logging plus an MCP that can handle restarts, or integrate with K3s" can be scripted, and whether an agentic MCP can sit inside the architecture as an OpenStory node.

Short answer: the node already exposes most of the tier 0 facts, but not as logs an agent can read, and not as a heartbeat any other node can hear. The probe built for this note found the local box in a critical state on its first run. Nothing in the process had said so.

---

## 1. Audit: what exists

### Logging

There is no logging framework. No crate in `rs/` depends on `tracing`, `log`, or `env_logger`; `grep` across every `Cargo.toml` returns nothing. All output is `eprintln!`. The one helper is `rs/server/src/logging.rs:21-24`: `log_event(category, message)` prints a local `HH:MM:SS`, an ANSI-coloured five-character category, and free text. No date, no level, no JSON, no request id, no session id field, no `RUST_LOG`, no file sink. The startup banner and every actor use it or raw `eprintln!` (38 sites in `rs/cli/src/main.rs`, 11 in `rs/server/src/state.rs`, 10 in `rs/src/watcher.rs`).

Where those lines land depends on the host. `brew services` writes them to `var/log/openstory.log` and `openstory.error.log` (`Formula/openstory.rb:51-52`). `just serve` leaves them in a terminal (`justfile:13`). Compose and `Dockerfile.prod` send them to the container's stdout, readable with `docker compose logs -f server`; `docs/deploy/hetzner.md:293-302` documents that path for the hub. The arena compose (`feat/arena-v1`, read with `git show`) uses `restart: unless-stopped` and the same stdout path. The managed NATS child is worse: `rs/cli/src/managed_nats.rs:98-99` sets its stdout and stderr to `Stdio::null()`, so a JetStream storage error on a brew or sandbox node goes nowhere. `docs/deploy/distributed.md:347` tells operators to "check NATS startup logs for storage errors", which on a managed node do not exist.

Consumer errors are mostly swallowed. `rs/server/src/consumers/persist.rs:191-201` and `:278` discard the results of plan save, JSONL append, FTS index, and session upsert with `let _ =`. The consumer loops in `rs/src/server/mod.rs:218-252`, `:268-334`, `:354-368`, `:383-450` log once on a failed subscribe and then, when `sub.receiver.recv()` returns `None`, the task returns silently. A dead consumer is invisible. Boot replay logs one line when it finishes (`rs/src/server/mod.rs:165-172`, "async replay complete in Ns") and nothing during; `boot_from_sqlite` at `rs/server/src/state.rs:441` prints the session count and then iterates in silence. The WebSocket fan-out does log `Lagged` (`rs/server/src/ws.rs:185`), which the backlog wants promoted to a frame.

### Metrics

`rs/server/src/metrics.rs` registers `events_ingested_total`, `events_deduped_total`, `patterns_detected_total`, `ws_messages_sent_total`, watcher raw and ignored notify counters, `watcher_publish_failures_total`, `watcher_last_event_timestamp_seconds`, `watcher_last_success_timestamp_seconds`, `sessions_active`, `sessions_total`, `ws_clients_connected`, and the projection cache gauges (`render_cache_metrics`, `:133`). `metrics_enabled` defaults to `false` (`rs/server/src/config.rs:404`) and is switched on by `--metrics` or `OPEN_STORY_METRICS` (`rs/cli/src/main.rs:116`). Nothing measures stream bytes, consumer lag, RSS, or replay progress. `just observe` (`justfile:424`) stacks `docker-compose.observe.yml` with Prometheus scraping `server:3002/metrics` and a `nats-exporter` on `:7777` (`observe/prometheus.yml`), plus two Grafana dashboards. Every file under `observe/` and the compose overlay carry a single commit: the initial commit of 2026-03-21. The last touch to `metrics.rs` was `29e1835` on 2026-07-11.

### Health

`GET /health` (`rs/server/src/router.rs:63`) returns `status` and `role`, and is the only probe any host shape uses: `docker-compose.prod.yml:109` opens a raw TCP socket and greps for `200 OK`. `GET /api/health` (`rs/server/src/api.rs:302`) returns backend, session count, `bus.connected`, projection freshness, and a watcher count. `bus.connected` cannot be false on a real bus: `NatsBus` never overrides `is_active`, so the trait default at `rs/bus/src/lib.rs:68` returns `true` forever, and only `NoopBus` says `false`. `GET /api/watchers` (`router.rs:114`) is the richest read: per-actor `last_event_at`, backfill, counters including `publish_failures`, and a recent ring. `GET /api/fleet` (`router.rs:122`) returns the person and principals, not fleet health. `GET /api/digests` (`router.rs:112`) exists for convergence and nothing calls it across nodes.

NATS monitoring is on. `nats.conf:5` sets `http_port: 8222` and the managed leaf config at `managed_nats.rs:152` does the same, so `/varz`, `/jsz`, `/leafz`, and `/connz` exist on every node, managed or not. `rs/server/src/admin.rs` already parses `/leafz` for the topology view. `/jsz?streams=true&config=true` returns each stream's `max_bytes`, `discard`, and live `bytes`, which is the number every open cap PR is arguing about.

### Prior art

PR #79 (merged 2026-06-12) is `docs/research/otel-comparison.md` plus `scripts/otel_vs_jsonl.py`. Its conclusion is that Claude Code's OTel stream is additive to the transcript, useful for timing and cost, and should be ingested with content gates off by default. It is about observing the agent, not the node. PR #46 (open since 2026-05-02, `feat/observe-otel-cross-stack`) adds an `otel-collector` service, a cross-stack Grafana dashboard, and a metric callsite refactor in `persist.rs`, `patterns.rs`, and `ingest.rs`. It has had no update since the day it opened and its Rust hunks conflict with the consumer decomposition that landed after it. `docs/research/node-and-network-health.md` designed `/api/health` and listed stream bytes, boot reports, ingest rate, and store degradation as "future fields"; only v1 shipped. `docs/BACKLOG.md:55-65` ("The mirror reports on itself") asks for exactly the missing fields and names the three cap PRs. `docs/BACKLOG.md:565` proposes a cron script that restarts the OpenClaw agent from OpenStory data, which is a tier 2 act on a watched agent and belongs behind a human credential, not a script.

### The failures, and the signal that would have fired

| Failure | Signal that would have fired | Tier | Exists today |
|---|---|---|---|
| Hub NATS OOM crash loop at 512 MB, fleet cut off | `presence` RSS of the `nats` process against the container limit; `varz.mem` is already there | 0 | number exists on `:8222`, nothing reads it |
| open-story crash-looping and re-flooding NATS | restart count and `boot_phase` in `presence`; publish rate per watcher | 0 | no restart counter, no boot phase |
| `deploy.sh` clobbering box-only fixes | config digest in `presence` differing between boots | 0 | none |
| events stream at the 1 GiB cap (PRs #95, #97, #98) | `jsz` stream bytes over 90 percent of `max_bytes` | 0 | number exists on `:8222`, `/api/health` does not read it |
| Full stack restart today, eleven minutes of replay with no progress | `boot_phase: replaying` with sessions done over total, and readiness false until done | 0 | one line at completion (`rs/src/server/mod.rs:169`) |
| OpenActor events dropped by the enum | translate rejection counter per `agent` value | 0 | `AgentPayload` at `rs/core/src/event_data.rs:217` still has five variants, no counter |

Every row is tier 0. None of the six needed a restart hand. They needed a fact the node could have emitted and did not.

---

## 2. Design

### Logging that an agent can read

Add `tracing` and `tracing-subscriber` to `open-story-server` and `open-story-cli`, initialised once in `main.rs`. A `log_format` config key and `OPEN_STORY_LOG_FORMAT` env select `text` (today's look) or `json` (the `tracing_subscriber::fmt().json()` layer). `RUST_LOG` drives `EnvFilter`, default `info`. Every line carries four fields: `actor` (one of `watcher`, `translate`, `persist`, `patterns`, `projections`, `broadcast`, `nats`, `boot`, `http`), `event` (a stable snake_case name such as `consumer.exited`, `replay.progress`, `publish.failed`), and where they exist `session_id` and `subject`. `log_event` becomes a shim over `tracing::info!` so the 40 existing callsites migrate in one commit. The consumer loops get a `consumer.exited` error line when `recv()` returns `None`, and the `let _ =` sites in `persist.rs` become `if let Err(e)` with `event = "persist.step_failed"`. The managed NATS child's stderr is piped and re-emitted as `actor=nats`.

The node keeps its own log ring: a `tracing` layer that pushes each structured line into a bounded `VecDeque` (default 2,000 lines, configurable) behind an `RwLock` in `AppState`. `GET /api/logs?since=<seq>&actor=&level=&limit=` serves it. The ring is the tier 0 `node_logs` hand's source. No vendor, no file rotation; the host's log driver keeps the long tail.

### Health as a fact the node emits

The node note proposed `presence.{host}.{principal}`. This note fixes the payload. A tokio timer (default every 30 s, plus on every boot phase change) publishes one CloudEvent with `type: io.arc.presence`:

```
boot:      { phase: starting|reconciling|replaying|serving, replay_done, replay_total, uptime_secs, restarts_seen }
watchers:  [ { actor, last_event_age_secs, publish_failures, events_emitted } ]
streams:   [ { name, bytes, max_bytes, pct, messages, discard } ]
consumers: [ { actor, alive, pending, last_batch_age_secs } ]
bus:       { connected, leaf_configured, leaf_connected, hub, slow_consumers }
process:   { rss_bytes, nats_rss_bytes, fds_open, store_bytes_on_disk }
config:    { digest }
```

`bus.connected` becomes real by asking the async-nats client for its connection state. `streams` comes from the JetStream context the bus already holds (`rs/bus/src/lib.rs:72` exposes it for "read-only stream-info queries"). `consumers.alive` comes from a `JoinHandle::is_finished` check on each actor task. The event is published interest-based like `changes`, so a solo node pays nothing. The hub and every peer subscribe to `presence.>`; the fleet tab and an ops agent read the same rows. The last N heartbeats stay in a ring next to the log ring, and `/api/health` returns the latest one.

### Ops hands, by tier

Tier 0, read, any host, on the MCP:

- `node_health`: the latest `presence` payload of this node.
- `node_logs { since, actor, level, limit }`: the log ring.
- `node_streams`: per-stream bytes, cap, percent, first and last timestamps.
- `fleet_presence`: the latest `presence` payload per `{host}.{principal}` seen on the bus, with age.

Tier 1, derived state, any host, author-stamped, idempotent, recorded on `ops.proposal.>` then `ops.command.>` by the node itself since the target is only its own read model:

- `node_reproject { session_ids? }`: rebuild projections from the store.
- `node_verify { peer }`: diff digests against a peer.
- `node_catch_up { peer, session_ids? }`: pull missing event ids from the peer.
- `node_prune { older_than_days }`: apply retention.

Each is rate-limited to one in flight and refuses while `boot.phase != serving`. Twice equals once, so a model running one is bounded.

Tier 2, substance, never on the MCP. The MCP may only file `ops.proposal.restart_consumer { actor, evidence: [presence ids] }`, `ops.proposal.resize_stream`, `ops.proposal.restart_nats`, `ops.proposal.restart_node`. A person reads the proposal and acts with an admin credential. What "restart" means depends on what died:

| Thing that dies | Detected by | Restart means | Who does it, by host shape |
|---|---|---|---|
| A consumer task | `consumers[].alive = false` | re-spawn the task with a fresh subscription; JetStream redelivers from the durable consumer position | in-process supervisor loop, gated by `ops.command.restart_consumer` from the CLI; same in every shape |
| Managed NATS child | `bus.connected = false` and the child pid gone | relaunch via `ensure_nats`, then reconnect the bus | the node, on the CLI's `ops.command.restart_nats`; in K3s the sidecar's own restart policy |
| The watcher thread | `watchers[].last_event_age` growing while files change | re-register the notify watcher and re-read from stored offsets | the node, `ops.command.restart_watcher` |
| The whole process | `/health` unreachable | supervisor restarts it; replay runs | `brew services` (`keep_alive true`), `restart: unless-stopped`, `systemd`, or the kubelet |

The node never restarts itself. The hub crash loop is the reason: the restarting process was the storm.

### K3s shape

One pod is one node is one principal. The pod runs the `open-story` container from `Dockerfile.prod` and a `nats` sidecar with the leaf config, sharing a network namespace so `nats://localhost:4222` and `:8222` work unchanged. The store is a PVC; JSONL lives on it too. `presence.*` and `memory.*` replicate through the leaf link to the hub, which is a separate in-cluster Deployment or the existing VPS. Logs go to stdout and the node's log driver, so `kubectl logs` and `node_logs` show the same lines. Probes: liveness `/health`, readiness `/api/health` with a 503 while `boot.phase != serving` or the leaf is configured but down, and a `startupProbe` with a large `failureThreshold` so an eleven-minute replay is not killed.

```yaml
apiVersion: apps/v1
kind: Deployment
metadata: { name: openstory-node, labels: { principal: maxs-air } }
spec:
  replicas: 1
  selector: { matchLabels: { app: openstory-node } }
  template:
    metadata: { labels: { app: openstory-node, principal: maxs-air } }
    spec:
      containers:
      - name: open-story
        image: ghcr.io/openstoryarc/open-story:0.4.0
        args: ["serve","--host","0.0.0.0","--port","3002","--data-dir","/data","--watch-dir","/watch","--static-dir","/static"]
        env:
        - { name: NATS_URL, value: "nats://localhost:4222" }
        - { name: OPEN_STORY_LOG_FORMAT, value: "json" }
        - { name: OPEN_STORY_HOST, valueFrom: { fieldRef: { fieldPath: spec.nodeName } } }
        ports: [{ containerPort: 3002 }]
        volumeMounts:
        - { name: store, mountPath: /data }
        - { name: transcripts, mountPath: /watch, readOnly: true }
        startupProbe: { httpGet: { path: /api/health, port: 3002 }, periodSeconds: 10, failureThreshold: 180 }
        livenessProbe: { httpGet: { path: /health, port: 3002 }, periodSeconds: 15 }
        readinessProbe: { httpGet: { path: /api/health, port: 3002 }, periodSeconds: 10 }
        resources: { limits: { memory: 4Gi } }
      - name: nats
        image: nats:2.14
        args: ["-c","/etc/nats/leaf.conf"]
        ports: [{ containerPort: 4222 }, { containerPort: 8222 }]
        volumeMounts:
        - { name: nats-conf, mountPath: /etc/nats }
        - { name: jetstream, mountPath: /js }
        resources: { limits: { memory: 1Gi } }
      volumes:
      - { name: store, persistentVolumeClaim: { claimName: openstory-store } }
      - { name: jetstream, persistentVolumeClaim: { claimName: openstory-js } }
      - { name: transcripts, hostPath: { path: /home/user/.claude/projects } }
      - { name: nats-conf, secret: { secretName: openstory-leaf-conf } }
---
# Optional: the ops agent. Talks to the node over REST and the MCP only.
# Holds no ServiceAccount token. Tier 0 and tier 1 only.
apiVersion: apps/v1
kind: Deployment
metadata: { name: openstory-ops-agent }
spec:
  replicas: 1
  selector: { matchLabels: { app: openstory-ops-agent } }
  template:
    metadata: { labels: { app: openstory-ops-agent } }
    spec:
      automountServiceAccountToken: false
      containers:
      - name: mcp
        image: ghcr.io/openstoryarc/open-story-mcp:0.4.0
        env: [{ name: OPENSTORY_API_URL, value: "http://openstory-node:3002" }]
```

Tier 2 in K3s is the kubelet plus, if wanted, a small Operator that subscribes to `presence.>` and deletes a pod when `consumers[].alive` stays false for N heartbeats. That Operator holds the human-held credential in the form of a scoped ServiceAccount; the ops agent pod never does. The two must not be the same pod.

### How much can be scripted

| Item | Script now | Needs Rust | Needs k8s | Estimate |
|---|---|---|---|---|
| Tier 0 probe from outside (`node_health_probe.py`) | yes | no | no | done, 1 day |
| Stream bytes, leaf, RSS in `/api/health` | no | yes, ~80 LOC in `api.rs` + bus stream info | no | 1 day |
| `bus.connected` that can be false | no | yes, `NatsBus::is_active` | no | 0.5 day |
| `tracing` JSON logs with `actor` and `event` | no | yes, init + shim + 40 callsites | no | 2 days |
| Log ring + `GET /api/logs` | no | yes | no | 1 day |
| `presence.*` heartbeat on a timer | no | yes, new consumer-side actor | no | 2 days |
| `consumers[].alive` and re-spawn supervisor | no | yes | no | 2 days |
| Replay progress in boot phase | no | yes, counter in `replay_boot_sessions` | no | 0.5 day |
| Translate rejection counter, `AgentPayload` tolerance | no | yes, experiment 2 | no | 1 day |
| `node_logs`, `node_streams`, `fleet_presence` MCP hands | no | yes, thin over REST | no | 1 day |
| Tier 1 hands on `ops.proposal` and `ops.command` | no | yes, `reproject` exists, three designed | no | 3 days |
| Managed NATS stderr capture | no | yes, ~20 LOC | no | 0.5 day |
| Config digest | partly (script can hash the file) | yes for the heartbeat field | no | 0.5 day |
| K3s manifests and probes | yes, YAML | no | yes, a cluster to test on | 1 day |
| Operator for tier 2 restart | no | no | yes | 3 days, optional |
| Dashboards refreshed to real metric names | yes, JSON | no | no | 0.5 day |

About 12 Rust days for a node that reports itself, one script day already spent, two days of YAML. Nothing here is an `events.*` publisher, so the soul invariant from the node note holds by construction.

---

## 3. Experiment 1: `scripts/node_health_probe.py`

The script reads the nine endpoints, folds them with pure functions, and prints a card or one JSON object. Every fetch is optional; a missing endpoint becomes a finding. It reads `nats_leaf_url` from `--config` and never echoes the token, only `host:port`. Percent of cap is floored, not rounded, so a stream at 99.996 percent prints 99.99 and never claims full. `--test` runs 32 assertions on captured shapes, including the real `/leafz` fixture from `rs/server/src/admin.rs:792` and the live `/jsz` numbers below. Exit code is 0, 1, or 2 for ok, warn, critical, so a cron or a `just` recipe can key on it.

### First run

Against the box at 2026-09-24T01:10:25Z, with `--config data/config.toml --data-dir data`:

```
OpenStory node health  2026-09-24T01:10:25Z   verdict: CRITICAL

  server   version=0.4.0 store=sqlite sessions=2447 bus_connected=True pid=96878 rss=562 MB
  nats     version=2.14.3 uptime=3h7m9s mem=0.72 GB conns=8 slow_consumers=6
  leaf     configured=True connected=False hub=debian-16gb-ash-1:7422
  fleet    person=You principals=1
  store    on_disk=12.22 GB

  streams
    local            0.0 MiB / 1024 MiB     0.00%  msgs=0
    ui               0.0 MiB / unlimited  -  msgs=0
    patterns        58.0 MiB / 256 MiB     22.65%  msgs=736
    changes          0.0 MiB / unlimited  -  msgs=0
    events        1023.9 MiB / 1024 MiB    99.99%  msgs=6,544

  watchers
    claude-code:/Users/maxglassie/.claude/projects               age=0s       publish_failures=10
    codex:/Users/maxglassie/.codex/sessions                      age=unknown  publish_failures=1
    grok:/Users/maxglassie/.grok/sessions                        age=35s      publish_failures=22

  findings
    [info    ] no /metrics on this node (HTTP 404); metrics_enabled is off
    [critical] stream events at 99.99% of cap (1,073,642,693 / 1,073,741,824 bytes, 6,544 msgs) (discard=old: oldest messages are being evicted, not refused)
    [warn    ] watcher claude-code:/Users/maxglassie/.claude/projects: 10 publish failures since boot
    [info    ] watcher codex:/Users/maxglassie/.codex/sessions: no last_event_at yet (never emitted since boot)
    [warn    ] watcher codex:/Users/maxglassie/.codex/sessions: 1 publish failures since boot
    [warn    ] watcher grok:/Users/maxglassie/.grok/sessions: 22 publish failures since boot
    [critical] nats_leaf_url is set (hub debian-16gb-ash-1:7422) but NATS /leafz reports 0 leaf connections: this node is solo; fleet will not see its sessions
    [warn    ] NATS reports 6 slow consumers since start
```

What the run says. The `events` stream is at the cap right now, three hours after today's restart; `first_seq` is 169683, so JetStream has already evicted everything before 21:27 UTC. `discard: old` means the wedge is silent loss of replay depth, not a refused publish, which is why the three cap PRs see different symptoms. `:8222` is open on this box because `just nats` uses `nats.conf`, but that NATS is standalone, so the leaf in `config.toml` is configured and not connected: this laptop has been solo for the whole session and `/api/health` says `bus.connected: true`. The 33 publish failures across watchers and the 6 slow consumers on the NATS side happened during the same window with no log line that names them. The server is a debug build at 562 MB RSS and NATS holds 720 MB.

What `/api/health` needs, with numbers from this machine: `streams[]` with `bytes` and `max_bytes` (1,073,642,693 of 1,073,741,824), `leaf.configured` and `leaf.connected` (true and false), `watchers[].publish_failures` and `last_event_age_secs`, `nats.slow_consumers` (6), `process.rss_bytes` (589 MB), and a `boot.phase`. Those are the heartbeat fields in section 2.

---

## Sources

**Files in this worktree (`feat/reel-chart-beats-kindle`)**

`rs/server/src/logging.rs`; `rs/server/src/metrics.rs`; `rs/server/src/config.rs`; `rs/server/src/router.rs`; `rs/server/src/api.rs`; `rs/server/src/state.rs`; `rs/server/src/ingest.rs`; `rs/server/src/ws.rs`; `rs/server/src/admin.rs`; `rs/server/src/catch_up.rs`; `rs/server/src/consumers/persist.rs`, `projections.rs`, `admin_broadcaster.rs`; `rs/src/server/mod.rs`; `rs/src/watcher.rs`; `rs/cli/src/main.rs`; `rs/cli/src/managed_nats.rs`; `rs/bus/src/lib.rs`; `rs/bus/src/noop_bus.rs`; `rs/bus/src/nats_bus.rs`; `rs/core/src/event_data.rs`; `nats.conf`; `justfile`; `docker-compose.yml`; `docker-compose.observe.yml`; `docker-compose.prod.yml`; `Dockerfile.prod`; `observe/prometheus.yml`; `observe/grafana/`; `Formula/openstory.rb`; `docs/deploy/distributed.md`; `docs/deploy/hetzner.md`; `docs/BACKLOG.md` (lines 55-65, 242-303, 313, 565); `docs/research/node-and-network-health.md`; `docs/research/state-management-interface.md`; `docs/research/otel-comparison.md`; `docs/research/openstory-as-node/2026-09-23-openstory-as-node.md`.

**Other branches, read with `git show`**

`feat/arena-v1`: `arena/deploy/docker-compose.yml`.

**Pull requests and commits**

PR #46 (open, 2026-05-02, `feat/observe-otel-cross-stack`); PR #79 (merged 2026-06-12); PRs #95, #97, #98 (open, stream caps); commits `feececd` (observe stack, 2026-03-21), `29e1835` (metrics cache gauges, 2026-07-11), `33c1609` (otel comparison, 2026-06-11), `019129e` (fd limit).

**Live reads on this box (2026-09-24T01:06 to 01:10 UTC)**

`GET :3002/health`, `/api/health`, `/api/watchers`, `/api/fleet`, `/metrics` (404), `/api/logs` (404); `GET :8222/varz`, `/jsz?streams=true&config=true`, `/leafz`, `/connz`; `ps` on pid 96878 (`rs/target/debug/open-story serve`) and 96731 (`nats-server -c nats.conf`); `data/config.toml` (token redacted).

**Sessions (OpenStory store, `GET /api/sessions` and `/api/search`)**

`01a0c7d2-5806-7600-80e2-b903af9941b2` (Grok, today, 3,927 events, the stack restart); search hits in `aa2de12e`, `d58d3d03`, `46c5653e` for the cold-boot replay cost; the node note's sessions `917baaad` and `3e73ef43` for the hub crash loop, cited there.

**Owner's notes (memory)**

Hub NATS 512 MB OOM crash loop; `deploy.sh` clobbering box-only fixes; OpenActor `agent = "openactor"` dropped; local store cold-boot cost.
