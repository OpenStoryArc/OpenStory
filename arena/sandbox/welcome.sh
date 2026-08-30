#!/usr/bin/env bash
set -u
# Seed workspace on first boot ($HOME is the per-user volume)
if [ ! -d "$HOME/workspace" ]; then
  cp -r /opt/workspace "$HOME/workspace"
fi
mkdir -p "$HOME/data" "$HOME/.claude/projects" "$HOME/.local/bin" "$HOME/.scratch"
# Skip claude onboarding prompts
if [ ! -f "$HOME/.claude.json" ]; then
  printf '{"hasCompletedOnboarding": true, "theme": "dark"}' > "$HOME/.claude.json"
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
if ! pgrep -f "open-story serve" >/dev/null; then
  setsid env OPEN_STORY_PORT=3002 open-story serve \
    --watch-dir "$HOME/.claude/projects" \
    --data-dir "$HOME/data" \
    --manage-nats >>"$HOME/data/open-story.log" 2>&1 &
fi
cd "$HOME/workspace"
echo "Welcome to Arena, ${ARENA_USERNAME:-friend}."
echo "Your agent history: https://${ARENA_USERNAME:-me}-story.${ARENA_BASE_DOMAIN:-arena}"
exec claude
