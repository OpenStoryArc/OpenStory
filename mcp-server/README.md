# OpenStory MCP Server

Read-only MCP server that wraps the OpenStory REST API, giving coding agents
self-awareness about their own operational history.

## Setup

```bash
cd mcp-server
uv sync
```

## Usage

### Claude Code integration

Add to your `.claude/settings.json` (project or global):

```json
{
  "mcpServers": {
    "openstory": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/OpenStory/mcp-server", "python", "server.py"],
      "env": {
        "OPENSTORY_URL": "http://localhost:3002"
      }
    }
  }
}
```

### Standalone

```bash
# stdio transport (default)
uv run python server.py

# SSE transport
uv run python server.py --sse

# Self-test
uv run python server.py --test
```

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSTORY_URL` | `http://localhost:3002` | OpenStory server base URL |
| `OPENSTORY_API_TOKEN` | (empty) | Bearer token for authenticated APIs |

## Tools (17)

| Tool | Description |
|------|-------------|
| `list_sessions` | List all sessions with metadata |
| `session_synopsis` | Structured overview of a session |
| `session_activity` | Detailed activity (files, tools, errors) |
| `tool_journey` | Chronological tool call sequence |
| `file_impact` | File read/write counts |
| `session_errors` | Error records with timestamps |
| `session_patterns` | Detected patterns (eval-apply, sentences, etc.) |
| `search` | Full-text search across all events |
| `agent_search` | Natural-language search with relevance ranking |
| `project_context` | Recent sessions for a project |
| `recent_files` | Recently modified files in a project |
| `project_pulse` | Activity summary across all projects |
| `token_usage` | Token counts and cost estimates |
| `daily_token_usage` | Daily token usage trends |
| `productivity` | Activity by hour of day |
| `session_transcript` | Conversation transcript |
| `session_plans` | Plans created during a session |

## Principles

- **Read-only.** No tools that create, update, or delete data.
- **Observe, never interfere.** Matches the OpenStory core principle.
- **Thin adapter.** All logic lives in the REST API; this is just a bridge.
