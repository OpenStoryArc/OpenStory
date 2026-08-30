#!/usr/bin/env bash
set -u
# Seed workspace on first boot ($HOME is the per-user volume)
if [ ! -d "$HOME/workspace" ]; then
  cp -r /opt/workspace "$HOME/workspace"
fi
# Interpolate the skel README's {username}/{base_domain} placeholders with
# the real values now that ARENA_USERNAME/ARENA_BASE_DOMAIN are known (R9) —
# the seeded README otherwise shows literal braces instead of the working
# URL. Idempotent: once substituted the tokens are gone, so re-running this
# on a later boot is a no-op rather than a re-substitution.
if [ -f "$HOME/workspace/README.md" ]; then
  sed -i "s|{username}|${ARENA_USERNAME:-me}|g; s|{base_domain}|${ARENA_BASE_DOMAIN:-arena}|g" "$HOME/workspace/README.md"
fi
mkdir -p "$HOME/data" "$HOME/.claude/projects" "$HOME/.pi/agent/sessions" "$HOME/.local/bin" "$HOME/.scratch"
# Skip claude onboarding prompts, and wire the openstory MCP server into
# Claude Code's config (R6). Runs every boot, not gated on `[ ! -f ]`: a
# volume from before this MCP existed would have a ~/.claude.json missing
# mcpServers forever otherwise. Merge, don't clobber — read what's there
# (or start from {}), overlay mcpServers from the skel, keep every other
# existing key (theme, hasCompletedOnboarding, etc.) untouched.
if [ -f /opt/skel/mcp.json ]; then
  node -e 'const fs=require("fs"),p=process.env.HOME+"/.claude.json";
    const c=fs.existsSync(p)?JSON.parse(fs.readFileSync(p,"utf8")):{};
    const m=JSON.parse(fs.readFileSync("/opt/skel/mcp.json","utf8"));
    c.mcpServers=Object.assign({},c.mcpServers,m.mcpServers);
    if(c.hasCompletedOnboarding===undefined)c.hasCompletedOnboarding=true;
    if(c.theme===undefined)c.theme="dark";
    fs.writeFileSync(p,JSON.stringify(c));' \
    || echo "warn: openstory MCP config merge failed (~/.claude.json may be malformed)" >&2
elif [ ! -f "$HOME/.claude.json" ]; then
  printf '{"hasCompletedOnboarding": true, "theme": "dark"}' > "$HOME/.claude.json"
fi
# Seed the read-only permission allowlist on first boot ($HOME volume shadows
# whatever was baked under ~/.claude at build time, so this must run here).
if [ ! -f "$HOME/.claude/settings.json" ] && [ -f /opt/skel/claude-settings.json ]; then
  cp /opt/skel/claude-settings.json "$HOME/.claude/settings.json"
fi
# Git identity from the logged-in username so first commit works, attributed
# per-student (R5). Idempotent; global scope so it applies to any repo.
u="${ARENA_USERNAME:-dev}"
git config --global user.name "$u"
git config --global user.email "${u}@arena.local"
git config --global init.defaultBranch main
# Private OpenStory observing this sandbox's own history. Run under setsid so
# it lands in its own session, immune to SIGHUP when claude exits and tmux
# tears the pane down — open-story must keep observing across attach cycles,
# not die with the first client's terminal.
# OpenStory watches every agent harness in the box, not just Claude Code, so
# its store (and the MCP over it) spans all of them (R6). The pi-mono watch
# dir has no clap flag — it's read straight from the OPEN_STORY_PI_WATCH_DIR
# env var at runtime (see rs/cli/src/main.rs), so it's absent from
# `serve --help` even though it's fully wired up server-side.
if ! pgrep -f "open-story serve" >/dev/null; then
  setsid env OPEN_STORY_PORT=3002 OPEN_STORY_PI_WATCH_DIR="$HOME/.pi/agent/sessions" open-story serve \
    --watch-dir "$HOME/.claude/projects" \
    --data-dir "$HOME/data" \
    --manage-nats >>"$HOME/data/open-story.log" 2>&1 &
fi
# Wait for OpenStory (and its managed NATS) to be ready before launching the
# agent — the openstory MCP connects to NATS at startup and would die if it
# raced ahead of the server on a cold boot.
tries=0
until curl -sf -o /dev/null http://localhost:3002/api/sessions; do
  tries=$((tries+1)); [ "$tries" -ge 60 ] && { echo "warn: open-story not ready after 30s; launching anyway" >&2; break; }
  sleep 0.5
done
cd "$HOME/workspace"
echo "Welcome to Arena, ${ARENA_USERNAME:-friend}."
echo "Your agent history: https://${ARENA_USERNAME:-me}-story.${ARENA_BASE_DOMAIN:-arena}"
exec claude
