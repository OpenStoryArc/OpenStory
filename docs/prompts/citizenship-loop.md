# Loop plan — Session citizenship (Slice 1)

Branch: `feat/session-citizenship-from-loop`  
Cadence: every 3m (scheduler job) · auto-expires 7d  
Soul: observe never interfere · BDD red→green · dogfood API/MCP

## Already done
- [x] Bus reconnect after slow-consumer drop
- [x] `scripts/session_citizenship.py` (verdicts + `--test`)
- [x] `docs/research/session-citizenship-ghosts.md`

## Slice 1 checklist
- [x] Pure `classify` + disk probe (unit-tested, mirrors script) — `rs/server/src/citizenship.rs`
- [x] `GET /api/sessions/{id}/citizenship` → verdict + disk/store/watcher signals
- [x] Integration tests: ghost / citizen / orphan-store / absent — `rs/tests/test_citizenship.rs`
- [x] Ghost risk on `GET /api/health` (watcher emits vs store flat) — `citizenship.ghost_risk`
- [x] MCP tool `session_citizenship` — GET REST citizenship (args: session_id)
- [x] Dogfood live Grok sessions; green `python3 scripts/session_citizenship.py --test`
  - Rebuild note: binary is `open-story-cli` → `cargo build -p open-story-cli` (not `-p open-story`)
  - Live: health.citizenship, GET …/citizenship absent+ghost, script found real ghosts
  - Script scan 2026-07-18: citizens=6 ghosts=4 (incl. planted dogfood ghost)

## Slice 2 (later — do not do in this loop)
- Durable named JetStream consumers
- Backfill rate-limit
- Fancy Live vs Explore UI

## How to run each cycle
1. Read this checklist; pick lowest unchecked Slice 1 box.
2. Failing test first; minimal code; re-run targeted tests.
3. Check the box; brief progress in commit message if landing code.
