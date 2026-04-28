# Open Story — Architecture Overview

A visual companion to [`docs/soul/architecture.md`](./soul/architecture.md). This file is all diagrams: what flows where, who owns what state, and how the pieces fit together.

---

## 1. The whole pipeline at a glance

Events flow one direction from coding agents to the dashboard. No component writes back upstream.

```mermaid
flowchart LR
    subgraph Sources["Sources (I/O in)"]
        CC[Claude Code<br/>~/.claude/projects/]
        PI[pi-mono<br/>~/.pi/agent/sessions/]
        HK[HTTP Hooks<br/>POST /hooks]
    end

    subgraph Ingest["Ingest (pure)"]
        W[Watcher<br/>notify crate]
        R[Reader<br/>byte-offset tail]
        T[Translators<br/>per-agent]
    end

    NATS[(NATS JetStream<br/>events.> / patterns.> / changes.>)]

    subgraph Consumers["Independent Actor-Consumers (tokio tasks)"]
        P[persist]
        PT[patterns]
        PJ[projections]
        B[broadcast]
    end

    subgraph Storage["Durable State"]
        ES[(EventStore<br/>SQLite / Mongo)]
        JL[(JSONL backup<br/>per-session)]
        FTS[(FTS5 index)]
    end

    subgraph UI["React Dashboard"]
        LIVE[Live Tab<br/>WebSocket]
        EXP[Explore Tab<br/>REST]
    end

    CC --> W
    PI --> W
    W --> R --> T
    HK --> T
    T -->|CloudEvents 1.0| NATS

    NATS --> P
    NATS --> PT
    NATS --> PJ
    NATS --> B

    P --> ES
    P --> JL
    P --> FTS
    PT --> ES
    PJ --> ES

    ES -->|REST /api/*| EXP
    B -->|WebSocket| LIVE
```

**Key properties**
- Read-only observation: no component ever writes back to the coding agent.
- Functional core: translate/views/patterns are pure; side effects live at watcher/server/consumer boundaries.
- Each consumer is an independent failure domain — no shared `RwLock`, only `Arc<dyn EventStore>`.

---

## 2. Two data paths to the UI

The Live and Explore tabs are deliberately **never merged**. Each is honest about its data source.

```mermaid
flowchart LR
    E[Event arrives] --> SQL[(SQLite<br/>durable)]
    E --> WS[WebSocket<br/>ephemeral]

    SQL -->|REST /api/sessions<br/>/records /search| EXPLORE[Explore Tab<br/>client-side indexes<br/>turn / file / tool / agent]

    WS -->|snapshot + deltas| LIVETAB[Live Tab<br/>reacts to stream]

    style SQL fill:#2b5a3e,color:#fff
    style WS fill:#5a4a2b,color:#fff
```

| Path | Durability | Consumer | UI tab |
|---|---|---|---|
| SQLite → REST | durable, authoritative | `persist` + `projections` | Explore |
| Broadcast → WebSocket | ephemeral, live | `broadcast` | Live |

---

## 3. NATS subject hierarchy

Subjects encode the parent/child relationship between a main session and its subagents. A single wildcard subscription captures both.

```mermaid
flowchart TD
    Root["events.>"]
    Proj["events.{project_id}.>"]
    Sess["events.{project_id}.{session_id}.>"]
    Main["events.{project_id}.{session_id}.main"]
    Agent["events.{project_id}.{session_id}.agent.{agent_id}"]

    Root --> Proj --> Sess
    Sess --> Main
    Sess --> Agent

    PAT["patterns.>"]
    CHG["changes.>"]

    Main -.detector output.-> PAT
    Main -.session meta.-> CHG
```

Streams:

| Stream | Retention | Purpose |
|---|---|---|
| `events` | limits-based, 1 GB | all CloudEvents from sources |
| `patterns` | durable | detector outputs (eval-apply, sentence, phases…) |
| `changes` | interest-based | session metadata deltas |

---

## 4. The four consumer actors

Each is a `tokio::spawn`ed task subscribing to the same NATS stream. They share **no mutable state** — the contract between them is the NATS subject, nothing else.

```mermaid
flowchart LR
    NATS[(NATS events.>)]

    NATS --> P[persist]
    NATS --> PT[patterns]
    NATS --> PJ[projections]
    NATS --> B[broadcast]

    P -->|dedup + write| ES[(EventStore)]
    P -->|append| JL[(JSONL)]
    P -->|index| FTS[(FTS5)]

    PT -->|7 detectors<br/>pure state machines| PATEV[PatternEvents]
    PATEV -->|publish| NATS2[(NATS patterns.>)]
    PATEV --> ES

    PJ -->|incremental<br/>materialized views| PROJ[SessionProjection<br/>tokens, metadata, depths]
    PROJ --> ES

    B -->|ViewRecord → WireRecord<br/>truncation| WS[WebSocket clients]
```

**The 7 pattern detectors** (`rs/patterns/src/`, one file each):

```mermaid
flowchart TB
    E[ViewRecord] --> EA[eval-apply<br/>SICP scope open/close]
    E --> SE[sentence<br/>per-turn narrative]
    E --> TC[test cycles<br/>edit→test→fix]
    E --> GW[git workflows<br/>commit/branch/push]
    E --> ER[error recovery<br/>error→fix]
    E --> AD[agent delegation<br/>main↔subagent]
    E --> TP[turn phases<br/>classification]
```

Each detector implements `(state, event) → (new_state, patterns)` — a pure state machine, no I/O.

---

## 5. Persistence layer — the EventStore seam

`EventStore` is an async trait; two backends implement it and a 47-helper conformance suite enforces semantic parity.

```mermaid
classDiagram
    class EventStore {
        <<trait, async>>
        +insert_event(ev)
        +events_for_session(id)
        +search_fts(query)
        +synopsis / pulse / context
        +tool_journey / file_impact
        +token_usage / productivity
    }

    class SqliteStore {
        data_dir/open-story.db
        events_fts virtual table
        json_extract + strftime + LIKE
    }

    class MongoStore {
        feature = "mongo"
        collections: events/sessions/<br/>patterns/turns/plans/events_fts
        $text + $dateFromString + $exists
    }

    EventStore <|.. SqliteStore
    EventStore <|.. MongoStore

    class JsonlStore {
        per-session *.jsonl
        always-on backup
        sovereignty escape hatch
    }
```

**Backend selection** (`data/config.toml`):

```toml
data_backend = "sqlite"   # default
# data_backend = "mongo"  # requires --features mongo
```

Boot fails loudly if `mongo` is configured without the feature compiled in — **never silently falls back**.

JSONL is always on, regardless of backend. Your data is always grep-able from outside the database.

---

## 6. Crate dependency graph

The Rust workspace has 8 members. The CLI is a thin wrapper so `cargo test` never touches the binary (avoids Windows file-lock conflicts).

```mermaid
flowchart TB
    CLI[open-story-cli<br/>thin binary]
    ROOT[open-story<br/>orchestration lib]
    SRV[open-story-server<br/>HTTP/WS + 4 consumers]
    BUS[open-story-bus<br/>NATS abstraction]
    STORE[open-story-store<br/>EventStore + projections]
    VIEWS[open-story-views<br/>CloudEvent → ViewRecord]
    PAT[open-story-patterns<br/>7 detectors]
    CORE[open-story-core<br/>CloudEvents + translators]

    CLI --> ROOT
    ROOT --> SRV
    ROOT --> BUS
    SRV --> STORE
    SRV --> VIEWS
    SRV --> PAT
    SRV --> BUS
    STORE --> CORE
    VIEWS --> CORE
    PAT --> VIEWS
    BUS --> CORE

    style CORE fill:#2b3a5a,color:#fff
    style VIEWS fill:#2b3a5a,color:#fff
    style PAT fill:#2b3a5a,color:#fff
    style STORE fill:#2b3a5a,color:#fff
    style BUS fill:#5a3a2b,color:#fff
    style SRV fill:#5a3a2b,color:#fff
```

Blue = pure domain logic. Orange = infrastructure with side effects.

> `rs/semantic/` exists on disk but is **not** a workspace member — vestigial Qdrant code scheduled for removal.

---

## 7. End-to-end trace: one tool call

Follow a single `PostToolUse` hook from Claude Code to the Explore tab.

```mermaid
sequenceDiagram
    participant CC as Claude Code
    participant HK as /hooks (server)
    participant TR as translate
    participant N as NATS
    participant PS as persist
    participant PT as patterns
    participant PJ as projections
    participant BR as broadcast
    participant ES as SQLite
    participant WS as WebSocket
    participant UI as React UI

    CC->>HK: POST /hooks (PostToolUse)
    HK->>TR: raw JSON
    TR->>TR: detect agent, translate<br/>(raw preserved)
    TR->>N: publish CloudEvent<br/>events.{proj}.{sess}.main

    par Fan-out
        N->>PS: deliver
        PS->>ES: insert + FTS index
        PS->>PS: JSONL append
    and
        N->>PT: deliver
        PT->>PT: 7 detectors (pure)
        PT->>N: publish patterns.>
        PT->>ES: persist patterns
    and
        N->>PJ: deliver
        PJ->>ES: update SessionProjection
    and
        N->>BR: deliver
        BR->>BR: ViewRecord→WireRecord<br/>(truncation)
        BR->>WS: push delta
    end

    WS-->>UI: Live tab renders card
    UI->>ES: (later) GET /api/sessions/{id}/records
    ES-->>UI: Explore tab renders timeline
```

---

## 8. Event type taxonomy

All events share `type: "io.arc.event"` and carry an `agent` discriminator. The `subtype` is hierarchical.

```mermaid
flowchart LR
    ROOT["io.arc.event"] --> MSG[message.*]
    ROOT --> SYS[system.*]
    ROOT --> PROG[progress.*]
    ROOT --> FILE[file.*]
    ROOT --> Q[queue.*]

    MSG --> MU[message.user.prompt<br/>message.user.tool_result]
    MSG --> MA[message.assistant.text<br/>message.assistant.tool_use<br/>message.assistant.thinking]

    SYS --> ST[system.turn.complete<br/>system.error<br/>system.compact<br/>system.hook]
    SYS --> SS[system.session_start<br/>system.model_change<br/><i>pi-mono</i>]

    PROG --> PR[progress.bash<br/>progress.agent<br/>progress.hook]

    FILE --> FS[file.snapshot]

    Q --> QE[queue.enqueue<br/>queue.dequeue]
```

The `agent` field (`"claude-code"`, `"pi-mono"`) lets the views layer branch on format-specific fields without mutating `data.raw`.

---

## 9. Config & I/O surface summary

```mermaid
flowchart TB
    subgraph Inputs
        W1[watch_dir<br/>~/.claude/projects/]
        W2[pi_watch_dir<br/>~/.pi/agent/sessions/]
        HE[HTTP /hooks<br/>port 3002]
    end

    subgraph Server[Server — port 3002]
        REST[REST /api/*]
        WSE[WebSocket /ws]
        MET[/metrics if enabled/]
        HOOK[/hooks/]
    end

    subgraph External
        NATSEXT[(NATS :4222)]
        MONGO[(Mongo :27017<br/>optional)]
    end

    subgraph OnDisk[data_dir = ./data]
        DB[(open-story.db<br/>SQLite + FTS5)]
        JLS[(*.jsonl per session)]
        CFG[config.toml]
        PLANS[plans/]
    end

    W1 --> Server
    W2 --> Server
    HE --> HOOK
    Server <--> NATSEXT
    Server <--> DB
    Server --> JLS
    Server -.optional.-> MONGO
    Server --> REST
    Server --> WSE
```

**Default ports:** server `3002`, UI dev `5173`, NATS `4222`, Mongo `27017`, Qdrant `6334`.

---

## See also

- [`docs/soul/architecture.md`](./soul/architecture.md) — prose version with more "why"
- [`docs/soul/philosophy.md`](./soul/philosophy.md) — principles that shaped these choices
- [`docs/architecture-tour.md`](./architecture-tour.md) — 14-stop guided code walkthrough
- [`docs/soul/patterns.md`](./soul/patterns.md) — learned anti-patterns (what *not* to do)
