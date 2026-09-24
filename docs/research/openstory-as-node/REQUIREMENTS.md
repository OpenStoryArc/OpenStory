# Node ops: requirements scoreboard

Status: living document. The loop in `docs/prompts/node-ops-loop.md` works this
file top to bottom: pick the first row not GREEN, write its failing test, make
it pass, refactor, flip its status, commit, repeat.
Design: `docs/superpowers/specs/2026-09-23-node-ops-design.md`. Audit and
vocabulary: `2026-09-23-openstory-as-node.md` and
`2026-09-23-logging-and-ops-hands.md` in this directory.
Last updated: 2026-09-24 (H-01 green).

Vocabulary: **tier 0** reads; **tier 1** derived state, idempotent, author
stamped, on `ops.>`; **tier 2** substance (restart, resize, rotate), never on
the MCP, executed by a human credential or the host supervisor. A **node** is
one OpenStory instance: one store, one principal, one NATS leaf. **Presence** is
the node's periodic health fact, an observed CloudEvent on `presence.>`.

Status vocabulary: `TODO` (no test yet), `RED` (test written, failing), `GREEN`
(test passing), `DEFERRED` (out of scope for now, kept for the record).

## Scoreboard

| Group | Requirements | GREEN | Notes |
|---|---|---|---|
| G · global constraints | G-01 … G-08 | 0 / 8 | enforced by every task |
| L · logging | L-01 … L-08 | 8 / 8 | `tracing`, JSON lines, log ring |
| E · errors and supervision | E-01 … E-07 | 7 / 7 | no swallowed errors, consumer supervisor |
| H · health | H-01 … H-08 | 1 / 8 | `/api/health` can say no |
| P · presence | P-01 … P-06 | 0 / 6 | the node's health as a fact on the bus |
| O · telemetry | O-01 … O-05 | 0 / 5 | OTel metrics and spans, exported not vendored |
| M · ops hands on the MCP | M-01 … M-09 | 0 / 9 | tier 0 and tier 1 only |
| K · Kubernetes shape | K-01 … K-08 | 0 / 8 | K3s on a1, probes, PVCs, wedge test |
| D · DORA | D-01 … D-06 | 0 / 6 | the four keys measured from the node's own record |

## Loop protocol

1. Take the first row whose status is `TODO` or `RED`, in scoreboard order (L
   before E before H; P needs H-01; M needs H and L; K needs H; D needs P and H).
2. Write the acceptance test named in the row, exactly. Run it. It must fail
   for the right reason.
3. Write the minimal code. Run it. It must pass. Run the group's full suite and
   `cargo clippy -p <crate> --all-targets -- -D warnings` for the touched crate.
4. Flip the status, update the scoreboard counts, commit with the requirement
   id in the first line (`feat(server): H-03 /api/health returns 503 during
   replay`). Push.
5. If a requirement turns out wrong, edit it here in the same commit and say
   why in the body.

## G · Global constraints

| id | requirement | how it is checked |
|---|---|---|
| G-01 | Nothing in this work writes to `events.*` or `local.*`. The only publishers of history remain the translators. | `scripts/subject_publishers.py` static audit (K-07) stays green; grep for `publish(` outside translate in review |
| G-02 | No MCP hand performs a tier 2 action. Restarting a consumer, the NATS child, the watcher, or the process is never reachable from `rs/mcp`. | M-08 test: every `ops.command.*` subject the MCP can publish is in the tier 1 allowlist |
| G-03 | All work happens in the worktree on branch `feat/reel-chart-beats-kindle` or a branch stacked on it. The main checkout `~/projects/OpenStory` is never edited. Nothing merges without the owner. | reviewer gate on each commit |
| G-04 | The owner's live instance on `:3002` is never restarted by the loop. Live tests run an isolated instance (scratch port, scratch data dir) or a testcontainer. | `scripts/scratch_node.sh` (L-08) is the only way tests boot a server |
| G-05 | No real session data is committed. Fixtures are synthetic or captured shapes with content replaced. | pre-commit grep for any id in `memory/hands/real/ids.txt` of the research repo; reviewer gate |
| G-06 | Tests read as behaviour: Rust `mod when_<condition> { fn it_<outcome> }`, TypeScript `describe("when …") / it("should …")`, Python `test_when_<condition>_it_<outcome>`. Every test asserts values, not presence. | reviewer gate |
| G-07 | No new logging or metrics vendor. `tracing`, `tracing-subscriber`, `opentelemetry` crates only; export is OTLP or Prometheus text. | `Cargo.toml` diff review |
| G-08 | a1 is for experiments only: k3s namespaces prefixed `os-loop-`, torn down at the end of each task; never the hub, never the owner's a1 services. | K-01 teardown test; reviewer gate |

## L · Logging

| id | requirement | acceptance test |
|---|---|---|
| L-01 | GREEN. The server initialises `tracing_subscriber` with an `EnvFilter` from `RUST_LOG` (default `info`) and a `log_format` config/env of `text` (default) or `json`. | `rs/server/tests/test_logging.rs::when_log_format_is_json::it_emits_one_json_object_per_line` |
| L-02 | GREEN. Every log line carries `ts` (RFC 3339), `level`, `target`, `event` (a stable snake_case name), and `actor` when emitted inside a consumer. | `…::when_a_consumer_logs::it_stamps_actor_and_event` |
| L-03 | GREEN. Lines about a session carry `session_id`. (The `subject` half moves to E-05: `IngestBatch` carries no subject today; the watcher publish path is where a subject is known.) | `…::when_persist_logs_a_session::it_carries_session_id` |
| L-04 | GREEN. `log_event` is a thin shim over `tracing::info!(category, …)`, so every existing site flows through both formatters; the `TextLine` formatter keeps `HH:MM:SS  category  message` with fields after. Call sites gain named `event`s as the rows that touch them land (L-07, E-01, E-05). | `…::when_log_format_is_text::it_keeps_time_category_message_shape` |
| L-05 | GREEN. The managed NATS child's stdout and stderr are captured to `<store_dir>/nats.log` (rotated at 50 MB), never `Stdio::null()`. | `rs/cli/src/managed_nats.rs::tests::when_child_writes_stderr::it_lands_in_nats_log` |
| L-06 | GREEN. An in-process log ring keeps the last 5,000 lines (bounded by bytes, 8 MB) and is served at `GET /api/logs?since=<seq>&actor=&level=&limit=`. | `rs/tests/test_logs_api.rs::when_logs_are_requested_since_seq::it_returns_only_newer_lines` |
| L-07 | GREEN. Boot replay logs progress every 5 s and every 10 % (`event=replay_progress`, sessions done / total, elapsed) and a final `replay_done`. | `…::when_replay_runs::it_logs_progress_and_done` |
| L-08 | GREEN. `scripts/scratch_node.sh` boots an isolated instance (port, data dir, managed loopback NATS on a scratch port, `log_format=json`) and prints its log path; `--stop` tears it down. | script `--test` on a dry run; used by every live test below |

## E · Errors and supervision

| id | requirement | acceptance test |
|---|---|---|
| E-01 | GREEN. No `let _ =` on a fallible persist, index, append, or publish in `rs/server/src/consumers/` and `rs/src/server/`. Each failure logs `event=<op>_failed` with the error and increments a counter. | (Gate roots: `rs/server/src` and `rs/src/server`; the MCP's stdio response sends are M-08 / K-07 territory.) `scripts/swallowed_errors.py` static audit, `--test`, wired into `just test` |
| E-02 | GREEN. A consumer whose subscription ends logs `event=consumer_ended` with the reason and exits with an error, never silently. | `rs/tests/test_consumer_supervision.rs::when_subscription_ends::it_logs_and_errors` |
| E-03 | GREEN. A supervisor task owns the four consumers; on exit it restarts the consumer with exponential backoff (1 s, 2 s, 4 s, cap 30 s) and logs `event=consumer_restarted` with the attempt. | `…::when_a_consumer_dies::it_is_restarted_with_backoff` |
| E-04 | GREEN. Restart counts and last-restart timestamps per consumer are part of health (H-05). | `…::when_a_consumer_restarts::it_shows_in_health` |
| E-05 | GREEN. Watcher publish failures are logged per file with `event=publish_failed`, the subject, and the error; the count is part of health. Root-cause the 15 Grok failures seen on 2026-09-23 as part of this task. | Root cause: NATS `max_payload` 8 MB; Grok transcripts carry 0.5 to 1 MB lines, so a hundred-event batch reached 5 to 10 MB and the client refused it before sending. Fix: the bus splits any batch over 4 MB in order (`split_batch`). | `rs/tests/test_watcher_publish.rs::when_publish_fails::it_logs_subject_and_error` |
| E-06 | GREEN. Translate rejections (unknown agent, malformed line) increment a counter by reason and log once per file, not per line. | Plus: `AgentPayload::Unknown(Value)` keeps an unknown agent's raw object whole; OpenActor's events are no longer dropped. | `rs/core/tests/agent_payload_tolerance.rs::when_agent_is_unknown::it_keeps_raw_and_counts_rejection` |
| E-07 | GREEN. The managed NATS child's death is detected within 5 s and logged `event=nats_child_exited` with its exit code; health flips `bus.connected=false`. | `rs/cli/src/managed_nats.rs::tests::when_child_exits::it_is_noticed` |

## H · Health

| id | requirement | acceptance test |
|---|---|---|
| H-01 | GREEN. `NatsBus::is_active` reflects the real connection state; `bus.connected` in `/api/health` is false when the client is disconnected. | `rs/bus/tests/test_bus_health.rs::when_nats_drops::it_reports_disconnected` |
| H-02 | `/api/health` returns `boot.phase` (`starting`, `replaying`, `serving`) with `replay.done`, `replay.total`, `replay.elapsed_ms`. | `rs/tests/test_health.rs::when_replay_is_running::it_reports_phase_and_progress` |
| H-03 | `/api/health` returns HTTP 503 while `boot.phase != serving`, 200 after. `/health` stays 200 whenever the process is up. | `…::when_replaying::it_returns_503_for_readiness_and_200_for_liveness` |
| H-04 | `/api/health` includes per-stream `bytes`, `max_bytes`, `messages`, `percent` for events, local, patterns, ui, changes, read from JetStream. | `…::when_streams_exist::it_reports_bytes_against_caps` |
| H-05 | `/api/health` includes per-consumer `alive`, `restarts`, `last_restart`, `lag` (pending messages). | `…::when_consumers_run::it_reports_alive_and_lag` |
| H-06 | `/api/health` includes `leaf.configured`, `leaf.connected`, `leaf.hub` (redacted URL) and per-watcher `last_event_at`, `age_secs`, `publish_failures`. | `…::when_leaf_is_configured_but_down::it_reports_not_connected` |
| H-07 | `/api/health` includes `version`, `git_sha`, `built_at`, `data_dir`, `store.size_bytes`, `process.rss_bytes`, `uptime_secs`. | `…::when_health_is_read::it_stamps_version_and_sha` |
| H-08 | The dashboard header shows a dot: green when health is ok, amber on any warn, red on any critical, with the JSON one click away. | `ui/tests/components/health-dot.test.tsx::when_health_has_a_critical::it_shows_red_with_the_reason` |

## P · Presence

| id | requirement | acceptance test |
|---|---|---|
| P-01 | The node publishes a `presence` CloudEvent every 15 s (configurable) on `presence.{host}.{principal}` with the H-04 to H-07 payload, `agent: "openstory"`, `subtype: node.presence`. | `rs/tests/test_presence.rs::when_the_node_runs::it_publishes_presence_on_its_subject` |
| P-02 | Presence is an observed family: the persist consumer stores it in its own table (`presence`), never in `events`. | `…::when_presence_arrives::it_lands_in_the_presence_table_not_events` |
| P-03 | `GET /api/fleet/presence` returns the latest presence per node with `age_secs`; a node older than 3 intervals is `stale`. | `…::when_a_node_stops_reporting::it_becomes_stale` |
| P-04 | The leaf and hub configs export and import `presence.>` alongside `events.>` (change lands in `openstory-deploy`; here: the leaf template in `managed_nats.rs`). | `rs/cli/src/managed_nats.rs::tests::when_leaf_config_is_rendered::it_includes_presence_subjects` |
| P-05 | The fleet tab shows each node with its dot and last presence; the local node reads its own presence, not a second path. | `ui/tests/components/fleet-presence.test.tsx::when_two_nodes_report::it_lists_both_with_ages` |
| P-06 | A presence event that fails to publish is logged (E-05 shape) and counted; it never blocks ingestion. | `…::when_publish_fails::it_logs_and_continues` |

## O · Telemetry

| id | requirement | acceptance test |
|---|---|---|
| O-01 | `metrics_enabled` default flips to `true`; `/metrics` serves Prometheus text with the existing gauges plus `openstory_events_ingested_total{agent}`, `openstory_consumer_lag{actor}`, `openstory_stream_bytes{stream}`, `openstory_consumer_restarts_total{actor}`, `openstory_publish_failures_total{watcher}`. | `rs/tests/test_metrics.rs::when_metrics_are_scraped::it_exposes_the_node_gauges` |
| O-02 | An `otlp_endpoint` config/env, when set, exports the same metrics over OTLP with `service.name=openstory`, `service.instance.id=<principal>`, `host.name`. Unset means no exporter and no network. | `…::when_otlp_endpoint_is_unset::it_opens_no_socket` and a testcontainer collector receiving one batch |
| O-03 | Each event carries a span from translate through persist with `session_id`, `subject`, `actor`; sampled at 1 % by default, 100 % under `RUST_LOG=trace`. | `…::when_an_event_flows::it_produces_one_span_per_stage` |
| O-04 | The `observe/` stack (Prometheus and Grafana under `just observe`) gets one dashboard, "Node", with the O-01 gauges; the 2026-03 dashboards are removed or updated. | `scripts/check_docs.py` gains a check that dashboard panel queries reference existing metric names |
| O-05 | PR #46 is closed with a comment pointing at this work; nothing from it is merged. | reviewer gate |

## M · Ops hands on the MCP

| id | requirement | acceptance test |
|---|---|---|
| M-01 | `node_health {}` returns `/api/health` as structured JSON plus a `verdict` (ok, warn, critical) and `findings` computed the same way `scripts/node_health_probe.py` does. | `rs/mcp/tests/ops_hands.rs::when_node_health_is_called::it_returns_verdict_and_findings` |
| M-02 | `node_logs {since?, actor?, level?, limit?}` reads `/api/logs` (L-06). | `…::when_node_logs_is_called_with_actor::it_filters` |
| M-03 | `node_streams {}` returns per-stream bytes against caps with percent. | `…::when_node_streams_is_called::it_reports_percent_of_cap` |
| M-04 | `fleet_presence {}` returns P-03. | `…::when_fleet_presence_is_called::it_lists_nodes_with_staleness` |
| M-05 | `subscribe_health {}` streams health changes (verdict transitions and any finding added or cleared) as notifications. | `…::when_health_flips_to_critical::it_notifies_once` |
| M-06 | Tier 1 hands `node_reproject {session_id}`, `node_verify {session_id}`, `node_catch_up {since}`, `node_prune {older_than_days}` publish an `ops.proposal.<hand>` CloudEvent with `author`, `evidence` (finding ids), and `idempotency_key`, then call the matching REST endpoint; the server records `ops.command.<hand>` with the result. | `…::when_node_reproject_is_called::it_publishes_proposal_then_command` |
| M-07 | Tier 1 hands are refused with a clear error while `boot.phase != serving`. | `…::when_replaying::tier_one_hands_refuse` |
| M-08 | The MCP can publish only subjects in `ops.proposal.>` and `ui.>`; a test enumerates every publish call in `rs/mcp` and asserts the prefix. | `…::when_mcp_publishes::it_only_touches_authored_subjects` |
| M-09 | `openstory_help` and the hands resource document the ops motions (`watch`, `diagnose`, `propose`) with the tier rule stated in one sentence. | `rs/mcp` instructions test (existing pattern) |

## K · Kubernetes shape

| id | requirement | acceptance test |
|---|---|---|
| K-01 | `deploy/k8s/` holds a kustomize base: Deployment (server + NATS leaf sidecar), two PVCs (store, jetstream), ConfigMap from `config.toml`, Secret for the leaf URL, Service on 3002, and a `os-loop-` namespace overlay for a1. `scripts/k3s_smoke.sh` applies it on a1, waits for ready, runs the probe against the pod, and tears the namespace down. | `scripts/k3s_smoke.sh --test` (dry run) and one real run on a1 recorded in the design doc |
| K-02 | Probes: liveness `GET /health`, readiness `GET /api/health` (503 during replay), startupProbe `failureThreshold: 180`, `periodSeconds: 5`. | `scripts/k8s_manifest_check.py` asserts the probe fields; `--test` |
| K-03 | The pod runs `Dockerfile.prod` with `--manage-nats` off and `nats_url` pointing at the sidecar; the sidecar uses the leaf config rendered by `render_leaf_config` (P-04) mounted from the ConfigMap. | `…::when_manifests_render::it_mounts_leaf_conf_and_points_nats_url_at_sidecar` |
| K-04 | Logs go to stdout in JSON (L-01); `kubectl logs` shows one JSON object per line. | recorded in the a1 run |
| K-05 | Horizontal scale is by node: the overlay for two principals produces two Deployments with distinct PVCs and subjects; a single Deployment never has `replicas > 1` (a check refuses it). | `scripts/k8s_manifest_check.py::test_when_replicas_exceed_one_it_fails` |
| K-06 | Optional ops-agent pod: runs `open-story-mcp` against the node's Service with `automountServiceAccountToken: false`; no cluster credential in the pod. | manifest check asserts the field |
| K-07 | `scripts/subject_publishers.py` static audit: maps every `publish(` in `rs/` to a subject prefix; fails on any publisher of `events.`/`local.` outside translate and any MCP publisher outside `ops.proposal.`/`ui.`. | script `--test`; wired into `just test` |
| K-08 | `rs/tests/test_stream_cap_wedge.rs` (testcontainers, needs docker): a node with a tiny events cap is flooded; `/api/health` flips to critical (H-04) before ingestion wedges; with a memory limit the NATS child's death is noticed (E-07). Runs on a1 via `scripts/remote_test.sh`. | the test itself |

## D · DORA

| id | requirement | acceptance test |
|---|---|---|
| D-01 | Every log line and `/api/health` carry `git_sha` and `built_at` (H-07), so a change is identifiable in every signal. | covered by H-07 and L-02 |
| D-02 | `scripts/dora.py` computes the four keys from the node's own record and git: deployment frequency (distinct `git_sha` values seen in presence per day), lead time (commit timestamp to first presence with that sha), change failure rate (share of shas whose first hour of presence contained a critical), time to restore (critical to ok duration). `--test` on fixtures. | script `--test` |
| D-03 | `just test` runs the two static audits (E-01, K-07) and the manifest check (K-02); CI runs the same. | `.github/workflows` diff and a green run |
| D-04 | `scripts/node_health_probe.py --json` is the deploy gate: `scripts/deploy_gate.sh` refuses to roll a new sha while the running node's verdict is critical, and rolls back if the new sha is critical after the startup window. Dry-run test. | script `--test` |
| D-05 | Rollback is a documented one-liner per host shape (brew, compose, k3s) in `docs/deploy/operations.md`, verified once on a1. | doc plus the a1 run |
| D-06 | The DORA numbers appear on the Admin tab as four tiles from D-02's JSON, with the window selectable. | `ui/tests/components/dora-tiles.test.tsx::when_dora_json_loads::it_renders_four_keys` |

## Loop log

- **2026-09-23 22:10 local.** L-01 GREEN (`tracing` + `tracing-subscriber` in
  `rs/server`; `LogFormat`, `build_subscriber`, `init`; `log_format` config
  field and `OPEN_STORY_LOG_FORMAT`; boot wires it in `rs/cli`). Along the
  way: the current stable toolchain (1.96) flags lints in the server and CLI
  crates that CI, which tracks stable, will also flag; cleared them (default
  field assignments, needless mut, `from_ref`, `sort_by_key`, and an allow
  with rationale on the CLI `Command` enum). Owner must decide: nothing new.
  Hub note from the replication check: the hub's Tailscale node key expired
  at 09:59Z today; the box answers on openstory.live, so the fix is
  `tailscale up --force-reauth` from the Hetzner console plus disabling key
  expiry for that node. The probe should learn to flag peer key expiry
  (candidate H-06 addition). Next: L-02.
- **2026-09-23 22:20 local.** L-02 GREEN. A `SpanFields` layer keeps each
  span's fields in its extensions; the JSON line merges them root to leaf
  under the record's own fields, and stamps `event=unnamed` when a record has
  no event name. The four consumer tasks in `rs/src/server/mod.rs` now run
  inside `info_span!("consumer", actor = …)` via `Instrument`, so `actor`
  reaches every line without threading it through calls. Next: L-03.
  Note: the orchestration crate's ignored container and perf test binaries
  carry toolchain-drift lints (`test_pi_mono_container.rs`,
  `test_compose_perf.rs`); the loop lints that crate's lib target and leaves
  those binaries for a dedicated cleanup, tracked under G-06 review.
- **2026-09-23 22:30 local.** L-03 GREEN. `PersistConsumer::process_batch`
  emits `event=batch_persisted` with `session_id`, `persisted`, `skipped`,
  `project_id`; the orchestration loop's duplicate print is gone. The
  `subject` half is re-scoped to E-05 because `IngestBatch` carries no
  subject. Next: L-04 (replace `log_event` with tracing across the server).
- **2026-09-23 22:40 local.** L-04 GREEN. `log_event` is a tracing shim;
  `TextLine` prints the familiar terminal line from the actor span, the
  category field, or the target, hides the `event` name, prefixes WARN and
  ERROR, and appends fields as key=value. JSON sees `category` and
  `message` from every legacy site. Next: L-05 (managed NATS child output
  to a rotated file).
- **2026-09-23 22:50 local.** L-05 GREEN. `spawn_logged` sends the managed
  NATS child's stdout and stderr to `<store_dir>/nats.log`; `open_child_log`
  rotates it to `nats.log.1` past 50 MB. Tonight's "did not become reachable
  within 15s" would have shown "could not parse address string" in that
  file. Next: L-06 (log ring + `GET /api/logs`).
- **2026-09-23 23:05 local.** L-06 GREEN. `LogRing` (5,000 lines, 8 MB,
  seqs from 1) fed by a `RingLayer` in both subscribers; `GET /api/logs`
  with since, actor, level, limit, and a `next` cursor. Both formatters and
  the ring share one `json_object` builder. Next: L-07 (replay progress).
- **2026-09-23 23:20 local.** L-07 GREEN. `ReplayProgress` (pure, injected
  clock) reports on every 10 % and every 5 s; `replay_boot_sessions` emits
  `replay_progress` and `replay_done` with counts and elapsed_ms; the
  orchestration crate's duplicate completion print is gone. Tonight's
  eleven-minute replay would have printed at least ten lines. Note: the
  L-06 commit carried rustfmt churn in `api.rs` and `router.rs` (format
  only). Next: L-08 (`scripts/scratch_node.sh`).
  Correction: the L-07 commit was pushed with one red spec (the L-03 test
  raced another test's scoped subscriber on the process-wide max-level
  hint). The loop's commit gate keyed on grep's exit code instead of
  cargo's; fixed in the next commit by serialising subscriber-installing
  tests and gating on the test exit code.
- **2026-09-23 23:40 local.** L-08 GREEN, group L complete (8/8).
  `scripts/scratch_node.sh` refuses the live ports, renders a JSON-logging
  loopback config whose watchers point at empty folders inside the scratch
  data dir (a first run with empty strings fell back to the real
  `~/.claude/projects` and ingested 22 real sessions; fixed and now asserted
  by a check), boots with `--manage-nats`, waits for `/health`, prints one
  JSON line, and `--stop` tears it down. Smoke: booted on :3106/:4322 with
  the pre-loop release binary, health ok, 0 sessions, stopped clean. Next:
  group E, starting with E-01 (`scripts/swallowed_errors.py`).
- **2026-09-23 23:55 local.** E-01 GREEN. `scripts/swallowed_errors.py`
  (8 self-test assertions) found 27 swallowed writes across the server
  crate, the orchestration loop, and the MCP's stdio; the gate is scoped to
  `rs/server/src` and `rs/src/server`, where nine real writes now log
  `event=<op>_failed` through `logging::failed` and tick
  `openstory_op_failures_total{op}`; four broadcast sends are marked
  audit-ok (no subscribers is not a failure). `just test` runs the gate.
  Next: E-02.
- **2026-09-24 00:15 local.** E-02 GREEN. `consumers::supervision::Driven`
  wraps a subscription receiver: `next()` hands out batches and logs
  `event=consumer_ended` at ERROR once when the channel closes;
  `finish()` returns `ConsumerExit::SubscriptionClosed{batches}`. All four
  consumer loops in the orchestration crate use it. A first cut as an
  async-closure driver could not prove `Send` for the spawned tasks on
  stable; inverting control (driver hands out batches, loop stays a loop)
  kept the bodies untouched. Next: E-03 (supervisor with backoff).
- **2026-09-24 00:40 local.** E-03 GREEN. `supervise(actor, factory, sleep)`
  reruns a consumer's factory on every error with `backoff(attempt)` (1, 2,
  4 … 30 s), logs `consumer_restarted` at WARN with attempt, backoff_ms,
  and reason, ticks `openstory_consumer_restarts_total{actor}`, and keeps
  alive / restarts / last_restart / last_exit in `stats()` for E-04 and
  H-05. All four consumers are supervised; the factory clones Arcs per run
  and recreates the JSONL SessionStore. A subscribe failure is now a
  `SubscribeFailed` exit (restarted) instead of a print. Next: E-04.
- **2026-09-24 00:50 local.** E-04 GREEN. `/api/health` carries
  `consumers: {actor: {alive, restarts, last_restart, last_exit}}` from the
  supervisor's stats. Next: E-05 (watcher publish failures logged with
  subject and error, and the fifteen Grok failures root-caused).
- **2026-09-24 01:20 local.** E-05 GREEN. `logging::publish_failed` logs
  actor, subject, session, batch size, and the whole error chain (`{e:#}`;
  the old print showed only "failed to publish to <subject>"); all three
  watcher publish sites use it; `/api/health` carries `publish_failures`.
  Root cause of the Grok failures: eleven Grok sessions of 13 to 26 MB
  across a few hundred lines, single lines up to 1 MB, batched by count
  into 5 to 10 MB publishes against NATS's 8 MB cap. Fix in the bus:
  `split_batch` (3 specs) and `NatsBus::publish` splits over 4 MB. Not yet
  verified against a live NATS from this branch (the live node runs the
  pre-loop binary); the split is unit-tested and the publish path compiles.
  Next: E-06.
- **2026-09-24 01:45 local.** E-06 GREEN. `AgentPayload::Unknown(Value)`
  (an `untagged` last arm) tolerates any agent tag with the raw object
  kept whole and round-tripping byte for byte; `agent()` reads
  `meta.agent`, else `_variant`, else "unknown"; ten accessors read the
  same-named field from the raw object or return None. `TranscriptState`
  counts rejections by reason; the reader counts `invalid_json` per file
  and logs `translate_rejected` once per file. Schemas regenerated. Next:
  E-07 (managed NATS child death noticed within 5 s).
- **2026-09-24 02:05 local.** E-07 GREEN, group E complete (7/7).
  `NatsGuard::watch` polls the managed child every 500 ms on a thread that
  inherits the tracing dispatcher; an exit logs `nats_child_exited` at
  ERROR with the code, flips `alive()`, and lowers
  `open_story_bus::health::nats_child_alive`, which `/api/health` folds
  into `bus.connected`. Intentional stops (drop) are not errors. Next:
  group H, starting with H-01 (`NatsBus::is_active` truthful).
- **2026-09-24 02:20 local.** H-01 GREEN. `NatsBus::is_active` returns the
  client's `connection_state() == Connected`; a throwaway nats-server on a
  scratch port proves the flip within a second of the server dying. The
  test skips (with a message) where `nats-server` is not on PATH. Next:
  H-02 (boot phase and replay progress on `/api/health`).
