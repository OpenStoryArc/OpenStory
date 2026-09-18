//! MCP prompts — the intelligence instruction, rendered per datum
//! (memory hands, requirement E-01).
//!
//! OpenStory never calls a model. What it can do is ship, for one node,
//! the exact instruction a host should apply (what to produce, in what
//! shape, under what budget, under what laws) together with that node's
//! context, in one `prompts/get` payload. The stream hand names these
//! prompts on every closed arc, so a host with nothing but the wire can
//! act. `prompts/list` is static; `prompts/get` reads the store.

use crate::server::Server;
use crate::subscription::Subscribe;
use serde_json::{json, Value};

pub struct PromptDef {
    pub name: &'static str,
    pub description: &'static str,
    /// (name, description, required)
    pub arguments: &'static [(&'static str, &'static str, bool)],
}

const HANDLE_ARG: (&str, &str, bool) = (
    "handle",
    "Arc or exchange handle (16 hex, or a 4+ char prefix)",
    true,
);
const SESSION_ARG: (&str, &str, bool) = (
    "session_id",
    "Session the handle lives in (recommended)",
    false,
);

pub const PROMPTS: &[PromptDef] = &[
    PromptDef {
        name: "narrate_arc",
        description: "Produce the enrichment for a closed arc: title, question, resolution, summary, slots. Serves needs=enrich.",
        arguments: &[HANDLE_ARG, SESSION_ARG],
    },
    PromptDef {
        name: "segment_arc",
        description: "Produce the final reading of a closed arc: a partition of its exchange handles into paragraphs with an intent each. Serves needs=read after close.",
        arguments: &[HANDLE_ARG, SESSION_ARG],
    },
    PromptDef {
        name: "read_exchange",
        description: "One step of the streaming fold: given a closed exchange and the provisional reading so far, decide Continue or Break and update the intent. Serves needs=read live.",
        arguments: &[HANDLE_ARG, SESSION_ARG, ("reading", "The provisional reading so far, as JSON (optional until memory.* lands)", false)],
    },
    PromptDef {
        name: "adjudicate_seam",
        description: "Rule SameTheme or NewTheme on an ambiguous seam inside an arc, with a reason. Serves needs=adjudicate.",
        arguments: &[HANDLE_ARG, SESSION_ARG, ("seam", "Exchange index (into the arc's exchanges) the seam sits before", true)],
    },
    PromptDef {
        name: "remember",
        description: "Answer a creator question by traversal under a token budget, reporting tokens spent.",
        arguments: &[("question", "The creator's question", true), SESSION_ARG],
    },
    PromptDef {
        name: "curate",
        description: "Propose which arcs of a session are worth keeping past the retention cliff.",
        arguments: &[("session_id", "Session to curate", true)],
    },
];

pub fn prompts_list_result() -> Value {
    let prompts: Vec<Value> = PROMPTS
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "description": p.description,
                "arguments": p.arguments.iter().map(|(n, d, r)| json!({ "name": n, "description": d, "required": r })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "prompts": prompts })
}

const LAWS: &str = "Laws (enforced by the write hand, so obey them or the write is rejected):\n\
- Every handle you output must appear in the context below. Never invent a handle; a reading is a grouping over the exchange handles you were given, never a new one.\n\
- Stamp your output with author: { host, model }.\n\
- A final reading supersedes a provisional one; it never deletes it.\n\
- Nothing you produce changes the skeleton. Handles are content addresses and stay put.";

fn narrate_instruction() -> String {
    format!(
        "You are the Narrator for one closed arc of a coding session. Read the context (the arc node, its exchanges in order, and its ancestors and siblings) and produce ONE enrichment as JSON:\n\
{{ \"handle\": <the arc handle>, \"title\": <one line>, \"question\": <what opened the arc>, \"resolution\": <what closed it: a decision, an artifact, a deferral>,\n  \"summary\": <3-5 sentences>, \"slots\": {{ \"decisions\": [..], \"deferrals\": [..], \"tradeoffs\": [..], \"failures\": [..], \"stance\": [..] }},\n  \"author\": {{ \"host\": <your host>, \"model\": <your model id> }} }}\n\
Budget: the context is about 1,000 tokens; answer in under 400. Quote the human's words for stance; never paraphrase a stance into something stronger.\n\n{LAWS}"
    )
}

fn segment_instruction() -> String {
    format!(
        "Produce the FINAL reading of this closed arc: group its exchange handles, in order, into paragraphs that each share one intent. Output JSON:\n\
{{ \"handle\": <arc handle>, \"standing\": \"final\", \"paragraphs\": [ {{ \"exchanges\": [<handle>, ...], \"intent\": <one line> }} ],\n  \"author\": {{ \"host\": ..., \"model\": ... }} }}\n\
Every exchange handle in the context appears in exactly one paragraph; keep the arc's order; use hindsight: a shift that turns out to answer the previous question is the same paragraph.\n\n{LAWS}"
    )
}

fn read_instruction(reading: Option<&str>) -> String {
    let so_far = reading.unwrap_or("(none yet: this is the first exchange of the arc)");
    format!(
        "One step of the streaming fold. You are given one closed exchange and the PROVISIONAL reading so far:\n{so_far}\n\n\
Decide: does this exchange continue the open paragraph's intent (Continue) or start a new one (Break)? Output JSON:\n\
{{ \"handle\": <exchange handle>, \"decision\": \"Continue\" | \"Break\", \"intent\": <the open paragraph's intent, updated>, \"standing\": \"provisional\",\n  \"author\": {{ \"host\": ..., \"model\": ... }} }}\n\
You have no lookahead; the final reading will supersede yours when the arc closes.\n\n{LAWS}"
    )
}

fn adjudicate_instruction(seam: &str) -> String {
    format!(
        "An ambiguous seam sits before exchange index {seam} of this arc: a closure verb met an opening verb on new entities inside the gap threshold. Rule whether the two sides are one theme. Output JSON:\n\
{{ \"handle\": <arc handle>, \"seam\": {seam}, \"verdict\": \"SameTheme\" | \"NewTheme\", \"reason\": <one or two sentences citing the exchanges>,\n  \"author\": {{ \"host\": ..., \"model\": ... }} }}\n\n{LAWS}"
    )
}

fn remember_instruction(question: &str) -> String {
    format!(
        "Answer this creator question by traversal, carrying handles rather than transcripts:\n  {question}\n\
Procedure: story_search (or story_list when you know the session) -> story_summary on the best handle -> story_descend or story_context one level at a time -> stop when answered.\n\
Budget: about 1,400 tokens of tool results in total. Report at the end: {{ \"answer\": ..., \"handles_visited\": [..], \"tokens_spent\": <your estimate> }}.\n\
Cite handles and event ids you actually saw; never invent one."
    )
}

fn curate_instruction(session_id: &str) -> String {
    format!(
        "Session {session_id} will age out of the live store. From its arcs (context below), propose which are worth keeping by hand: the ones that produced a decision, an artifact, a deferral someone will need, or a stance. Output JSON:\n\
{{ \"session_id\": \"{session_id}\", \"keep\": [ {{ \"handle\": <arc handle>, \"reason\": <one line> }} ], \"author\": {{ \"host\": ..., \"model\": ... }} }}\n\
You mark; the human keeps.\n\n{LAWS}"
    )
}

fn text_message(text: String) -> Value {
    json!({ "role": "user", "content": { "type": "text", "text": text } })
}

fn resource_message(uri: String, context: &Value) -> Value {
    json!({ "role": "user", "content": { "type": "resource", "resource": {
        "uri": uri, "mimeType": "application/json", "text": serde_json::to_string_pretty(context).unwrap_or_default() } } })
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

/// Render one prompt for one node. Reads the store for the node's context.
pub async fn render<S: Subscribe>(
    server: &Server<S>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let Some(def) = PROMPTS.iter().find(|p| p.name == name) else {
        return Err(format!("unknown prompt `{name}`"));
    };
    for (n, _, required) in def.arguments {
        if *required && arg(args, n).is_none() {
            return Err(format!("prompt `{name}` requires `{n}`"));
        }
    }
    let session = arg(args, "session_id");
    let context_for = |node: &str| {
        let mut a = json!({ "node": node });
        if let Some(s) = session {
            a["session_id"] = json!(s);
        }
        a
    };

    let (instruction, context, uri) = match name {
        "narrate_arc" | "segment_arc" | "adjudicate_seam" | "read_exchange" => {
            let handle = arg(args, "handle").unwrap_or("");
            // The node with its neighbours, plus what lies beneath it: for an
            // arc, its exchanges in order (the handles a reading groups).
            let context =
                crate::tools::memory::story_context(&server.store, context_for(handle)).await?;
            let beneath =
                crate::tools::memory::story_descend(&server.store, context_for(handle)).await?;
            let ctx = json!({ "context": context, "beneath": beneath });
            let instruction = match name {
                "narrate_arc" => narrate_instruction(),
                "segment_arc" => segment_instruction(),
                "adjudicate_seam" => adjudicate_instruction(arg(args, "seam").unwrap_or("?")),
                _ => read_instruction(arg(args, "reading")),
            };
            (
                instruction,
                Some(ctx),
                format!("openstory://story/context/{handle}"),
            )
        }
        "remember" => (
            remember_instruction(arg(args, "question").unwrap_or("")),
            None,
            String::new(),
        ),
        "curate" => {
            let sid = arg(args, "session_id").unwrap_or("");
            let ctx = crate::tools::memory::story_list(&server.store, json!({ "session_id": sid }))
                .await?;
            (
                curate_instruction(sid),
                Some(ctx),
                format!("openstory://story/list/{sid}"),
            )
        }
        _ => unreachable!("catalog and match are in sync"),
    };

    let mut messages = vec![text_message(instruction)];
    if let Some(ctx) = context {
        messages.push(resource_message(uri, &ctx));
    }
    Ok(json!({ "description": def.description, "messages": messages }))
}
