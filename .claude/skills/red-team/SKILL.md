---
name: red-team
description: Run OpenStory's automated red-team — dependency scanners, aggressive in-process tests, and testcontainer-based exploit probes — then report which attack vectors were fended off and which broke through. Use when the user asks "audit security", "run a red team", "are we secure", "what attacks would work", or before merging anything that touches auth, the HTTP surface, or dependencies. Dogfoods the project's own tooling — never invent ad-hoc checks when the runner exists.
---

# red-team

A two-phase skill: a script attacks the system, you read the report.

The runner is deterministic so you cannot hallucinate a clean bill of health. If a probe didn't run, the report says `skipped`; if it broke through, the report says `red`. Anything else is a bug in the runner.

## Phase 1 — Run the red team

```bash
# Full run — every probe (~5-15 min if container build is needed)
python3 scripts/red_team.py

# Quick run — skip container tests + slow scanners (~1 min)
python3 scripts/red_team.py --quick

# One category only
python3 scripts/red_team.py --only deps      # cargo-audit, osv-scanner, npm audit
python3 scripts/red_team.py --only policy    # cargo-deny, clippy -D warnings, install-script audit
python3 scripts/red_team.py --only tests     # test_security + test_security_aggressive + test_security_container

# Machine-readable output
python3 scripts/red_team.py --json | jq

# CI exit policy
python3 scripts/red_team.py --fail-on medium   # exit 1 on medium+ severity red probes
```

The script requires:
- `cargo`, `cargo-audit`, `cargo-deny` on PATH (install: `cargo install cargo-audit cargo-deny --locked`)
- `npm` for the npm audits
- `docker` for container tests (optional — they auto-skip without it)
- `osv-scanner` binary (optional — downloads from github.com/google/osv-scanner)

## Phase 2 — Read the report and act

The report has three categories:

- **deps** — `cargo-audit`, `osv-scanner`, `npm audit`. Red here means a known CVE in a transitive dependency. Fix path: bump the offending crate or its parent, document if blocked upstream.
- **policy** — `cargo-deny check bans sources`, `clippy -D warnings`, install-script inventory. Red here means a policy violation (banned crate, unknown registry, deny lint).
- **tests** — `test_security` (20 baseline), `test_security_aggressive` (27 in-process exploit attempts), `test_security_container` (7 real-container probes). Red here means a probe broke through — there's a real bug to fix or a regression to revert.

Each red probe in the human-readable output lists the first 5 findings (the JSON output has all of them).

## When to use this skill

- The user says "audit security", "run a red team", "are we secure", "do we have CVEs"
- Before merging a PR that touches: `rs/server/src/auth.rs`, `rs/server/src/router.rs`, `rs/server/src/ws.rs`, `Caddyfile`, `Dockerfile*`, `docker-compose*.yml`, `rs/store/src/sqlite_store.rs`, or any `Cargo.toml` / `package.json` / `requirements.txt`
- After bumping any direct dependency
- On the weekly cron (set up via `/schedule`)
- When responding to a Dependabot alert

## What this skill does NOT do

- It does not fix anything. Red probes get reported; a follow-up agent or human fixes them.
- It does not chase 0-day research — only known advisories + existing test coverage. To go deeper, see `docs/security/deep-analysis-plan.md` (the section on cargo-fuzz, mutation testing, loom).
- It does not test the *agent* (Claude Code / pi-mono / OpenClaw) — OpenStory's threat boundary is the server. The agent's own security is its own concern.

## What "red" means by category

| Category | Red probe means | First action |
| --- | --- | --- |
| `cargo-audit` | RustSec advisory matches a crate in `rs/Cargo.lock` | `cargo update -p <crate> --precise <fix>`; if blocked, bump the parent crate in `Cargo.toml` |
| `osv-scanner` | OSV (cross-DB) advisory match | Same as above; OSV often catches what RustSec hasn't synced |
| `npm audit` | npm advisory match | `npm audit fix`, then re-run; check if the chain is dev-only |
| `cargo-deny bans/sources` | A banned crate or unknown registry leaked in | Find the introducer with `cargo tree -i <crate>`, replace |
| `clippy -D warnings` | A new lint failed | Fix at the call site; never `#[allow]` security-relevant lints |
| `test_security*` red | An exploit probe broke through | Bug fix in the source; **never** weaken the assertion to pass |
| `test_security_container` red | Same, but against the running container — likely an HTTP/network-layer issue not visible in process | Inspect the failing probe's name; fix the handler |

## Extending the red team

To add a new attack vector:
1. Write the in-process probe first (`rs/tests/test_security_aggressive.rs`) — fast feedback.
2. If the probe needs real network/Docker behavior, add a container variant (`rs/tests/test_security_container.rs`).
3. If neither test file fits (e.g., it's a static-analysis check), add a new `probe_*` function in `scripts/red_team.py`.
4. Update the table above with what "red" means for the new probe.

The full design rationale for which tools we run, why, and what each one catches that the others miss lives in `docs/security/deep-analysis-plan.md`.
