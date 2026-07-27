//! `openstory_help` — in-band body schema for agents that never call resources/read.
//!
//! Pure: no store I/O. Routes need/topic → motions, tool cards, resource URIs.

use serde_json::{json, Value};

use crate::protocol::{
    AGENT_IN_UI_URI, EXAMPLE_FILE_LOCUS_URI, EXAMPLE_PICKUP_URI, EXAMPLE_SHOW_HUMAN_URI, HANDS_URI,
    PHYSICS_URI,
};

pub fn openstory_help_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "need": {
                "type": "string",
                "description": "Motion: orient | what-touched | find | cost | live | show-human | stuck"
            },
            "topic": {
                "type": "string",
                "description": "hands | physics | ui | motions | a tool name (e.g. session_story) | free text"
            }
        },
        "additionalProperties": false
    })
}

/// Pure help router.
pub fn openstory_help(args: Value) -> Result<Value, String> {
    let need = args
        .get("need")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let topic = args
        .get("topic")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let text = if !need.is_empty() {
        motion_card(&need)
    } else if !topic.is_empty() {
        topic_card(&topic)
    } else {
        overview()
    };

    Ok(json!({
        "text": text,
        "resources": [
            HANDS_URI,
            PHYSICS_URI,
            AGENT_IN_UI_URI,
            EXAMPLE_PICKUP_URI,
            EXAMPLE_FILE_LOCUS_URI,
            EXAMPLE_SHOW_HUMAN_URI,
        ],
    }))
}

fn overview() -> String {
    format!(
        r#"# OpenStory help

LAW: history tools are read-only. Cite session_id / paths / event ids. Do not invent events.
Sentences are SVO projections of acts — not intent labels. ui_control steers the dashboard only (ui.*).

## Motions
| need | tools |
|------|--------|
| orient | list_sessions → session_synopsis \| session_story |
| what-touched | file_impact \| tool_journey \| session_sentences |
| find | search \| agent_search → session_story |
| cost | token_usage \| daily_token_usage |
| live | subscribe_session \| subscribe_tokens |
| show-human | navigate_to (any event/graph) → where_is_user; ui_control low-level |

Call again with need="orient" (etc.) or topic="session_sentences" / "physics" / "ui".

## Resources
- {HANDS_URI}
- {PHYSICS_URI}
- {AGENT_IN_UI_URI}
- examples: {EXAMPLE_PICKUP_URI}, {EXAMPLE_FILE_LOCUS_URI}, {EXAMPLE_SHOW_HUMAN_URI}
"#
    )
}

fn motion_card(need: &str) -> String {
    match need {
        "orient" | "resume" | "pickup" => format!(
            r#"# Motion: orient
1. list_sessions {{ days?, project?, limit? }}
2. session_story {{ session_id }}  — or session_synopsis first if you only need overview
3. Optional: session_sentences, tool_journey, session_errors

Example resource: {EXAMPLE_PICKUP_URI}
Full curriculum: {HANDS_URI}"#
        ),
        "what-touched" | "file" | "files" | "locus" => format!(
            r#"# Motion: what-touched
1. file_impact {{ session_id }}  and/or tool_journey {{ session_id }}
2. session_sentences {{ session_id }}  — SVO coordinates (projections, not intent)
3. Across fleet: search / agent_search with a path or symbol

Example: {EXAMPLE_FILE_LOCUS_URI}
Physics limits: {PHYSICS_URI}"#
        ),
        "find" | "search" => format!(
            r#"# Motion: find
1. agent_search {{ query, limit? }}  — sessions ranked by hit
   or search {{ query, session_id?, limit? }}  — raw FTS
2. session_story {{ session_id }} on the best hit
3. session_errors if hunting failures

Curriculum: {HANDS_URI}"#
        ),
        "cost" | "tokens" | "spend" => r#"# Motion: cost
1. daily_token_usage { days: 7 }
2. token_usage { session_id? , days? , model? }
Includes cache fields when present. Read-only analytics."#
            .to_string(),
        "live" | "stream" | "watch" => r#"# Motion: live
1. subscribe_session { session_id }  — CloudEvents as they land
2. subscribe_tokens { session_id }   — running token tally
Cancel via your client's notifications/cancelled. Does not rewrite history."#
            .to_string(),
        "show-human" | "ui" | "dashboard" | "present" => format!(
            r#"# Motion: show-human (attention layer)
PRIMARY HAND — navigate_to (click parity):
  {{ kind:\"event\", id:EVENT, sessionId:SES, details:true }}
  {{ kind:\"session\", id:SES, canvasMode:\"gantt\" }}  // open chart + click session
  {{ kind:\"canvas\", id:\"canvas\", canvasMode:\"sunburst\" }}
  {{ kind:\"file\", id:\"path.rs\" }}

1. where_is_user {{}} — prefer driving when idle (tempo.rest)
2. navigate_to {{…}}  (or low-level ui_control)
3. where_is_user again to confirm

Example: {EXAMPLE_SHOW_HUMAN_URI}
Full map: {AGENT_IN_UI_URI}"#
        ),
        "stuck" | "help" => overview(),
        other => format!(
            "Unknown need {other:?}. Use: orient | what-touched | find | cost | live | show-human\n\n{}",
            overview()
        ),
    }
}

fn topic_card(topic: &str) -> String {
    match topic {
        "hands" | "curriculum" => {
            format!("Read resource {HANDS_URI} (resources/read). Summary:\n\n{}", overview())
        }
        "physics" | "ground" | "facts" => format!(
            r#"# Physics (summary)
events → turns → outcomes → sentences (SVO projections).
Cite tool fields; do not invent. Soft holes: no turn boundaries → no sentences
(use session_activity / transcript); Bash verbs softer than Write/Edit.
Full: resources/read {PHYSICS_URI}"#
        ),
        "ui" | "agent-in-ui" | "control" => format!(
            r#"# Dashboard control
ui_control actions: open_view | focus_event | present | query | toggle | set
where_is_user / subscribe_ui_state follow the human.
Sovereignty: ui.* only — never mutates events.*.
Full: resources/read {AGENT_IN_UI_URI}
Example: {EXAMPLE_SHOW_HUMAN_URI}"#
        ),
        "motions" => overview(),
        "session_story" => r#"# session_story
WHEN: orient on one session — best single fact sheet
MOTION: orient
CALL: { session_id }
RETURNS: record types, tool histogram, pattern counts, sample sentences, prompts (physics projections)
NEXT: session_sentences | tool_journey | session_transcript
LIMITS: not an LLM summary of intent"#
            .to_string(),
        "session_sentences" => format!(
            r#"# session_sentences
WHEN: turn-level SVO coordinates
MOTION: what-touched / orient
CALL: {{ session_id }}
RETURNS: verb/object/human_prompt from turn.sentence patterns (projections)
NEXT: session_story | session_patterns
LIMITS: empty if no turn boundaries — see {PHYSICS_URI}; not intent labels"#
        ),
        "session_synopsis" => r#"# session_synopsis
WHEN: quick overview of one session
MOTION: orient
CALL: { session_id }
RETURNS: counts, time range, top tools
NEXT: session_story for richer fact sheet"#
            .to_string(),
        "list_sessions" => r#"# list_sessions
WHEN: find sessions to inspect
MOTION: orient
CALL: { days?, project?, limit?, after? }
RETURNS: trim rows (id, label, project, times, event_count)
NEXT: session_story | session_synopsis"#
            .to_string(),
        "file_impact" => r#"# file_impact
WHEN: which files were read/written in a session
MOTION: what-touched
CALL: { session_id }
RETURNS: per-file read/write counts (outcome-grounded analytics)
NEXT: session_sentences | search across fleet"#
            .to_string(),
        "tool_journey" => r#"# tool_journey
WHEN: chronological tool sequence
MOTION: what-touched
CALL: { session_id }
RETURNS: { tool, file, timestamp } entries
NEXT: file_impact | session_story"#
            .to_string(),
        "search" | "agent_search" => r#"# search / agent_search
WHEN: find across fleet history
MOTION: find
search: raw FTS hits. agent_search: group by session, ranked.
NEXT: session_story on hit session_ids"#
            .to_string(),
        "ui_control" => motion_card("show-human"),
        "where_is_user" => r#"# where_is_user
WHEN: follow the human before driving UI
MOTION: show-human
CALL: {} (no args)
RETURNS: present, view, session_id?, event_id?, filters?, summary, …
NEXT: ui_control from that context; subscribe_ui_state for live follow"#
            .to_string(),
        "token_usage" | "daily_token_usage" => motion_card("cost"),
        "subscribe_session" | "subscribe_tokens" => motion_card("live"),
        other if other.contains("session") || other.contains("project") || other.contains("token") => {
            format!(
                "No dedicated card for {other:?}. Try need=orient|what-touched|find|cost or resources/read {HANDS_URI}"
            )
        }
        other => format!(
            "Unknown topic {other:?}. Try need=… or topic=hands|physics|ui|session_story|session_sentences\n\n{}",
            overview()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_overview_mentions_motions() {
        let v = openstory_help(json!({})).unwrap();
        let t = v["text"].as_str().unwrap();
        assert!(t.contains("orient"));
        assert!(t.contains("session_story"));
    }

    #[test]
    fn help_need_orient_points_at_story() {
        let v = openstory_help(json!({"need": "orient"})).unwrap();
        let t = v["text"].as_str().unwrap();
        assert!(t.contains("session_story"));
        assert!(t.contains("list_sessions"));
    }

    #[test]
    fn help_topic_sentences_mentions_projection_limits() {
        let v = openstory_help(json!({"topic": "session_sentences"})).unwrap();
        let t = v["text"].as_str().unwrap();
        assert!(t.contains("projection") || t.contains("LIMITS"));
    }
}
