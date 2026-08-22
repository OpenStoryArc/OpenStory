# Reels v2 — rich beats (R1)

**Status:** implement · 2026-08-10 · `feat/agent-pen`  
**Soul:** curation *about* history; never mutate observed events.

## Goal

Expand reels from “event spotlight + line” into a **BLUF-framed slide show**:
title cards, history spotlights, diagram beats, image beats — still grounded
where they claim history.

## R1 scope (this ship)

| Beat `kind` | History anchor | Visual |
|-------------|----------------|--------|
| `spotlight` (default) | **required** sessionId+eventId | EventSpotlight (today) |
| `title` | none | Full-screen title / BLUF card (`line`) |
| `diagram` | optional sessionId for tool-journey | Layered labels (baked or fetched) |
| `image` | none | `visual.imageHref` (https or data:) |

Also:

- Keep reel-level `opener` / `closer` (BLUF bookends already exist).
- Player renders non-spotlight stops without requiring EventSpotlight.
- **Pen type tool** (type text strokes) on Draw tab — studio, not reel player yet.
- MCP `save_reel` schema allows `kind` + `visual` (additional props).

## Out of R1

- Imagine generation pipeline (API + assets store)
- Marginalia *on* reel beats (use Draw overlay while watching for now)
- CloudEvents stream for reel.play (can ride interactions later)
- Animations / chart queries
- Sticky pin of pen to event DOM

## Validation

- `spotlight`: event must exist (unchanged).
- `diagram` / `title` / `image`: `line` required; empty session/event OK.
- Unknown kind → treat as spotlight if ids present, else reject.

## Tests

- Rust: serde round-trip for diagram stop; path traversal still rejected.
- UI: pure `diagramLabelsToStrokes` / stop kind helper; player still sequences.
- API integration: post reel with mixed stops (if fixtures allow).
