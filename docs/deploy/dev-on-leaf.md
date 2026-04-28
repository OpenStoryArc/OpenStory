# Local Dev Against the Leaf Stack

How to develop the React UI with hot-module-reload while keeping the NATS leaf
link to the team hub alive — so you see your local sessions *and* your
collaborators' sessions in the dashboard you're editing.

## The state we're describing

```
Browser ──→ :5173  Vite dev server (host-native node)
                    │  reads ui/src/, full HMR on save
                    │
                    ├── /api/*  ──proxy──→ http://localhost:3002 ┐
                    ├── /ws     ──proxy(ws)─────────────────────┤
                    └── /hooks  ──proxy───────────────────────── ┤
                                                                 ▼
                          :3002  open-story:prod (Docker, leaf stack)
                                  │
                                  ├── poll-watcher reads /watch
                                  │   (mounted from ~/.claude/projects)
                                  └── NATS client → :4222 leaf NATS
                                                     │
                                                     │  leafnode replication
                                                     ▼
                                       hub on Tailscale @ 100.77.40.95:7422
                                       (Max's VPS — collaborators publish here)
```

Three moving parts:

1. **Leaf NATS + open-story:prod (Docker)** — same as `docker-compose.leaf.yml`
   describes. NATS connects out as a leafnode to the hub; the open-story binary
   subscribes to local NATS and watches `~/.claude/projects` for your own
   transcripts.
2. **Vite (host-native, not Docker)** — serves `ui/src/` on `:5173` with HMR,
   proxies `/api`, `/ws`, `/hooks` into the leaf container on `:3002`.
3. **Browser** — points at `:5173` and never sees `:3002` directly. The proxy
   makes it feel like one origin.

## Why this configuration over the alternatives

| Alternative | What you get | What you give up |
|---|---|---|
| **Standalone dev compose** (`docker-compose.yml`) | Mongo + Vite + server in one `docker compose up` | No leafnode link → no collaborators' sessions, only yours |
| **Native Rust + Vite** (`just up-no-mongo`) | Fastest Rust feedback loop | Need to manually run a leafnode NATS *and* keep the watch dir wired |
| **Prod stack only** (leaf compose, browser on `:3002`) | Real distribution shape, single port | No HMR — every UI change requires rebuilding `open-story:prod` |
| **This config** (leaf prod + Vite on host) | UI HMR *and* collaborators' sessions | Rust changes still require an image rebuild |

We chose this because the work in front of us is **frontend iteration on data we
can only get through the leaf link**. A standalone stack hides bugs that only
appear when you're rendering a heterogeneous mix of agents (`claude-code` from
your machine, `pi-mono` and `claude-code` from Max's machine, varied project
structures, varied event volumes). HMR matters because UI work is cheap to
break and cheap to fix, so the round-trip cost dominates.

If you find yourself needing to change Rust code more than the UI, switch to
`just up-no-mongo` and accept the loss of the leaf link for that session — it's
the right tool for that work.

## Prerequisites

- Tailscale connected to the same tailnet as the hub VPS. Verify with:

  ```bash
  tailscale status | grep debian-16gb-ash-1   # or whichever hostname/IP your hub uses
  nc -zv 100.77.40.95 7422                    # TCP reachability of the leafnode port
  ```

- `deploy/nats-leaf.local.conf` exists with the real hub token + Tailscale
  address, and `listen: 0.0.0.0:4222` (so the open-story container can reach
  it on the compose network). This file is gitignored.

- `docker-compose.leaf.local.yml` exists (also gitignored) and overrides the
  NATS volume mount to use `nats-leaf.local.conf` instead of the committed
  template `nats-leaf.conf`.

- `open-story:prod` image is built from current `master` (or your branch). The
  prod image needs the `OPEN_STORY_WATCHER=poll` env var that `Dockerfile.prod`
  sets — this is what makes the file watcher reliable inside Docker on macOS.
  See `docs/soul/patterns.md` ("Inotify is broken inside Docker on macOS") for
  why.

- `ui/node_modules` populated — `cd ui && npm install` if not.

## Recreating from a cold start

```bash
# 1. Make sure no other open-story stack is bound to 3002.
docker compose down 2>/dev/null
docker compose -f docker-compose.leaf.yml down 2>/dev/null

# 2. Rebuild the prod image if you've changed Rust code or the Dockerfile.
#    Skip this if the existing image is current.
docker build -f Dockerfile.prod -t open-story:prod .

# 3. Bring up the leaf stack with the local override (real NATS config).
docker compose \
  -f docker-compose.leaf.yml \
  -f docker-compose.leaf.local.yml \
  up -d

# 4. Verify the leaf link came up. Look for "Leafnode connection created"
#    in the NATS logs and "mode: Poll" in the open-story logs.
docker compose -f docker-compose.leaf.yml -f docker-compose.leaf.local.yml \
  logs nats | grep -iE 'leafnode|listening'
docker compose -f docker-compose.leaf.yml -f docker-compose.leaf.local.yml \
  logs open-story | grep -iE 'mode|watching'

# 5. Confirm the API is serving collaborator data through the leaf.
#    "total" should be much larger than your local session count alone —
#    that's the proof the leafnode link is replicating.
curl -s http://localhost:3002/api/sessions \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print("total =", d.get("total"))'

# 6. Start Vite on the host, pointed at the leaf container.
cd ui && VITE_API_URL=http://localhost:3002 npx vite --host 127.0.0.1 --port 5173

# 7. Open the dev UI.
open http://localhost:5173
```

## Verifying it's actually working

Three independent checks. If any one of them fails, you'll have a confusing
partial-success state — fix the failing layer before chasing UI bugs.

```bash
# Vite is serving your source.
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://localhost:5173/
# Expect: 200 text/html

# Vite's proxy reaches the leaf API.
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://localhost:5173/api/sessions
# Expect: 200 application/json

# Leaf API actually has collaborator data (not just yours).
curl -s http://localhost:3002/api/sessions \
  | python3 -c 'import json,sys
from collections import Counter
d = json.load(sys.stdin)
projects = Counter(x.get("project_id") or "<none>" for x in d.get("sessions",[]))
print("total:", d.get("total"))
print("distinct projects:", len(projects))
print("top:")
for p,n in projects.most_common(5):
    print(f"  {n:4d} {p}")'
# Expect: a mix of -Users-<you>-... and -Users-<collaborator>-... project ids.
```

## Tearing it down

```bash
# Stop Vite (whichever shell it's in: Ctrl+C, or:)
lsof -ti:5173 | xargs kill

# Stop the leaf stack.
docker compose -f docker-compose.leaf.yml -f docker-compose.leaf.local.yml down
```

## Operational footnotes

- **Two browser tabs trap.** With this config running, `:5173` shows your live
  source, but `:3002` *also* serves a UI — it's the prebuilt bundle baked into
  the prod image. They are different builds and edits to `ui/src/` only show up
  on `:5173`. If something looks wrong, check the URL.

- **Vite is on the host, not in Docker.** This is deliberate. The dev compose
  runs Vite in a `node:22-slim` container that re-runs `npm install` on every
  boot (slow). Native node uses your existing `node_modules` and starts in
  about a second.

- **HMR works for UI; Rust still needs a rebuild.** If you change a `.rs` file
  while developing this way, you have to `docker build -f Dockerfile.prod -t
  open-story:prod . && docker compose -f docker-compose.leaf.yml -f
  docker-compose.leaf.local.yml up -d` to pick it up. That's the explicit
  trade-off this configuration makes — UI-fast, Rust-slow.

- **The polling watcher is a deliberate cost.** The prod image runs with
  `OPEN_STORY_WATCHER=poll` because Docker Desktop on macOS does not propagate
  inotify events from host writes. We pay ~2s detection latency and a small
  amount of idle CPU to get reliable observation of your transcripts. Native
  runs (`just up-no-mongo`) get inotify/FSEvents speed without this cost.
  Selection logic: `rs/src/watcher.rs::WatcherKind::from_env()`.

- **Collaborator events arrive through NATS, not the watcher.** Even if your
  poll-watcher were broken, you'd still see Max's sessions — they ride into
  your local NATS via the leafnode link, and the open-story binary's NATS
  subscription consumes them. The watcher is only for *your* transcripts on
  *your* disk.

## Related docs

- `docs/deploy/distributed.md` — the original NATS leaf topology design.
- `docs/soul/patterns.md` — "Inotify is broken inside Docker on macOS" entry.
- `CLAUDE.md` — Learned anti-patterns, including the watcher tradeoff note.
- `rs/src/watcher.rs` — `WatcherKind` and the env-driven selection.
- `ui/vite.config.ts` — proxy config (the `VITE_API_URL` knob).
