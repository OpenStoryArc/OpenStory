#!/usr/bin/env bash
set -u
# Seed workspace on first boot ($HOME is the per-user volume)
if [ ! -d "$HOME/workspace" ]; then
  cp -r /opt/workspace "$HOME/workspace"
fi
mkdir -p "$HOME/data" "$HOME/.claude/projects"
# Skip claude onboarding prompts
if [ ! -f "$HOME/.claude.json" ]; then
  printf '{"hasCompletedOnboarding": true, "theme": "dark"}' > "$HOME/.claude.json"
fi
# Private OpenStory observing this sandbox's own history
if ! pgrep -f "open-story serve" >/dev/null; then
  OPEN_STORY_PORT=3002 open-story serve \
    --watch-dir "$HOME/.claude/projects" \
    --data-dir "$HOME/data" \
    --manage-nats >>"$HOME/data/open-story.log" 2>&1 &
fi
cd "$HOME/workspace"
echo "Welcome to Arena, ${ARENA_USERNAME:-friend}."
echo "Your agent history: https://${ARENA_USERNAME:-me}-story.${ARENA_BASE_DOMAIN:-arena}"
exec claude
