#!/bin/sh
# Entrypoint for the Claude runner container.
# Loads ANTHROPIC_API_KEY from Docker secret (preferred) or environment,
# then runs Claude Code headless with the provided prompt.

set -e

# Ensure transcript files are readable by Open Story (group-readable)
umask 0027

# Load API key from Docker secret if available
if [ -f /run/secrets/anthropic_api_key ]; then
    export ANTHROPIC_API_KEY=$(cat /run/secrets/anthropic_api_key)
fi

# Validate
if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "ERROR: ANTHROPIC_API_KEY not set." >&2
    echo "  Provide via Docker secret (file: .anthropic_api_key)" >&2
    echo "  or environment variable." >&2
    exit 1
fi

# Run Claude headless with all arguments as the prompt
exec claude -p "$@" --dangerously-skip-permissions --output-format json
