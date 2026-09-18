# Memory hands parity — scorecard (no soft language)

**Date:** 2026-09-18
**Definition of DONE:** an agent with only MCP can perform every Narrator and Rememberer
responsibility, each with a land assert on the store. Spec:
`openstory-research/memory/hands/REQUIREMENTS.md`; design:
`openstory-research/memory/notes/2026-09-17-memory-hands.md`.

## Verdict

| Claim | Status |
|-------|--------|
| **Read side** (carry a handle, descend, surface, context, search, related) | **PASS** |
| **Stream side** (closed exchanges and arcs reach a host, resumable) | **PASS** |
| **Write side** (enrich, adjudicate, stitch, propose keep land on the store) | **FAIL** — `memory.*` deferred (group D) |
| **Host motions** (narrate, adjudicate, remember, listen as compositions) | **FAIL** — group E not landed |

## Rows

| Responsibility | Hands | Land assert | Status |
|---|---|---|---|
| Carry a handle | `story_list`, `story_search` | line/hit handle equals `expected.json` on every golden (`story_hands` parity) | PASS |
| Dereference a handle | `story_summary` | `down` equals the arc's exchange handles; enrichment fields absent until enriched | PASS |
| Descend without losing meaning | `story_descend`, `story_context` | context == surface(node) + descend(parent) on goldens; ranges tile the stream | PASS |
| Find the way back up | `story_surface` | event → sentence → exchange → arc on every golden exchange | PASS |
| Pointers across | `story_related` | arcs sharing entities, most shared first | PASS |
| Hear the story close | `subscribe_arcs` | ack, per-pattern notification, cancel drops the route; live JetStream smoke | PASS |
| Resume from a cursor | `subscribe_arcs { from_seq }` | stored prefix from `batch_seq`, then the live tail | PASS |
| Enrich a closed arc | write hand `enrich` | `memory.enrich` row; next `story_summary` carries the title | FAIL — deferred (D) |
| Adjudicate an ambiguous seam | write hand `adjudicate_boundary` | `memory.verdict` row | FAIL — deferred (D) |
| Stitch a saga / propose what to keep | `link_saga`, `propose_keep` | `memory.saga` / `memory.keep` rows | FAIL — deferred (D) |
| Answer a creator question under budget | host motion `remember` | 12-schema recall set correct, tokens spent ≤ 1,400 | FAIL — group E |
| Narrate live | host motion `listen` | an arc closed → a reading written, author-stamped | FAIL — group E |
| Never write history | none | `HttpEventStore::insert_event` is a hard error (`rs/mcp/tests/http_store.rs`) | PASS |

## Gates (must stay green)

```bash
just test-memory   # folds, goldens, properties, read + stream hands, backfill
OPENSTORY_HOST=http://localhost:3002 cargo test -p open-story-server --test test_story_dogfood   # real data
cargo test -p open-story-mcp --test nats_smoke   # real JetStream (skips without NATS)
```

If any fails, parity is FAIL until fixed. No "mostly".

**Honest answer:** read and stream sides DONE; the write side and the host motions are not, by scope decision, and are the next branches.
