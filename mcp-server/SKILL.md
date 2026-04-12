---
name: openstory
description: Query your own coding sessions via the OpenStory MCP tools — find past work, understand what happened, check token usage, search across history. All read-only observability, never interferes with your execution.
---

# OpenStory — self-awareness via your own session history

OpenStory observes every session you run. You can query that history in real time through the `openstory` MCP server, which exposes 19 read-only tools. All data is local to this machine.

## When to use these tools

- **User asks "what did you do earlier?" or "what happened in session X?"** → `session_story`, `session_synopsis`, `session_transcript`
- **User asks about a file** ("have I touched auth.rs recently?") → `file_impact`, `recent_files`
- **User asks about token spend or cost** → `token_usage`, `daily_token_usage`
- **User wants to find something across history** ("that bug with the translator") → `search`, `agent_search`
- **User asks "what am I working on?"** → `list_sessions`, `project_context`, `project_pulse`
- **You're picking up where a previous session left off** → `session_story` on the most recent session for the project
- **You hit an error that looks familiar** → `session_errors` to see if it's recurred before

## Key tools

| Tool | Purpose |
|------|---------|
| `list_sessions` | All sessions with metadata (session_id, label, project, start/end, token count) |
| `session_synopsis` | Structured overview: duration, tool histogram, file list, error count |
| `session_story` | Fact sheet with prompts, tool sequences, eval-apply patterns, verbatim sentences |
| `session_activity` | Detailed event list for a session (files read, tools called, errors) |
| `session_transcript` | Chronological conversation transcript (assistant text, user prompts) |
| `session_sentences` | Compressed "what was said" narrative — the high-level story of a turn |
| `session_plans` | Plans extracted from plan mode during a session |
| `session_errors` | Error records with timestamps |
| `session_patterns` | Detected patterns (eval-apply cycles, sentences, etc.) |
| `tool_journey` | Chronological sequence of tool calls |
| `file_impact` | Which files were read/written and how many times |
| `search` | Full-text search across all event content (FTS5) |
| `agent_search` | Natural-language search with relevance ranking |
| `project_context` | Recent sessions for a given project |
| `project_pulse` | Activity summary across all projects |
| `recent_files` | Recently-modified files in a project |
| `token_usage` | Input/output/cache token counts and estimated cost (per session, time range, or model) |
| `daily_token_usage` | Daily token usage trends |
| `productivity` | Activity grouped by hour of day |

## Important conventions

- **All tools are read-only.** OpenStory observes, it never mutates.
- **Session IDs are UUIDs** derived from Claude Code's JSONL filenames — they're globally unique.
- **Prefer MCP tools over curl.** The tools call the same REST API under the hood but give you structured data without parsing.
- **Before guessing, query.** If the user asks about something you *could* know from history, look it up instead of guessing from memory.
- **When picking up from a past session**, call `session_story` first to get the full fact sheet, then drill into specific tools (`tool_journey`, `session_transcript`) if you need detail.

## Example flow

User: "Remind me what I was working on yesterday in OpenStory."

1. `list_sessions` → filter to project "OpenStory", sort by recent
2. `session_story` on the most recent session → get the structured story
3. Report back: "You were working on X, committed Y, and stopped at Z"

User: "How much have I spent this week?"

1. `daily_token_usage` with `days=7` → get daily breakdown
2. Report totals and trend

User: "Have I ever hit this error before?"

1. `search` with the error message
2. If hits: summarize the past occurrences and how they were resolved
