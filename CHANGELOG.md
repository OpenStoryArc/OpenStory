# Changelog

All notable changes to OpenStory are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [0.2.0] — 2026-06-09

The "effortless install" release. `brew install openstory` now brings up the
whole stack with one command — no manual NATS, no JetStream config.

### Added
- **`open-story serve --manage-nats`** — serve launches and supervises its own
  JetStream `nats-server` when none is reachable (reuses an existing one in dev),
  and cleans it up on SIGTERM/SIGINT. The Homebrew service uses this, so a single
  `brew services run openstory` brings up NATS + API + dashboard.
- **`open-story init`** — interactive, zero-dependency first-run setup wizard:
  days-of-history, watch dir(s), port + data dir; writes `config.toml` and offers
  to start the service. Non-interactive contexts fail fast with guidance.

### Changed
- Homebrew docs/wizard default to **`brew services run openstory`** (background,
  this login session only); `brew services start` is the explicit launch-at-login
  opt-in.
- One process serves the API **and** dashboard on `:3002` (no separate UI server
  outside dev).

### Removed
- Retired the dead HTTP `/hooks` ingestion residue (stale docs, the WSL setup
  hook config, dead tests). NATS JetStream + the file watcher are the sole
  real-time path. Observed `system.hook`/`progress.hook` events are unaffected.

### Fixed
- `rs/Dockerfile` now copies `mcp/benches/`, so `docker build` (and the e2e image)
  succeed again.
- A `check_docs.py` guard keeps the retired `/hooks` endpoint out of the docs.

## [0.1.0]

Initial Homebrew distribution: source-build formula + bottle workflow.
