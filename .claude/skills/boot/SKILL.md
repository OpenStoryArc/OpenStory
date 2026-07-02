---
name: boot
description: Start, run, boot, or "stand up" OpenStory locally. Use when the user asks to start/run/boot OpenStory, bring up the stack, or restart it. Walks the check-running → pick-mode → verify-prereqs → check-ports → boot → verify flow, with a conversational tone. Three modes (Docker / native SQLite / split dev) plus "you choose for me." Serves UI at http://localhost:5173 and API at http://localhost:3002.
---

# Booting OpenStory

Three modes plus "you choose for me." All three serve the UI at http://localhost:5173 and API at http://localhost:3002.

## Tone

Be conversational and match the user's register — if they're casual or excited ("let's get started!"), match that energy; if they're terse, stay terse. Greet them back. Say what you're about to check before you check it — but say it like a person, not a procedure ("quick check first…" beats "before doing anything I'll verify…"). When you list the modes, weave them into prose instead of a parenthetical-heavy single sentence. Don't ignore curl errors silently; acknowledge them naturally ("nothing answering on 3002, so it's not up").

## Steps

1. **Check if it's already running** — narrate first ("Let me see if it's already up…"), then `curl -s -o /dev/null -w "%{http_code}" http://localhost:3002/api/sessions`. A `200` means it's up; tell the user warmly that it's already at http://localhost:5173 and offer the natural next steps (tail logs, leave it, restart) — don't restart unless they ask.
2. **If not running, ask which mode they want** — phrase it as a question, not a menu. Mention the four options inline if helpful, but the goal is "how would you like to start it?" not "select an item from this list."
   - **Docker** — `docker compose up -d --build`. No native deps, ~3 min first build.
   - **Native SQLite** — `just up-no-mongo`. Fastest restart; needs `cargo`, `npm`, `nats-server`.
   - **Split dev mode** — three terminals (`just nats`, `just serve`, `just dev`). For active code editing.
   - **You choose for me** — probe `docker info`, `just --version`, `cargo --version`, `nats-server --version` in parallel. Prefer Docker if available, then native SQLite. Tell the user what you picked and why.
3. **Verify prereqs before starting** — run `just check --mode <native|docker>` (or no flag to check all; under the hood: `scripts/check-prereqs.sh`). If anything's missing, walk the user through it: the brew-installable ones can be installed in one shot with `just check --mode <…> --install` after the user agrees; for things the script can't auto-install (Rust toolchain via rustup, Docker Desktop), surface the install command and let the user run it. Don't silently fall back to a different mode.
4. **Check for port collisions** — `lsof -ti:3002,5173,4222`. Any PID = something's bound; investigate before booting.
5. **After boot, verify** the API returns 200 on `/api/sessions` and the UI loads.
6. **If `~/.claude/projects/` is empty,** warn the user the UI will show zero sessions until they run a Claude Code session somewhere — expected, not a bug.

## Tone example

> **User:** let's get started with openstory!
>
> **Claude:** Hey, let's do it! Quick check first to see if it's already running… nothing on 3002, so we're starting fresh. A few ways to go:
>
> - **Docker** is the simplest — one command, no native deps to install. ~3 min the first time.
> - **Native SQLite** (`just up-no-mongo`) is fastest if you've already got `cargo`, `npm`, and `nats-server` on the machine.
> - **Split dev mode** runs each piece in its own terminal — best when you're going to be editing code.
>
> Or I can poke around and pick based on what's installed. What sounds good?

## Modes (reference)

### Docker

```bash
docker compose up -d --build      # first boot (~3 min)
docker compose down               # stop (volumes persist)
docker compose logs -f server     # tail logs
```

Override watch dir: `CLAUDE_PROJECTS_DIR=/path/to/projects docker compose up -d`.

### Native, SQLite

```bash
just up-no-mongo
```

Prereqs: `brew install just nats-server`, plus `cargo` and `npm`.

### Split dev mode

```bash
just nats     # terminal 1 — NATS JetStream
just serve    # terminal 2 — Rust server
just dev      # terminal 3 — Vite UI (HMR)
```

**Why Vite runs separately in dev (and not in prod):** Vite is a separate process here purely for hot module reload while editing UI code. The Rust server can serve UI static files directly via `--static-dir` (the capability is in `rs/server/src/router.rs` and has been since the initial commit). `Dockerfile.prod` uses exactly that path — `npm run build` → `ui/dist` → `--static-dir /static` — so the deployed shape is a single binary serving both API and UI. The dev split is a UI-iteration ergonomics choice, not an architectural one.
