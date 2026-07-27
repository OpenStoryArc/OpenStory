# Example: what touched a file / path

**Need:** what-touched / find

```jsonc
// A) Known session
{ "tool": "file_impact", "args": { "session_id": "SESSION_ID" } }

// B) Across history
{ "tool": "agent_search", "args": { "query": "sentence.rs", "limit": 5 } }
// or
{ "tool": "search", "args": { "query": "sentence.rs", "limit": 10 } }

// Then on a hit session:
{ "tool": "session_sentences", "args": { "session_id": "SESSION_ID" } }
{ "tool": "session_story", "args": { "session_id": "SESSION_ID" } }
```

Optional — show the human the same locus:

```jsonc
{ "tool": "ui_control", "args": {
    "action": "open_view",
    "params": { "view": "explore", "detailView": "search", "searchQuery": "sentence.rs" }
}}
```

Cite paths and sessions. Empty `file_impact` means no measured file ops in that
session, not “the file was never important.”
