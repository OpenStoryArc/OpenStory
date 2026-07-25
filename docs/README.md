# Open Story Documentation

**Mission:** read your agent history.  
**Constraint:** observe, never interfere.

Use this map by *what you are trying to do* — not by folder name.

## I want to run Open Story

1. **[Root README — Quickstart](../README.md#quickstart)** — Homebrew install, open the dashboard
2. **[README — Using Open Story](../README.md#using-open-story)** — MCP, skills, REST essentials, more agents
3. **[README — Install options](../README.md#install--run-all-options)** — source, Docker, Mongo, verify
4. Stuck on empty UI? Run a coding-agent session so transcripts appear under the watch dir (default `~/.claude/projects/`)

## I want to understand the product

1. **[Soul — Philosophy](soul/philosophy.md)** — mission, constraint, across the whens, attention layer
2. **[Soul — Architecture](soul/architecture.md)** — pipeline and how to read history via the API
3. **[Agent-in-UI](agent-in-ui.md)** — optional attention layer (steer the mirror, don’t rewrite history)
4. **[MCP architecture](mcp-architecture.md)** — how `open-story-mcp` reads the store safely

## I want to contribute or change the code

1. **[CONTRIBUTING.md](../CONTRIBUTING.md)** — non-negotiables and workflow
2. **[Soul — Patterns](soul/patterns.md)** — mistakes already made
3. **[Soul — Use cases](soul/use-cases.md)** — principles in real code
4. **[Architecture tour](architecture-tour.md)** — guided walk through the codebase
5. **[BACKLOG.md](BACKLOG.md)** — what to build next (entry = spec)

## I want to deploy or federate

- **[Distributed / multi-machine](deploy/distributed.md)** — NATS leaf, hub, Tailscale, `publish_sessions`
- **[Hetzner notes](deploy/hetzner.md)** — if present in tree

## Research & deep dives

Working notes, audits, and prototypes under **[research/](research/)**. Not the product path — expect draft voice and dates. Session dogfood example: [research/sessions/](research/sessions/).

## Security

- **[security/](security/)** — audits and plans
- Root README [Security notes](../README.md#security-notes) for runtime defaults

## Working in this repo (agents and humans)

When you need to know what a session did: **read history through the API or `scripts/sessionstory.py`** — don’t grep raw transcripts. Boot commands and config tables live in `CLAUDE.md` at the repo root.
