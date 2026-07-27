# Example: resume work on a project

**Need:** orient  
**Tools only — no repo required.**

```jsonc
// 1. Recent sessions (adjust days / project)
{ "tool": "list_sessions", "args": { "days": 2, "limit": 20 } }

// 2. Fact sheet on the best match
{ "tool": "session_story", "args": { "session_id": "SESSION_ID" } }

// 3. Only if you need narrative coordinates per turn
{ "tool": "session_sentences", "args": { "session_id": "SESSION_ID" } }

// 4. Only if story is thin — rawer detail
{ "tool": "tool_journey", "args": { "session_id": "SESSION_ID" } }
{ "tool": "session_errors", "args": { "session_id": "SESSION_ID" } }
```

Report to the human: project, last labels, top tools/files, last prompts —
with `session_id` cited. Do not invent commits or file edits not in the data.
