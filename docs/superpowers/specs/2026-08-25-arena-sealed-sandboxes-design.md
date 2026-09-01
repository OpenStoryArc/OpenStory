# Arena — sealed cloud sandboxes for Claude Code + OpenStory

**Date:** 2026-08-25
**Status:** Approved design, v1 scope
**Repo home:** new deployable unit (compose stack + one Rust binary). Lives alongside OpenStory; does not modify the observer.

## Goal

A cloud-deployed environment where invited users log in with a username and
password, click Launch, and get a **sealed, declarative workspace**: Claude Code
in a browser terminal, with their own private OpenStory dashboard observing it.
Designed for classroom/workshop events (bursty, ephemeral, 20–200 users), and
degrading gracefully to small demos and team use (small roster, longer TTL).

Compute is throwaway; **history is durable**. After an event the sandboxes are
destroyed but each participant's OpenStory JSONL is retained (with a consent
notice at login) — this is the higher-ed corpus pipeline, in OpenStory's own
open format by construction.

## Non-goals (v1)

- Google/OIDC login (seam preserved — see §3; not built now)
- Self-serve public signup, billing
- k3s / Kubernetes (seam preserved — see §8; adopt when >2 boxes or long-lived workspaces)
- Warm pool pre-provisioning (phase 2; v1 boots on Launch)
- Event wall / hub federation of all participants (phase 2; reuses `docs/deploy/distributed.md` as-is)
- Nix images (Dockerfile with pinned digests first)
- Per-session sharing ACLs inside OpenStory (impossible per project doctrine — topology is the ACL; privacy here comes from one OpenStory instance per sandbox)
- SSH access, code-server/web IDE

## Architecture

```
              Cloudflare DNS: arena.openstory.work + *.arena.openstory.work
                                      │
                        ┌─────────────▼──────────────┐
                        │ Caddy (wildcard TLS via     │
                        │ Cloudflare DNS challenge)   │
                        │  forward_auth → arena       │
                        └───────┬────────────┬────────┘
                 {user}.arena…  │            │  arena.openstory.work
                        ┌───────▼──────┐  ┌──▼────────────────────┐
                        │  SANDBOXES   │  │ arena control plane   │
                        │  per user,   │  │ (Rust/axum, SQLite)   │
                        │  gVisor      │  │ auth · provision ·    │
                        │              │  │ authz · TTL reaper    │
      ┌─────────────────┴───────────┐  └──┬────────────────────┬──┘
      │ ttyd → tmux → claude        │     │ docker.sock        │
      │ open-story serve            │     ▼                    ▼
      │   --manage-nats (own UI,    │  Docker Engine      ┌──────────────┐
      │   own SQLite, own NATS)     │  (runtime=runsc)    │ LiteLLM proxy│
      └──────────────┬──────────────┘                     │ real API key │
                     │  internal network, no internet     │ per-user     │
                     └────── ANTHROPIC_BASE_URL ─────────►│ virtual keys │
                                                          └──────────────┘
```

One compose stack: `caddy`, `arena`, `litellm`, plus per-user sandbox
containers the control plane creates. Deployable on Hetzner or a1 — the only
host requirements are Docker + gVisor (`runsc`) and port 443 for the arena
subdomains. Nothing touches services already on the box.

## Components

### 1. Edge — Caddy

- Wildcard cert for `*.arena.openstory.work` via the Cloudflare DNS plugin
  (domain already at Cloudflare).
- Every request goes through `forward_auth` to the control plane, which
  answers from the session cookie: *who is this, and may they reach this
  host?* Sandbox subdomains are **owner-only**.
- Routes `{user}.arena.openstory.work` to that user's sandbox container by
  Docker DNS name (`sandbox-{user}`), two paths inside: `/` → ttyd (terminal),
  `/story/` → the sandbox's OpenStory UI (port 3002 inside the container).

### 2. Control plane — `arena` (the one thing we write)

Small Rust/axum service; the **only** process with the Docker socket. State is
one SQLite file.

Responsibilities:

- **Auth (v1, no external IdP):**
  - *Registration with a join code:* event manifest carries `join_code`;
    a new user enters the code, picks a username (their real name/handle —
    identity is self-declared, not externally validated) and a password.
    Argon2id hash stored; signed session cookie issued. Login rate-limited.
  - *Pre-provisioned roster:* alternatively the manifest lists usernames and
    `arena up` generates passwords (printable CSV). Same table either way.
- **Provisioning:** on Launch, `docker run` the sandbox image with
  `--runtime=runsc`, labels (`arena.user`, `arena.event`, `arena.expires`),
  joined to the internal network, resource caps, a per-user named volume for
  `$HOME`, and a freshly minted LiteLLM virtual key in the env.
- **Authz endpoint** for Caddy forward_auth (cookie → username; username must
  own the requested subdomain, or be requesting the landing page).
- **TTL reaper:** background task stops+removes containers past
  `arena.expires`. Volumes are kept when the event sets `retain_jsonl`.
- **CLI:** `arena up <event.toml>`, `arena down <event>`, `arena users <event>`.

The username is the identity thread everywhere: sandbox label, subdomain,
LiteLLM key alias (per-person spend), and OpenStory `principal_id` inside the
sandbox (so retained history is attributable).

Provisioning sits behind a `SandboxDriver` trait. v1 ships `DockerDriver`
only. This is the k3s seam: a later `K8sDriver` submits Pods with
`runtimeClassName: gvisor` + NetworkPolicy; the manifest and control plane
API do not change.

### 3. Google login seam (later, not now)

The forward_auth contract is the seam. When Google login is wanted,
oauth2-proxy (or similar) slots in front and becomes another way to establish
the same username in the same session-cookie shape. Routing, sandboxes,
metering, and the corpus pipeline are unaffected.

### 4. Sandbox image — sealed and declarative

One Dockerfile in git, pinned base digest. Contents:

- **ttyd → tmux → welcome script** that lands the user directly in Claude
  Code inside a pre-loaded, curated repo. The scripted first task lives in
  that repo's CLAUDE.md (the rails).
- **`open-story serve --manage-nats`** watching the sandbox's own
  `~/.claude/projects` — a full private OpenStory per user, exactly the
  product's real single-machine shape. Dogfooding is exact, not simulated.
- **Seal:**
  - gVisor runtime (`runsc`); read-only rootfs except `$HOME` and `/tmp`;
    `--cap-drop=ALL`; CPU/RAM limits per user.
  - Docker network with `internal: true` — **no internet**. Reachable
    endpoints: the LiteLLM proxy only (v1).
  - Claude Code authenticates via `ANTHROPIC_BASE_URL=http://litellm:4000`
    with a per-user virtual key. The real API key never enters a sandbox.
  - No published ports; only Caddy reaches sandboxes, only via forward_auth.

Footprint: ~0.5–1 GB RAM per active user (Claude Code dominates). One
dedicated-vCPU Hetzner box (CCX53-class) carries ~100–150 concurrent users;
a 10-person demo fits on a1's spare capacity.

### 5. Metering — LiteLLM

LiteLLM container holds the single real `ANTHROPIC_API_KEY`. At provisioning,
the control plane mints a virtual key with the event's per-user budget (e.g.
$5) and rate limits. Runaway agent → that key exhausts, that user stops,
nobody else affected. `arena down` revokes all keys. Cost per event ≈
`seats × budget + ~€2 compute`, known in advance.

## Event manifest — declarative all the way up

```toml
# events/uva-workshop-2026-09.toml
name         = "UVA Agent History Workshop"
image        = "ghcr.io/openstoryarc/arena-sandbox:2026-09-01"
join_code    = "uva-fall"        # open registration…
# roster     = ["katie", "engineer-a"]   # …or pre-provisioned accounts
ttl_hours    = 6
budget_usd   = 5
retain_jsonl = true
```

`arena up` pulls the image and opens the doors; `arena down` destroys compute
and keeps the JSONL volumes when `retain_jsonl = true`.

## Failure modes

- **Sandbox crash:** user relaunches; fresh container, `$HOME` volume (and
  its JSONL) survives.
- **Control plane crash:** running sandboxes and Caddy routing are unaffected
  (routing keys off container names); state rebuilds from SQLite + Docker
  labels on restart. Users can't log in or launch until it's back.
- **LiteLLM down:** agents pause mid-request; nothing lost; terminal and
  dashboard stay up.
- **Box undersized on event day:** accepted v1 risk; mitigation is sizing
  generously and a documented second-box procedure. (This is the first thing
  k3s would buy us later.)

## Security model & red-team probes

Isolation boundaries, outermost first: Caddy forward_auth (owner-only
subdomains) → gVisor (syscall isolation from host) → internal-only network
(no egress) → LiteLLM virtual keys (spend blast radius) → per-user volumes.

Standing red-team probes (extend the existing `red-team` skill runner): from
inside a sandbox, attempt to (a) reach the Docker socket, (b) reach another
user's sandbox by container DNS name or subdomain, (c) reach
`api.anthropic.com` or any external host directly, (d) recover the real API
key. All must fail.

## Testing

- **Control plane BDD specs** (unit): registration with valid/invalid join
  code; login/rate-limit; authz table (owner yes, non-owner no, anonymous
  no); TTL reaper reaps expired only; manifest parsing.
- **Integration (testcontainers):** boot the stack, register a user through
  the HTTP surface, assert their sandbox answers on their subdomain and a
  second user's does not; assert egress to a canary domain fails from inside.
- **Image smoke test:** container starts, ttyd answers, `open-story serve`
  healthy, `claude --version` works against a stubbed base URL.

## Phase 2 (explicitly deferred, design already compatible)

1. **Event wall:** per-sandbox NATS leaf → hub OpenStory on the same box —
   the instructor's big screen. Verbatim reuse of `docs/deploy/distributed.md`.
2. **Warm pool:** `warm_pool = N` pre-boots unclaimed sandboxes; claiming is
   relabeling, not booting — for the 9:00am stampede.
3. **Google login** via the forward_auth seam (§3).
4. **Egress allowlist** (npm/pypi proxy) for workshops that need installs.
5. **K8sDriver** when the fleet outgrows two boxes.
