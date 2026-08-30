#!/usr/bin/env bash
set -u
# Seed workspace on first boot ($HOME is the per-user volume)
if [ ! -d "$HOME/workspace" ]; then
  cp -r /opt/workspace "$HOME/workspace"
fi
mkdir -p "$HOME/data" "$HOME/.claude/projects" "$HOME/.pi/agent/sessions" "$HOME/.local/bin" "$HOME/.scratch"
# Skip claude onboarding prompts
if [ ! -f "$HOME/.claude.json" ]; then
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
cd "$HOME/workspace"
echo "Welcome to Arena, ${ARENA_USERNAME:-friend}."
echo "Your agent history: https://${ARENA_USERNAME:-me}-story.${ARENA_BASE_DOMAIN:-arena}"
exec claude
