# Grok session seed — this OpenStory integration conversation

**Source session** (live Grok Build on Max's machine):

```
/Users/maxglassie/.grok/sessions/%2FUsers%2Fmaxglassie%2Fprojects%2Fgrok-build/019f6cb5-f7e4-7bc1-bb25-9985af59619e/updates.jsonl
```

- **session_id:** `019f6cb5-f7e4-7bc1-bb25-9985af59619e`
- **cwd (encoded):** `%2FUsers%2Fmaxglassie%2Fprojects%2Fgrok-build`
- **extracted:** 2026-07-16 (OpenStory Grok support implementation thread)
- **turns in source:** 15
- **truncation:** on (tool/text caps for golden size)

## Seed files

| File | Source turn | Prompt (abbrev) |
|------|-------------|-----------------|
| `real_turn_01_text_only.jsonl` | turn 1 | 'what is TUI?' |
| `real_turn_02_session_storage.jsonl` | turn 2 | "where does grok build keep it's session data? " |
| `real_turn_07_openstory_vs_grok.jsonl` | turn 7 | 'How does OpenStory relate to Grok Build? What is similar, what is diff' |
| `real_turn_09_acp_and_mcp.jsonl` | turn 9 | 'what is acp? and are you aware of the openstory mcp and can you connec' |

## Container / watcher tree

```
fixtures/grok/seed_tree/
  %2FUsers%2Fmaxglassie%2Fprojects%2Fgrok-build/
    019f6cb5-f7e4-7bc1-bb25-9985af59619e/
      updates.jsonl          ← concatenated seed turns
      chat_history.jsonl     ← noise (must be ignored by watcher filter)
```

Mount `seed_tree` as the Grok watch root (same shape as `~/.grok/sessions`).

Regenerate:

```
python3 scripts/extract_grok_session_seed.py
python3 scripts/extract_grok_session_seed.py --no-truncate   # full outputs
```
