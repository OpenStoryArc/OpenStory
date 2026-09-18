# Memory hands parity — scorecard (no soft language)

**Date:** 2026-09-18 (updated after group D)
**Definition of DONE:** an agent with only MCP can perform every Narrator and Rememberer
responsibility, each with a land assert on the store. Spec:
`openstory-research/memory/hands/REQUIREMENTS.md`; design:
`openstory-research/memory/notes/2026-09-17-memory-hands.md`.

## Verdict

| Claim | Status |
|-------|--------|
| **Read side** (carry a handle, descend, surface, context, search, related) | **PASS** |
| **Stream side** (closed exchanges and arcs reach a host, resumable) | **PASS** |
| **Write side** (enrich, adjudicate, stitch, propose keep land on the store) | **PASS** — memory table/collection, `memory.>`, `/api/memory`, four write hands |
| **Host motions** (narrate, adjudicate, remember, listen as compositions) | **PARTIAL** — prompts, notifications with prompt refs, schemas, validator, channel push and skills landed; the plugin eval suite (E-07..E-10) is not |

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
| Enrich a closed arc | write hand `enrich` | memory row (kind enrichment); next `story_summary` carries `title` and the author (`story_hands`, `test_memory_api`) | PASS |
| Adjudicate an ambiguous seam | write hand `adjudicate_boundary` | memory row (kind verdict); `story_summary.verdicts` | PASS |
| Stitch a saga / propose what to keep | `link_saga`, `propose_keep` | memory rows (kind saga / keep) via the same door (`write_hands`) | PASS |
| Answer a creator question under budget | `remember` prompt + skill | 12-schema recall set correct, tokens spent ≤ 1,400 | FAIL — eval suite (E-07) not landed |
| Narrate live | `subscribe_arcs` + channel mode + `listen` skill | an arc closed → prompts named on the wire → enrichment written through `enrich` | PASS for the wire and the write; the in-session dogfood (E-10) is pending |
| Never write history | none | `HttpEventStore::insert_event` is a hard error (`rs/mcp/tests/http_store.rs`) | PASS |

## Gates (must stay green)

```bash
just test-memory   # folds, goldens, properties, read + stream hands, backfill
OPENSTORY_HOST=http://localhost:3002 cargo test -p open-story-server --test test_story_dogfood   # real data
cargo test -p open-story-mcp --test nats_smoke   # real JetStream (skips without NATS)
```

If any fails, parity is FAIL until fixed. No "mostly".

**Honest answer:** read, stream, and write sides DONE with land asserts; the host motions are carried by the MCP (prompts, notifications, channel push) and skills; what remains is the eval suite and the in-session dogfood.
