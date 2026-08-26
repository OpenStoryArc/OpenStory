# Arena — deploy runbook

Design: [sealed-sandboxes spec](../docs/superpowers/specs/2026-08-25-arena-sealed-sandboxes-design.md) · Build: [Arena v1 plan](../docs/superpowers/plans/2026-08-25-arena-v1.md)

Ephemeral, sealed coding-agent sandboxes for events. One host runs Caddy
(TLS termination + owner-only authz), the control plane (`arena` binary —
auth, provisioning, LiteLLM key minting), LiteLLM (the one real Anthropic
key, proxied per-user), Postgres (LiteLLM's own state), and, created and
destroyed at event time, one sealed Docker sandbox per participant.

This is a **v1, single-host** design. See "Second box" at the bottom for
what to do if one host isn't enough for an event.

## 1. Host prerequisites

### Docker + gVisor

Install Docker normally, then install gVisor (`runsc`) — it's the syscall
isolation layer between each sandbox and the host kernel:

```bash
curl -fsSL https://gvisor.dev/archive.key | sudo gpg --dearmor -o /usr/share/keyrings/gvisor-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main" | sudo tee /etc/apt/sources.list.d/gvisor.list
sudo apt-get update && sudo apt-get install -y runsc
sudo runsc install   # registers the `runsc` runtime with the Docker daemon
```

Then edit `/etc/docker/daemon.json` to register the runtime explicitly and
to widen Docker's default address-pool allocation:

```json
{
  "runtimes": {
    "runsc": {
      "path": "/usr/local/bin/runsc"
    }
  },
  "default-address-pools": [
    { "base": "10.200.0.0/16", "size": 24 }
  ]
}
```

**Why `default-address-pools` matters:** every sandbox gets its own
`/24` internal network (`arena-sb-{user}`, created and destroyed at
runtime by the control plane — see `src/docker_driver.rs`). Docker's
out-of-the-box default pool (`172.17.0.0/16` sliced into a handful of
`/20`s) runs out after roughly a dozen networks. A `/16` sliced into
`/24`s gives ~250 usable per-sandbox networks — comfortably above any
single-host event's roster size, with room to spare for `edge` and
`backplane`.

Restart Docker after editing the file:

```bash
sudo systemctl restart docker
docker info | grep -A2 Runtimes   # confirm "runsc" is listed
```

**Host services bind loopback.** `docker_driver.rs`'s module doc records
the residual: a sandbox's network is `internal: true` (no route out, no
route to other sandboxes), but it still has a route to the host's own
Docker bridge gateway address. Anything the host binds on `0.0.0.0` is
therefore reachable from *every* sandbox on the box, gVisor or not — the
seal stops sandbox-to-sandbox and sandbox-to-internet traffic, not
sandbox-to-host. Whatever you run on this host outside the compose stack
(a debug shell, a metrics exporter, an ad-hoc `python -m http.server`)
**must bind `127.0.0.1`, never `0.0.0.0`.** The compose stack itself
follows this rule already — only Caddy publishes ports, and only 80/443.

### Firewall

Open inbound 80 and 443 only. Nothing else needs to be reachable from the
internet; Postgres, LiteLLM, and the control plane are all on Docker's
internal `backplane` network.

## 2. Cloudflare DNS

`openstory.work` is already registered and managed at Cloudflare. Add,
under the zone you're deploying into:

| Type  | Name                      | Value           | Proxy status |
|-------|---------------------------|-----------------|--------------|
| A     | `arena`                   | `<host IP>`     | DNS only     |
| CNAME | `*.arena`                 | `arena`         | DNS only     |

**Proxy status must be "DNS only" (grey cloud), not proxied.** Caddy
terminates TLS itself via the Cloudflare DNS-01 challenge (`tls { dns
cloudflare ... }` in the Caddyfile) and needs to see the real client IP for
the rate limiter and forward_auth's host-based authz — Cloudflare's
proxy would rewrite both.

Generate a scoped API token for the DNS-01 challenge: Cloudflare dashboard
→ My Profile → API Tokens → "Edit zone DNS" template, scoped to the
`openstory.work` zone only. This is `CF_API_TOKEN` below.

## 3. First boot

```bash
cd arena/deploy
cp .env.example .env
```

Fill in `.env`:

- `ARENA_BASE_DOMAIN` — e.g. `arena.openstory.work`
- `ARENA_COOKIE_KEY` — 128 hex chars (64 random bytes). Fastest path:
  `openssl rand -hex 64`. Equivalent to the binary's own `arena keygen`
  subcommand — `docker compose run --rm arena arena keygen` — if you'd
  rather use that (works once the `arena` service has an image to run;
  `docker compose build arena` first if you haven't booted yet).
- `CF_API_TOKEN` — the scoped token from step 2
- `ANTHROPIC_API_KEY` — the one real key; it lives only in the `litellm`
  container's environment and is never passed into a sandbox
- `LITELLM_MASTER_KEY` — `openssl rand -hex 32`
- `POSTGRES_PASSWORD` — `openssl rand -hex 16`
- `ARENA_DOCKER_RUNTIME=runsc` (leave unset for local dev without gVisor —
  see "Local-dev mode" below)
- `ARENA_SANDBOX_CPUS` / `ARENA_SANDBOX_MEMORY_BYTES` — optional; leave
  unset to use the binary's defaults (2 CPUs, 2GiB). See the commented-out
  lines at the bottom of `.env.example`.

**Every line in `.env` must be exactly `KEY=value`, with no trailing
comment.** `docker compose`'s `.env` parser has no comment syntax on a
value line — `LITELLM_MASTER_KEY=   # openssl rand -hex 32` sets the
master key to the literal string `"  # openssl rand -hex 32"`, not an
empty value. `.env.example` keeps every explanatory comment on its own
line above the variable for exactly this reason; keep that shape when you
edit `.env`.

Then boot the stack:

```bash
docker compose up -d --build
docker compose ps            # arena-pg should show "healthy"; the rest
                              # show "running" (only postgres has a
                              # healthcheck defined — litellm waits on it
                              # via depends_on/service_healthy)
docker compose logs -f arena # tail the control plane
```

## 4. Build and push the sandbox image

The sandbox image (`arena/sandbox/Dockerfile`) is built and pushed
separately from the deploy stack — event manifests reference it by tag
(`image = "ghcr.io/openstoryarc/arena-sandbox:2026-09-01"`).

```bash
# from the repo root — the sandbox image COPYs rs/ to build open-story-cli
docker build -f arena/sandbox/Dockerfile -t ghcr.io/openstoryarc/arena-sandbox:2026-09-01 .
docker push ghcr.io/openstoryarc/arena-sandbox:2026-09-01
```

**Pre-pull it on the deploy host before an event.** `DockerDriver` does
**not** pull images — it only creates containers from what's already
present locally (see `docker_driver.rs`). If the image isn't pulled ahead
of time, the first participant's `arena up`-triggered launch will fail
with an image-not-found error mid-event. Pull it explicitly:

```bash
docker pull ghcr.io/openstoryarc/arena-sandbox:2026-09-01
```

Re-pull whenever you change the tag in a manifest.

## 5. Running an event

Event manifests are host-authored TOML (see `arena/events/example-event.toml`
for the format: `name`, `image`, `join_code` or `roster`, `ttl_hours`,
`budget_usd`, `retain_jsonl`). The control plane reads them from
*inside* the `arena-cp` container — there's no HTTP upload path for
manifests — so `docker-compose.yml` bind-mounts `./events` (i.e.
`arena/deploy/events/` on the host) to `/events` in the container,
read-only (the control plane only ever reads a manifest, never writes one
back).

Drop your manifest on the host, then:

```bash
cp my-event.toml arena/deploy/events/
docker exec arena-cp arena up /events/my-event.toml
```

- With a `join_code`, this prints a confirmation line — share the code and
  `https://{ARENA_BASE_DOMAIN}/register` with participants.
- With a `roster`, this prints CSV credentials (`username,password,event`
  — three columns, one row per participant) to stdout — **capture this
  output immediately**; it is not stored anywhere and cannot be
  regenerated. Redirect it straight to a file you control and delete once
  distributed:

  ```bash
  docker exec arena-cp arena up /events/my-event.toml > /root/my-event-creds.csv
  ```

  Treat that CSV like any other credential dump: don't leave it on shared
  storage, don't email it in the clear.

List who's registered for an event at any point:

```bash
docker exec arena-cp arena users my-event
```

Tear the event down when it's over — this destroys every sandbox
container, revokes its LiteLLM virtual key, and removes its database row.
Sandboxes' `$HOME` volumes (and their OpenStory JSONL) are kept if the
manifest set `retain_jsonl = true` (the default), destroyed otherwise:

```bash
docker exec arena-cp arena down my-event
```

A relaunch after a sandbox crash is just the participant re-authenticating
— the control plane recreates the container and reattaches the existing
`$HOME` volume, so in-progress JSONL survives.

### Consent notice

Participants see a retention notice on the registration page before they
create an account: *"Session history from this event may be retained by
the organizer"* (`arena/src/assets/register.html`). This exists because
`retain_jsonl = true` (the manifest default) means each participant's
OpenStory transcript outlives the event on the organizer's disk. If you
run an event with `retain_jsonl = true`, make sure participants have
actually seen that notice before they start — don't route around
`/register` (e.g. pre-provisioning accounts via `roster` without telling
people what's retained).

## 6. Local-dev mode

No gVisor, no real DNS, no Cloudflare token needed. Two things differ from
production:

**`ARENA_DOCKER_RUNTIME` unset.** Leave it out of `.env` (or set it to
empty). `DockerDriver` treats an absent runtime as "use the Docker
default" — sandboxes run without gVisor. Fine for iterating on the
control plane; don't use it for a real event.

**Cookies need a real domain, not `localhost`.** The session cookie is
scoped to `ARENA_BASE_DOMAIN` so it's shared between the base domain and
every `*.{base}` sandbox subdomain — that doesn't work with bare
`localhost`, and the cookie is marked `Secure`, so it won't be sent over
plain HTTP either. Fake both with `/etc/hosts` plus Caddy's internal CA:

1. Add to `/etc/hosts`:
   ```
   127.0.0.1  arena.test
   127.0.0.1  alice.arena.test
   127.0.0.1  alice-story.arena.test
   ```
   (one `{user}.arena.test` / `{user}-story.arena.test` pair per local
   test user you plan to register).

2. `arena/deploy/Caddyfile.dev` already ships in this repo — it's identical
   to `Caddyfile` except `tls internal` (Caddy's local CA, no Cloudflare
   token needed) in place of the Cloudflare DNS block. Keep the two files'
   directives in sync if you change one (a comment at the top of
   `Caddyfile.dev` says the same).

3. Trust Caddy's local CA in your browser (Caddy prints the cert on first
   run, or export it: `docker compose exec caddy cat
   /data/caddy/pki/authorities/local/root.crt`), then run with
   `ARENA_BASE_DOMAIN=arena.test` and mount `Caddyfile.dev` in place of
   `Caddyfile` (e.g. `docker compose -f docker-compose.yml -f
   docker-compose.dev-override.yml up`, or just swap the volume line
   locally — don't commit that swap to the shared compose file).

Validate `Caddyfile.dev` the same way as production, against the built
image (see "Validation" below).

## 7. Validation

Run before every deploy change, from `arena/deploy`:

```bash
cp .env.example .env   # fill dummies locally

docker compose config -q                      # compose syntax valid

docker compose build caddy arena              # both images build

# Caddy validate MUST run against the custom-built image (it has the
# cloudflare DNS module compiled in) — never stock caddy:2, which will
# fail to even parse the `tls { dns cloudflare ... }` directive.
#
# NOTE: the cloudflare DNS module sanity-checks the token's *shape* even
# during `validate` (no network call) — a trivial value like `CF_API_TOKEN=x`
# is rejected with "API token 'x' appears invalid". Use a fake value that's
# at least token-shaped (Cloudflare tokens are 40-char base64url strings).
docker run --rm -v "$PWD/Caddyfile:/etc/caddy/Caddyfile:ro" \
  -e ARENA_BASE_DOMAIN=arena.test -e CF_API_TOKEN=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  arena-caddy caddy validate --config /etc/caddy/Caddyfile

# Caddyfile.dev uses `tls internal`, not the cloudflare module, so it
# doesn't need a real-shaped token — validate it the same way regardless:
docker run --rm -v "$PWD/Caddyfile.dev:/etc/caddy/Caddyfile:ro" \
  -e ARENA_BASE_DOMAIN=arena.test \
  arena-caddy caddy validate --config /etc/caddy/Caddyfile
```

(`arena-caddy` is the image tag `docker compose build` produces for the
`caddy` service under the `arena` project name — confirmed by `docker
images | grep caddy` after building; check yours if your Compose version
tags it differently.)

**`caddy validate` cannot see routing-logic bugs** — it only checks that
the Caddyfile parses into *some* valid config, not that the config does
the right thing. The wildcard site's two sandbox routes (`-story` →
ttyd terminal) are exactly the kind of bug that class misses: a named
matcher (`@user`) declared *inside* an unmatched `handle {}` block is
syntactically valid and "validates" fine, but is never actually applied
to anything, so its regex capture (`{re.user.1}`) is never populated at
request time and every non-`-story` request 502s. Catch this class with
`caddy adapt` (compiles the Caddyfile to its JSON form, no live traffic
needed — hermetic) plus a `jq` assertion that both sandbox routes carry a
`header_regexp` matcher and a non-empty dial placeholder:

```bash
docker run --rm -v "$PWD/Caddyfile:/etc/caddy/Caddyfile:ro" \
  -e ARENA_BASE_DOMAIN=arena.test -e CF_API_TOKEN=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  arena-caddy caddy adapt --config /etc/caddy/Caddyfile --adapter caddyfile \
  > /tmp/arena-adapted.json

for name in story user; do
  jq -e --arg n "$name" \
    '[.. | objects | select(has("match")) | select(.match|type=="array")
       | select(.match[]? | objects | .header_regexp.Host.name? == $n)] | length == 1' \
    /tmp/arena-adapted.json
done

jq -e '[.. | objects | select(has("dial")) | .dial] | any(test("^sandbox-\\{http\\.regexp\\.story\\.1\\}:3002$"))' /tmp/arena-adapted.json
jq -e '[.. | objects | select(has("dial")) | .dial] | any(test("^sandbox-\\{http\\.regexp\\.user\\.1\\}:7681$"))' /tmp/arena-adapted.json
```

All four `jq -e` calls must print `true` and exit 0. Repeat against
`Caddyfile.dev` (drop `-e CF_API_TOKEN=...`, `tls internal` doesn't need
it) before every change to either file. The `header_regexp`-presence
checks are the ones that actually catch the bug above — the `dial`
placeholder string looks identical (`sandbox-{http.regexp.user.1}:7681`)
whether or not the matcher is wired up; only the *absence of a `match`
key* on the terminal route gives it away.

## 8. Second box (documented, not built)

v1 is single-host by design (see the design doc's "Failure modes":
"Box undersized on event day" is an accepted v1 risk). If an event's
roster genuinely won't fit on one box, the mitigation is a second,
independent stack, not a horizontally-scaled one:

1. Provision a second host, repeat sections 1–4 on it in full (its own
   Docker, its own gVisor, its own `.env`, its own pulled sandbox image).
2. Give it its own DNS name under the same zone, e.g. an
   `arena2.openstory.work` A record plus a `*.arena2.openstory.work`
   CNAME, with its own Cloudflare DNS-edit token (or the same token, since
   it's zone-scoped, not host-scoped).
3. Split the roster or the event manifest across boxes — e.g. run two
   manifests with disjoint rosters, one `arena up` per host. There is no
   cross-host coordination: each box has its own SQLite, its own LiteLLM
   spend tracking against its own virtual keys, its own Postgres. Point
   participants at whichever box their `join_code`/roster entry lives on.
4. Tear down independently per box with `arena down` on each.

This is exactly the seam the design doc calls out as "the first thing k3s
would buy us later" — a real orchestrator would let one control plane
schedule sandboxes across hosts. v1 doesn't have that; two independent
stacks is the whole procedure.

## 9. Security

Two scripts in `arena/tests/` exercise the seal end-to-end against a real
running stack — they drive actual HTTP requests and actual `docker exec`,
not unit-test doubles:

- **`arena/tests/e2e.sh`** (`just arena-e2e`) — registers two participants
  via join-code `/register`, launches both sandboxes, polls until each
  participant's terminal and `-story` dashboard answer through Caddy, and
  confirms an anonymous request is redirected and the second participant
  is denied the first's sandbox (403). Read the comment block at the top
  of the script for exactly how it resolves `*.arena.test` hostnames
  without touching `/etc/hosts` (`curl --resolve`, no sudo needed).
- **`arena/tests/redteam.sh`** (`just arena-redteam`) — the standing
  seal probes, run from *inside* a launched sandbox via `docker exec`:
  the Docker socket must be unreachable, another participant's sandbox
  must be unreachable by container DNS, direct internet egress (both to
  `api.anthropic.com` and to an arbitrary host) must fail, no real
  `sk-ant-...` key may be recoverable from the sandbox's own environment,
  and LiteLLM's admin endpoint (`/key/generate`) must refuse a request
  that doesn't carry the master key. Every probe is written so *failure*
  of the probed command is the passing outcome — a probe that succeeds is
  printed as `BREACH`, and the script exits non-zero if any probe breaches.

**Running both against a deployed host** (not local dev): export
`ARENA_BASE_DOMAIN` to the real domain (already in DNS, no `--resolve`
tricks needed — real DNS resolves it), make sure an event with a known
join code exists (`EVENT_JOIN_CODE`, or run `arena up` yourself and skip
the script's own attempt), and run:

```bash
ARENA_BASE_DOMAIN=arena.openstory.work EVENT_JOIN_CODE=<real-code> \
  bash arena/tests/e2e.sh
bash arena/tests/redteam.sh sandbox-<user1> sandbox-<user2>
```

A breach here is a design bug in the seal (docker_driver.rs's per-user
internal network, or the LiteLLM/Caddy wiring around it), not a test bug —
treat `BREACH:` lines as a stop-ship signal, not something to loosen the
probe to avoid.

**Red-team skill integration:** these two scripts are not yet wired into
`.claude/skills/red-team` — that skill isn't modified in this branch. See
`docs/BACKLOG.md` for a line item to add an Arena-aware hook there once the
skill's runner supports invoking an external compose-stack script rather
than only its in-repo Rust/dependency probes.
