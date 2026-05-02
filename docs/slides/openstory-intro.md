---
marp: true
theme: default
paginate: true
header: "OpenStory"
footer: "Personal sovereignty for humans working with AI agents"
---

# OpenStory

**Personal sovereignty for humans working with AI agents.**

A mirror, not a leash.

---

## The Problem

When you use a coding agent, it acts on your behalf.

- Reads your files
- Runs commands
- Makes decisions

You have to trust blindly — or not at all.

---

## The Solution

Full visibility into that process, in real time.

- Observe — never interfere
- Translate — into a form you can see, search, reason about
- Own — open formats, portable data

---

## Architecture

```
watcher → translate → NATS JetStream → consumers
                                    ├─ persist    (EventStore)
                                    ├─ patterns   (detection)
                                    ├─ projections (materialized views)
                                    └─ broadcast  (WebSocket → UI)
```

Each consumer is an independent actor with its own failure domain.

---

## Principles as Constraints

1. Observe, never interfere
2. Behavior-Driven Development
3. Actor systems and message-passing
4. Functional-first, side effects at the edges
5. Reactive and event-driven
6. Open standards, user-owned data
7. Minimal, honest code

---

## Stack

- **Rust** — 9-crate workspace
- **NATS JetStream** — hierarchical event bus
- **SQLite / MongoDB** — pluggable EventStore
- **React + RxJS** — reactive dashboard
- **CloudEvents 1.0** — open event format

---

## Try It

```bash
brew install nats-server
just up-no-mongo
```

Then open `http://localhost:5173`.

---

# Questions?

github.com/maxglassie/OpenStory
