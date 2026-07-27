# Plan: MCP Agent Hands (in-band body schema)

**Status:** Phase A+B largely shipped (2026-07-27) — hands/physics/examples embedded, `openstory_help`, gesture descriptions, initialize.instructions dual map. Phase C (empty hints) still open.  

**Audience:** implementers  
**Constraint:** Agents are first-class users of OpenStory. Most will **never** see this repo, `docs/`, or `rs/mcp/SKILL.md`. If it isn’t in the MCP protocol surface, it doesn’t exist for them.

**Non-goals (this plan):** sentence–entity graph store, robotic sim, thinking-trace ingest, `session_compact` full product (noted as follow-on). Those thicken physics later; this plan only makes existing physics **reachable and teachable** in-band.

**Product law (unchanged):** OpenStory surfaces auditable structure. It does not interpret. Docs and tools teach *how to read and cite*, never *what the session meant*.

---

## 0. Who is the user?

I am a coding agent. My host attached `open-story-mcp`. I have:

- `initialize` → `instructions`, `serverInfo`, capabilities  
- `tools/list` → names, descriptions, schemas  
- `tools/call` → results  
- `resources/list` + `resources/read` (if I bother)  
- maybe streaming notifications  

I do **not** have:

- OpenStory git clone  
- human-facing README  
- institutional memory that “session_story is the fact sheet”  
- guaranteed `resources/read` habit (many agents only tool-call)

**Success:** With only that surface, I can use my hands without guessing, and I know the difference between physics and my own prose.

---

## 1. What I need (agent requirements)

### R1 — Orientation on connect (no optional reading)

On `initialize`, I need ≤ ~400 tokens that answer:

1. What is this server for? (read history; optionally drive UI)  
2. Hard law: read-only on history; never invent events; cite when reporting  
3. Physics in one line: events → turns → outcomes → sentences (coordinates, not my summary)  
4. **Motions** (need → first tools), not a dump of 20 names  
5. Pointers: which resource / help tool for depth  

Today: `INSTRUCTIONS` is almost only agent-in-UI. History hands are invisible at connect.

### R2 — Progressive depth without the repo

I need stable URIs I can read when stuck:

| URI | Job |
|-----|-----|
| `openstory://docs/hands` | Motions, default flows, before-you-guess |
| `openstory://docs/physics` | What is fact vs projection; soft holes; citation path |
| `openstory://docs/agent-in-ui` | Already exists — keep |
| `openstory://docs/catalog` | One line per tool + motion tag (optional if hands is enough) |
| `openstory://examples/*` | 2–3 worked procedures with example args |

All **embedded in the binary** (`include_str!` or equivalent). No filesystem, no network to GitHub.

### R3 — Tools teach gestures, not archaeology

Every tool description must answer:

- **WHEN** — user/agent need  
- **MOTION** — orient | what-touched | find | cost | live | show-human  
- **CALL** — required args  
- **RETURNS** — shape + “this is physics / projection”  
- **NEXT** — 1–2 related tools  
- **LIMITS** — only where agents over-claim (sentences, Bash, empty patterns)

Today: many tools are accurate but inert (“Files read or written… Sorted by total operations.”). No WHEN/NEXT.

### R4 — Help is a tool (not only a resource)

Many agents never call `resources/read`. I need:

```text
openstory_help
  topic?: hands | physics | ui | motions | <tool_name>
  need?:  orient | what-touched | find | cost | live | show-human
```

No store I/O. Returns the right card / flow / resource URI. This is the “prosthetic reflex” when I’m lost.

### R5 — Empty and error results educate

When physics is missing, don’t return a bare `[]`.

```json
{
  "items": [],
  "hint": "No turn.sentence patterns. Often: missing turn boundaries. Try session_activity or session_transcript. See openstory://docs/physics#limits"
}
```

Only on empty / soft-failure paths — not on every success (noise kills trust).

### R6 — Worked motions I can copy

I need copy-pasteable sequences (in hands doc + help tool + examples resources):

| Motion | Sequence |
|--------|----------|
| Pickup / resume project | `list_sessions` → `session_story` → optional drill |
| What about this file? | `file_impact` / `recent_files` / `search` → `session_sentences` |
| Find past work | `agent_search` → `session_story` on hits |
| Cost | `daily_token_usage` / `token_usage` |
| Live self-watch | `subscribe_session` / `subscribe_tokens` |
| Show the human | `where_is_user` → `ui_control` (existing UI doc) |

### R7 — Scientific packaging in returns (light)

Where cheap, prefer structured fields agents can cite:

- `session_id`, paths, timestamps already present — keep  
- Hints point at IDs, not narrative conclusions  
- Never add “intent” or “theme” fields from the server  

### R8 — Host-optional skill is not a substitute

`rs/mcp/SKILL.md` and plugin skills help **some** hosts. They must be **generated from or kept in sync with** in-band docs, or they drift. Primary contract = MCP. Skill = optional mirror for Claude/OpenClaw install paths.

---

## 2. Current baseline (gap list)

| Surface | Today | Gap |
|---------|--------|-----|
| `initialize.instructions` | UI-heavy, good for dashboard | No history motions / physics law |
| `resources/*` | Only `openstory://docs/agent-in-ui` | No hands, physics, examples, catalog |
| Tool descriptions | Args + return shape | No WHEN / MOTION / NEXT / LIMITS |
| Meta help tool | None | Agents who don’t resources/read stay blind |
| Empty results | Often bare empty | No soft-hole education |
| Tests | UI resource + initialize UI strings | Need hands curriculum tests |
| SKILL.md | Human/host skill | Drift risk vs in-band |

---

## 3. Design principles for the work

1. **In-band only** — if an agent can’t learn it from MCP, we didn’t ship it.  
2. **Motions over catalogs** — teach needs, not 21 equal tools.  
3. **Physics vs prose** — every educational surface restates the law once.  
4. **Short connect, deep resources** — instructions stay scannable; depth is pull.  
5. **Help tool + resources** — dual path for different client habits.  
6. **Embed, don’t fetch** — binary-local docs (existing pattern).  
7. **One source of truth** — markdown files under e.g. `rs/mcp/docs/` or `docs/mcp/agent/` included into the binary; SKILL.md derived or linked, not hand-divergent.  
8. **No interpretation product** — no “session meaning” tool in this plan.

---

## 4. Phased delivery

### Phase A — Connect + curriculum resources (MVP)

**Ship hands at connect + readable depth.**

**A1. Agent doc pack (source files)**  
Create embedded markdown (suggested paths; adjust to taste):

```text
rs/mcp/agent-docs/
  hands.md       # primary curriculum
  physics.md     # fact vs projection, limits, citation
  catalog.md     # optional: tool × motion table
  examples/
    pickup.md
    file-locus.md
    find.md
```

Content rules:

- No repo paths as required reading  
- No “see CLAUDE.md”  
- Example args use placeholders (`SESSION_ID`, `PROJECT`)  
- Explicit: sentences are projections; do not treat as LLM summaries of intent  

**A2. Protocol wiring** (`rs/mcp/src/protocol.rs`)

- Expand `INSTRUCTIONS` to dual map (history motions + UI one-liner + resource URIs).  
- Cap length: aim ≤ 500 words; link out for depth.  
- `resources/list`: hands, physics, agent-in-ui, examples (and catalog if included).  
- `resources/read`: dispatch by URI table (replace single-URI if).  
- Keep `openstory://docs/agent-in-ui` URI stable (no break).  

**A3. Tests** (`protocol.rs` tests + discovery if needed)

- initialize instructions contain: read-only, motions keywords, `openstory://docs/hands`.  
- resources/list includes hands + physics.  
- resources/read hands/physics returns non-empty markdown.  
- unknown URI still invalid_params.  
- agent-in-ui still works.  

**Acceptance (Phase A):**  
A fresh agent that only reads `initialize` + `resources/read hands` can name three motions and the correct first tool for “what happened in session X” (`session_story` or `session_synopsis` → story).

---

### Phase B — Gesture tool descriptions + help tool

**B1. Description template**  
Apply consistently across `TOOLS` in `rs/mcp/src/tools/mod.rs` (and control tools):

```text
WHEN: …
MOTION: …
CALL: … 
RETURNS: … (physics|projection)
NEXT: …
LIMITS: … (omit if none)
```

Keep each description usable in a tools/list dump (prefer &lt; ~800 chars; hard cap ~1200).

Priority order for rewrite:

1. `session_story`, `session_sentences`, `session_synopsis`, `list_sessions`  
2. `file_impact`, `tool_journey`, `search`, `agent_search`  
3. `token_usage`, `daily_token_usage`, streaming subscribe_*  
4. UI trio (already rich; align MOTION: show-human + pointer to UI resource)  
5. Rest  

**B2. `openstory_help` tool**

- Static content from same agent-docs (or thin router over them).  
- Args: `topic?`, `need?` (see R4).  
- Returns markdown or structured JSON with `text` + `suggested_tools[]`.  
- Register in `TOOLS` + `dispatch_query_tool` (no store).  

**B3. Tests**

- tools/list includes `openstory_help`.  
- help with `need=orient` mentions `session_story` / `list_sessions`.  
- help with `topic=session_sentences` mentions LIMITS / projection.  
- Description smoke: key tools’ descriptions contain `MOTION:` or `WHEN:`.  

**Acceptance (Phase B):**  
Agent with only `tools/list` (no resources) can still pick the right tool for file locus vs cost vs pickup by reading descriptions or calling `openstory_help`.

---

### Phase C — Soft-hole hints on empty results

**C1. Shared helper**  
e.g. `tools/hint.rs`: wrap empty arrays for specific tools.

| Tool | Hint when empty |
|------|-----------------|
| `session_sentences` / patterns filtered to sentences | turn boundary / physics limits |
| `session_patterns` | try without filter; or activity |
| `file_impact` | no tool outcomes / empty session |
| `search` / `agent_search` | try broader query; check API up |
| `session_plans` | no plan mode artifacts |

**C2. Shape**  
Prefer additive field without breaking clients:

```json
{ "items": […], "hint": null }
```

or keep raw array for back-compat and only wrap when empty — **decide in implementation**:  
- Prefer **non-breaking**: if tool today returns a bare array, either keep array and put hint only in MCP `content` text preamble, or return `{ "data": [...], "hint": "..." }` only if no external client depends on bare array.  
- Check MCP tests + any dogfood clients; default to MCP content blocks: first block JSON data, optional second block text hint on empty.

**Acceptance (Phase C):**  
Calling `session_sentences` on a session with zero sentences returns a usable hint referencing physics limits and a fallback tool.

---

### Phase D — Host mirrors + discoverability (optional same PR train)

**D1.** Regenerate or rewrite `rs/mcp/SKILL.md` from `hands.md` + catalog (manual or script).  
**D2.** One paragraph in root README “Using Open Story” / MCP section: agents learn in-band; humans may still read docs.  
**D3.** (Optional) MCP `prompts/list` if we want host slash-commands — only if clients we care about use it; otherwise skip (help tool covers it).

**Acceptance:** Skill and hands doc do not contradict motions table.

---

### Phase E — Follow-ons (out of this plan’s MVP, track only)

Not required for “hands”; empower further later:

| Item | Why later |
|------|-----------|
| `session_compact` | Map-not-territory under context pressure (BACKLOG exists) |
| Entity graph tools | Locus queries; needs store projection |
| Thinking/monologue subtype | Only when source emits; open models |
| Robotic / sim outcomes | New physics kinds; same education pattern |

Each new tool must ship with WHEN/MOTION in description + a line in `hands.md` / help router.

---

## 5. Content specs (what the docs must say)

### 5.1 `hands.md` outline

1. Law (4 bullets: read-only history, prefer tools over memory, cite, UI is ui.* only)  
2. Physics one-liner + pointer to physics.md  
3. Motions table  
4. Default flows (pickup, file, find, cost, live, show-human) with ordered tools  
5. “If stuck → `openstory_help` or resources/read this URI”  
6. What not to do (invent sessions, claim intent as fact, dump whole transcript first)

### 5.2 `physics.md` outline

1. Layers: events, turns, outcomes, sentences  
2. Sentences = projection / coordinates  
3. Soft holes: no turn boundary → no sentences; Bash softer than Write/Edit; federated pattern gaps if relevant  
4. Citation path: claim → tool fields → event ids when available  
5. Thinking/monologue: only if present in store — never assume  

### 5.3 `INSTRUCTIONS` outline (connect)

```text
OpenStory MCP — observe your coding history (read-only) and optionally drive the dashboard (ui.* only).

LAW: Prefer these tools over memory for past work. Cite session_id / paths / event ids. Do not invent events. Sentences are projections of acts, not intent labels.

MOTIONS:
  orient      list_sessions → session_synopsis | session_story
  what-touched file_impact | session_sentences | tool_journey
  find        search | agent_search
  cost        token_usage | daily_token_usage
  live        subscribe_session | subscribe_tokens
  show-human  where_is_user | ui_control

DEPTH: openstory_help | resources/read openstory://docs/hands
       physics: openstory://docs/physics
       dashboard: openstory://docs/agent-in-ui
```

---

## 6. Implementation checklist (files)

| Area | Likely touch |
|------|----------------|
| Docs source | `rs/mcp/agent-docs/*.md` (new) |
| Protocol | `rs/mcp/src/protocol.rs` — INSTRUCTIONS, resources list/read, tests |
| Tools registry | `rs/mcp/src/tools/mod.rs` — descriptions, help tool, dispatch |
| Help impl | `rs/mcp/src/tools/help.rs` (new) |
| Empty hints | per-tool handlers in `per_session.rs` / `search.rs` / shared hint helper |
| Host skill | `rs/mcp/SKILL.md` sync |
| Human docs | brief note in `docs/mcp-architecture.md` “agent-facing curriculum” |
| Architecture claim | if we count tools, update tool count in mcp-architecture + `scripts/check_docs.py` expectations if any |

---

## 7. Test plan

| Level | What |
|-------|------|
| Unit | initialize / resources / help routing / description markers |
| Integration | existing MCP discovery tests still green; new help call |
| Dogfood | Start MCP only; as agent, complete three tasks with no repo: (1) list recent sessions, (2) story one session, (3) file_impact — using only in-band help if stuck |
| Regression | agent-in-ui resource URI and control tools unchanged |

No E2E browser required for MVP.

---

## 8. Rollout / PR strategy

| PR | Scope | Merge bar |
|----|--------|-----------|
| **PR1** | Phase A: instructions + hands/physics/examples resources + tests | unit green; dogfood connect text |
| **PR2** | Phase B: description pass + `openstory_help` + tests | tools/list dogfood |
| **PR3** | Phase C: empty hints + SKILL sync + mcp-architecture blurb | empty-sentences dogfood |

Keep PRs reviewable; PR1 alone already upgrades first-class agents.

---

## 9. Success criteria (product)

An agent **with zero OpenStory repo access** can:

1. State history is read-only and citable after `initialize` alone.  
2. Resume a project via the orient motion without human coaching.  
3. Answer “what happened to this file?” via what-touched tools.  
4. Discover depth via `openstory_help` or `resources/read hands` when stuck.  
5. Avoid claiming sentence text as “intent fact.”  
6. Drive UI only through the documented ui.* seam when needed.

If any of 1–4 requires opening GitHub, the plan failed.

---

## 10. Open decisions (resolve in PR1)

1. **Return shape for hints** — second content block vs wrapper object (prefer non-breaking).  
2. **Whether `catalog.md` is separate** or folded into hands (prefer fold if hands stays short).  
3. **`openstory_help` in PR1 vs PR2** — default PR2; PR1 is enough if instructions + resources are excellent.  
4. **Doc home** — `rs/mcp/agent-docs/` (next to binary) vs `docs/mcp/agent/` (human-visible, include_str path longer). Prefer `rs/mcp/agent-docs/` for “ships with the tool.”

---

## 11. Why this order

Agents die at **connect** and **tool choice**. Graph stores and robots increase data wealth; they don’t fix “I have 21 mystery verbs.” Hands-first, physics-labeled, in-band only — then richer physics attaches to the same curriculum slots.

---

## 12. One-line charter

**Make every connected agent a first-class citizen of OpenStory by teaching body schema and motions inside the MCP itself—so that without the repo, they still have hands, and without interpretation, they still have science.**
