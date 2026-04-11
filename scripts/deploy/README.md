# scripts/deploy/

Bash scripts that drive deploys of the OpenStory + OpenClaw + Telegram bot
stack to a remote VPS over SSH. Every script here was distilled from a real
deploy session. The CLAUDE.md principle — **Scripts over rawdogging** —
applies: if a deploy step is worth running twice, it should be a committed
artifact, not a scrollback one-liner.

## Principle

> Each script is idempotent and read-only unless it isn't, in which case
> it's loud about it.

- `preflight.sh` and `smoke.sh` are strictly read-only.
- `backup.sh` writes a tarball but never touches the live volumes.
- `set-nats-env.sh` writes to `.env` and `deploy/nats-hub.conf` but is
  idempotent: rerunning it with an existing `NATS_LEAF_TOKEN` reuses the
  value rather than rotating it.
- `deploy.sh` composes the other scripts and does the actual stack swap
  (build + `docker compose up -d`). Not idempotent in effect — reruns will
  rebuild images — but safe to rerun.
- `rollback.sh` is **destructive**. It stops the stack and untars over
  `/var/lib/docker/volumes/openstory_*`. It prompts for `y/N` before
  extracting. Use with intent.

## Expected host state

The scripts assume a VPS with:

- Debian 13 or similar, Docker + compose plugin installed
- `~/openstory/` — a checkout of this repository on some branch
- `~/openclaw/` — optional checkout of the upstream openclaw source, used
  only to build the base `openclaw:latest` image. **Not** managed by these
  scripts.
- `openclaw:latest` image already built from `~/openclaw` (the base that
  `Dockerfile.openclaw` extends).
- `sudo` available without password for the deploy user (tar of volume
  directories needs it).
- `tailscale` installed and joined to a tailnet (for `tailscale ip -4`).
- `openssl`, `jq` (optional), `curl`.
- Named volumes: `openstory_openclaw-state`, `openstory_openclaw-workspace`,
  `openstory_os-data`, `openstory_nats-data`.
- A `~/openstory/.env` file with the service tokens (`ANTHROPIC_API_KEY`,
  `OPEN_STORY_API_TOKEN`, `TELEGRAM_BOT_TOKEN`, etc). This repo never
  produces those — they live on the VPS only.

## Scripts

| Script | Purpose | Mutates? |
|---|---|---|
| `preflight.sh VPS_HOST` | Read-only health check before deploy | no |
| `backup.sh VPS_HOST [TAG]` | Tarball the three data volumes | writes tarball only |
| `set-nats-env.sh VPS_HOST` | Generate/reuse NATS token, set tailscale IP, substitute into `nats-hub.conf` | writes `.env`, `deploy/nats-hub.conf` |
| `deploy.sh VPS_HOST BRANCH` | Full orchestrated deploy | yes (composes the others) |
| `smoke.sh VPS_HOST` | Post-deploy verification | no |
| `rollback.sh VPS_HOST [BACKUP_FILE] [BRANCH]` | Restore from tarball and roll back branch | **destructive** |

Each script supports `-h` / `--help` and uses `set -euo pipefail`.

## Ordering

The canonical deploy flow is:

```
preflight  ->  backup  ->  git checkout  ->  set-nats-env
           ->  build images  ->  compose up  ->  smoke
```

`deploy.sh` is that flow in one command:

```bash
scripts/deploy/deploy.sh deploy@<vps-host> feat/openclaw-mcp-deploy
```

You can also run the individual steps by hand when troubleshooting. That's
the whole point of the split — each step is reviewable in isolation.

## What these scripts do NOT do

- They do not rebuild the upstream `openclaw:latest` base image from
  `~/openclaw`. That repository has an independent history and usually
  lags by many commits. Rebuilding it is a separate change window:

  ```bash
  ssh deploy@VPS 'cd ~/openclaw && git pull && docker build -t openclaw:latest .'
  ```

  Then rerun `deploy.sh` to rebuild `openclaw-mcp:latest` on top.

- They do not provision a fresh VPS. See the existing
  `scripts/deploy-vps.sh` for the bare-metal installer.

- They do not touch secrets. The only secret these scripts create is
  `NATS_LEAF_TOKEN`, which is generated **on the VPS** with
  `openssl rand -hex 24` and never echoed to stdout.

- They do not push to or pull from GitHub. The VPS's git remote does that.

## Adding a new leaf node

Leaf nodes connect the hub NATS on the VPS via Tailscale.

1. Install NATS on the leaf machine.
2. On the VPS, read the token from `.env`:

   ```bash
   ssh deploy@VPS 'grep ^NATS_LEAF_TOKEN= ~/openstory/.env'
   ```

   (This is the only time you should see the token value.)
3. Copy `deploy/nats-leaf.conf` to the leaf machine and substitute the
   token and the VPS tailscale IP.
4. Start `nats-server -c nats-leaf.conf` on the leaf.
5. Confirm on the VPS: `docker logs openstory-nats-1 | grep LEAF`.

If a leaf is compromised, rotate: remove `NATS_LEAF_TOKEN=` from
`~/openstory/.env` on the VPS, rerun `set-nats-env.sh`, restart the stack,
and redistribute the new token to legitimate leaves. There is no
per-leaf credential today — the hub token is shared across all leaves.

## Why this lives here

CLAUDE.md says: "Scripts are artifacts: saved to `scripts/`, committed,
reusable, reviewable." These scripts were written after the first real
production deploy of `feat/openclaw-mcp-deploy`. The prior deploy was
rawdogged from the scrollback. This is the reusable version.

When the next deploy diverges (new volume, new image, new service), update
the script and commit the change with the deploy. Do not fork the flow in
shell history.
