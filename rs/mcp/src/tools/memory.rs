//! Memory hands — read side over the story layer (requirements group B).
//!
//! These tools read `story.arc` / `story.exchange` patterns (folded by
//! `open_story_patterns::story::StoryDetector`) plus `turn.sentence`
//! patterns and raw events, all through the same read-only `EventStore`
//! the other query tools use. No new store handle, no writes.
//!
//! Handles are content addresses (16 hex). A handle names a node; nodes
//! are arcs, exchanges, sentences (handle derived from event ids), and
//! events (their own id). Prefixes of four or more characters resolve.

use open_story_patterns::story::{handle as content_handle, MemoryRecord};
use open_story_patterns::PatternEvent;
use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

/// How many sessions a session-less call scans, newest first.
const SESSION_SCAN_LIMIT: usize = 50;

pub fn story_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Restrict to one session (UUID)"},
            "limit": {"type": "integer", "minimum": 1, "description": "Max arcs returned (default 50)"}
        },
        "additionalProperties": false
    })
}

pub fn story_summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "handle": {"type": "string", "description": "Arc handle (16 hex) or a prefix of 4+ chars"},
            "session_id": {"type": "string", "description": "Session to look in (recommended; otherwise recent sessions are scanned)"}
        },
        "required": ["handle"],
        "additionalProperties": false
    })
}

fn meta_str<'a>(p: &'a PatternEvent, key: &str) -> Option<&'a str> {
    p.metadata.get(key).and_then(|v| v.as_str())
}

/// One line per arc: the handle a host carries in working memory.
fn arc_line(session_id: &str, arc: &PatternEvent) -> Value {
    let exchanges = arc
        .metadata
        .get("exchanges")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    json!({
        "handle": meta_str(arc, "handle"),
        "session_id": session_id,
        "arc_index": arc.metadata.get("arc_index"),
        "started_at": arc.started_at,
        "ended_at": arc.ended_at,
        "question": meta_str(arc, "question"),
        "exchanges": exchanges,
        "closed_by": meta_str(arc, "closed_by"),
        "ambiguous_seams": arc.metadata.get("ambiguous_seams").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
    })
}

fn truncate(s: &str, max: usize) -> String {
    open_story_core::strings::truncate_at_char_boundary(s, max).to_string()
}

/// Siblings a context carries: a window of this many on each side of the
/// node. A forty-exchange arc would otherwise ride along with every
/// exchange view (30 KB per call on the pilot's recall run).
const SIBLING_WINDOW: usize = 4;

/// Entities a summary carries: the most frequent few, with the total.
const ENTITY_CAP: usize = 12;

/// Pure: the slice of `siblings` within `SIBLING_WINDOW` of the entry whose
/// handle is `handle` (all of them when the handle is absent).
fn window_siblings(siblings: Vec<Value>, handle: &str) -> Vec<Value> {
    let pos = siblings
        .iter()
        .position(|s| s.get("handle").and_then(|h| h.as_str()) == Some(handle));
    match pos {
        Some(i) => {
            let start = i.saturating_sub(SIBLING_WINDOW);
            let end = (i + SIBLING_WINDOW + 1).min(siblings.len());
            siblings[start..end].to_vec()
        }
        None => siblings,
    }
}

/// Pure: the `ENTITY_CAP` most frequent entities (count desc, name asc)
/// and how many there were.
fn cap_entities(entities: Option<&Value>) -> (Value, usize) {
    let Some(map) = entities.and_then(|e| e.as_object()) else {
        return (json!({}), 0);
    };
    let mut pairs: Vec<(&String, u64)> = map
        .iter()
        .map(|(k, v)| (k, v.as_u64().unwrap_or(0)))
        .collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let kept: serde_json::Map<String, Value> = pairs
        .into_iter()
        .take(ENTITY_CAP)
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    (Value::Object(kept), map.len())
}

/// Sessions to scan for a call without `session_id`: newest first, capped.
async fn candidate_sessions(
    store: &Arc<dyn EventStore>,
    session_id: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(sid) = session_id {
        return Ok(vec![sid.to_string()]);
    }
    let mut rows = store
        .list_sessions()
        .await
        .map_err(|e| format!("list_sessions failed: {e}"))?;
    rows.sort_by(|a, b| b.last_event.cmp(&a.last_event));
    Ok(rows
        .into_iter()
        .take(SESSION_SCAN_LIMIT)
        .map(|r| r.id)
        .collect())
}

/// `story_list { session_id?, limit? }` → `[arc line]`
pub async fn story_list(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let mut lines = Vec::new();
    for sid in candidate_sessions(store, session_id).await? {
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .map_err(|e| format!("session_patterns failed: {e}"))?;
        for arc in &arcs {
            lines.push(arc_line(&sid, arc));
            if lines.len() >= limit {
                return Ok(Value::Array(lines));
            }
        }
    }
    Ok(Value::Array(lines))
}

/// A resolved node: which session it lives in and the pattern (or event) itself.
pub struct Found {
    pub session_id: String,
    pub pattern: PatternEvent,
}

/// Find arcs whose handle starts with `prefix` in the candidate sessions.
async fn find_arcs(
    store: &Arc<dyn EventStore>,
    prefix: &str,
    session_id: Option<&str>,
) -> Result<Vec<Found>, String> {
    if prefix.len() < 4 {
        return Err(format!(
            "handle prefix `{prefix}` is too short; give at least 4 characters"
        ));
    }
    let mut hits = Vec::new();
    for sid in candidate_sessions(store, session_id).await? {
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .map_err(|e| format!("session_patterns failed: {e}"))?;
        for arc in arcs {
            if meta_str(&arc, "handle")
                .map(|h| h.starts_with(prefix))
                .unwrap_or(false)
            {
                hits.push(Found {
                    session_id: sid.clone(),
                    pattern: arc,
                });
            }
        }
    }
    Ok(hits)
}

/// Exactly one arc for a prefix, or an error naming the candidates.
async fn resolve_arc(
    store: &Arc<dyn EventStore>,
    prefix: &str,
    session_id: Option<&str>,
) -> Result<Found, String> {
    let mut hits = find_arcs(store, prefix, session_id).await?;
    match hits.len() {
        0 => Err(format!("no arc matches handle `{prefix}`")),
        1 => Ok(hits.remove(0)),
        _ => {
            let candidates: Vec<String> = hits
                .iter()
                .map(|f| {
                    format!(
                        "arc {} in session {} ({})",
                        meta_str(&f.pattern, "handle").unwrap_or(""),
                        f.session_id,
                        truncate(meta_str(&f.pattern, "question").unwrap_or(""), 60)
                    )
                })
                .collect();
            Err(format!(
                "handle `{prefix}` is ambiguous; candidates: {}",
                candidates.join("; ")
            ))
        }
    }
}

/// `story_summary { handle, session_id? }` → the top-node payload.
///
/// Skeleton fields only. `title` and `slots` appear once an enrichment
/// exists for the handle (group D); today they are absent, not null.
pub async fn story_summary(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let prefix = args
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "story_summary requires `handle`".to_string())?;
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let found = resolve_arc(store, prefix, session_id).await?;
    let arc = &found.pattern;
    let handle_s = meta_str(arc, "handle").unwrap_or("");

    // Across: sibling arcs in the same session sharing an entity.
    let mine: std::collections::BTreeSet<String> = arc
        .metadata
        .get("entities")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let siblings = store
        .session_patterns(&found.session_id, Some("story.arc"))
        .await
        .map_err(|e| format!("session_patterns failed: {e}"))?;
    let across: Vec<Value> = siblings
        .iter()
        .filter(|s| meta_str(s, "handle") != Some(handle_s))
        .filter(|s| {
            s.metadata
                .get("entities")
                .and_then(|v| v.as_object())
                .map(|m| m.keys().any(|k| mine.contains(k)))
                .unwrap_or(false)
        })
        .map(|s| json!(meta_str(s, "handle")))
        .collect();

    let (entities, entities_total) = cap_entities(arc.metadata.get("entities"));
    let mut summary = json!({
        "handle": handle_s,
        "session_id": found.session_id,
        "arc_index": arc.metadata.get("arc_index"),
        "started_at": arc.started_at,
        "ended_at": arc.ended_at,
        "question": arc.metadata.get("question"),
        "resolution": arc.metadata.get("resolution"),
        "entities": entities,
        "entities_total": entities_total,
        "tools": arc.metadata.get("tools"),
        "closed_by": arc.metadata.get("closed_by"),
        "ambiguous_seams": arc.metadata.get("ambiguous_seams"),
        "down": arc.metadata.get("exchanges"),
        "across": across,
    });
    merge_memory(store, handle_s, &mut summary).await?;
    Ok(summary)
}

/// Content address for a sentence: derived from its event ids, the same
/// function arcs and exchanges use, so sentences can be addressed too.
pub fn sentence_handle(p: &PatternEvent) -> String {
    content_handle("sentence", &p.event_ids)
}

// ═══════════════════════════════════════════════════════════════════
// Nodes: arcs, exchanges, sentences, events — one address space
// ═══════════════════════════════════════════════════════════════════

/// Everything the story layer knows about one session, loaded once per call.
struct SessionStory {
    session_id: String,
    arcs: Vec<PatternEvent>,
    exchanges: Vec<PatternEvent>,
    sentences: Vec<PatternEvent>,
}

async fn load_story(store: &Arc<dyn EventStore>, sid: &str) -> Result<SessionStory, String> {
    let get = |t: &'static str| async move {
        store
            .session_patterns(sid, Some(t))
            .await
            .map_err(|e| format!("session_patterns({t}) failed: {e}"))
    };
    let arcs = get("story.arc").await?;
    let exchanges = get("story.exchange").await?;
    let mut sentences = get("turn.sentence").await?;
    sentences.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    Ok(SessionStory {
        session_id: sid.to_string(),
        arcs,
        exchanges,
        sentences,
    })
}

impl SessionStory {
    fn arc_by_handle(&self, h: &str) -> Option<&PatternEvent> {
        self.arcs.iter().find(|a| meta_str(a, "handle") == Some(h))
    }
    fn exchange_by_handle(&self, h: &str) -> Option<&PatternEvent> {
        self.exchanges
            .iter()
            .find(|e| meta_str(e, "handle") == Some(h))
    }
    fn arc_of_exchange(&self, h: &str) -> Option<&PatternEvent> {
        self.arcs.iter().find(|a| {
            a.metadata
                .get("exchanges")
                .and_then(|v| v.as_array())
                .map(|xs| xs.iter().any(|x| x.as_str() == Some(h)))
                .unwrap_or(false)
        })
    }
    fn exchange_containing(&self, event_id: &str) -> Option<&PatternEvent> {
        self.exchanges
            .iter()
            .find(|e| e.event_ids.iter().any(|id| id == event_id))
    }
    fn sentence_containing(&self, event_id: &str) -> Option<&PatternEvent> {
        self.sentences
            .iter()
            .find(|s| s.event_ids.iter().any(|id| id == event_id))
    }
    /// Sentences whose events all lie inside the exchange, in time order.
    fn sentences_of_exchange(&self, ex: &PatternEvent) -> Vec<&PatternEvent> {
        let ids: std::collections::HashSet<&str> =
            ex.event_ids.iter().map(String::as_str).collect();
        self.sentences
            .iter()
            .filter(|s| {
                !s.event_ids.is_empty() && s.event_ids.iter().all(|id| ids.contains(id.as_str()))
            })
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Arc,
    Exchange,
    Sentence,
    Event,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Arc => "arc",
            Kind::Exchange => "exchange",
            Kind::Sentence => "sentence",
            Kind::Event => "event",
        }
    }
}

/// A resolved node: its kind, session, and a compact view.
struct Node {
    kind: Kind,
    session_id: String,
    /// Arc / exchange / sentence handle, or the event id.
    id: String,
    view: Value,
}

fn exchange_view(sid: &str, ex: &PatternEvent) -> Value {
    json!({
        "kind": "exchange",
        "handle": meta_str(ex, "handle"),
        "session_id": sid,
        "started_at": ex.started_at,
        "ended_at": ex.ended_at,
        "user_prompt": meta_str(ex, "user_prompt"),
        "eval_result": meta_str(ex, "eval_result"),
        "turns": ex.metadata.get("turns"),
        "rich_turns": ex.metadata.get("rich_turns"),
        "injected_count": ex.metadata.get("injected_count"),
        "first_verb": ex.metadata.get("first_verb"),
        "last_verb": ex.metadata.get("last_verb"),
        "entities": ex.metadata.get("entities"),
        "tools": ex.metadata.get("tools"),
    })
}

fn arc_view(sid: &str, arc: &PatternEvent) -> Value {
    let mut v = arc_line(sid, arc);
    v["kind"] = json!("arc");
    v
}

fn sentence_view(sid: &str, s: &PatternEvent) -> Value {
    json!({
        "kind": "sentence",
        "handle": sentence_handle(s),
        "session_id": sid,
        "started_at": s.started_at,
        "one_liner": truncate(&s.summary, 160),
        "verb": s.metadata.get("verb"),
        "object": s.metadata.get("object"),
        "turn": s.metadata.get("turn"),
        "event_ids": s.event_ids,
    })
}

fn event_view(sid: &str, ev: &Value) -> Value {
    let text = ev
        .pointer("/data/agent_payload/text")
        .and_then(|v| v.as_str())
        .map(|t| truncate(t, 160));
    json!({
        "kind": "event",
        "id": ev.get("id"),
        "session_id": sid,
        "subtype": ev.get("subtype"),
        "time": ev.get("time"),
        "tool": ev.pointer("/data/agent_payload/tool"),
        "text": text,
    })
}

/// Resolve a node id (handle or event id, 4+ char prefix) to exactly one node.
/// Event ids are only searched when `session_id` is given: they need the
/// session's event stream, which is not scanned across sessions.
async fn resolve_node(
    store: &Arc<dyn EventStore>,
    prefix: &str,
    session_id: Option<&str>,
) -> Result<(Node, SessionStory), String> {
    if prefix.len() < 4 {
        return Err(format!(
            "node `{prefix}` is too short; give at least 4 characters"
        ));
    }
    let mut hits: Vec<(Node, SessionStory)> = Vec::new();
    for sid in candidate_sessions(store, session_id).await? {
        let story = load_story(store, &sid).await?;
        let mut found: Vec<Node> = Vec::new();
        for a in &story.arcs {
            if let Some(h) = meta_str(a, "handle").filter(|h| h.starts_with(prefix)) {
                found.push(Node {
                    kind: Kind::Arc,
                    session_id: sid.clone(),
                    id: h.to_string(),
                    view: arc_view(&sid, a),
                });
            }
        }
        for e in &story.exchanges {
            if let Some(h) = meta_str(e, "handle").filter(|h| h.starts_with(prefix)) {
                found.push(Node {
                    kind: Kind::Exchange,
                    session_id: sid.clone(),
                    id: h.to_string(),
                    view: exchange_view(&sid, e),
                });
            }
        }
        for s in &story.sentences {
            let h = sentence_handle(s);
            if h.starts_with(prefix) {
                found.push(Node {
                    kind: Kind::Sentence,
                    session_id: sid.clone(),
                    id: h,
                    view: sentence_view(&sid, s),
                });
            }
        }
        if session_id.is_some() && found.is_empty() {
            let events = store
                .session_events(&sid)
                .await
                .map_err(|e| format!("session_events failed: {e}"))?;
            for ev in &events {
                if let Some(id) = ev
                    .get("id")
                    .and_then(|v| v.as_str())
                    .filter(|id| id.starts_with(prefix))
                {
                    found.push(Node {
                        kind: Kind::Event,
                        session_id: sid.clone(),
                        id: id.to_string(),
                        view: event_view(&sid, ev),
                    });
                }
            }
        }
        // Each hit needs its own loaded story; clone the story per hit only if several.
        if found.len() == 1 {
            hits.push((found.remove(0), story));
        } else {
            for n in found {
                hits.push((
                    n,
                    SessionStory {
                        session_id: story.session_id.clone(),
                        arcs: story.arcs.clone(),
                        exchanges: story.exchanges.clone(),
                        sentences: story.sentences.clone(),
                    },
                ));
            }
        }
    }
    match hits.len() {
        0 => Err(format!("no node matches `{prefix}`")),
        1 => Ok(hits.remove(0)),
        _ => {
            let candidates: Vec<String> = hits
                .iter()
                .map(|(n, _)| format!("{} {} in session {}", n.kind.name(), n.id, n.session_id))
                .collect();
            Err(format!(
                "`{prefix}` is ambiguous; candidates: {}",
                candidates.join("; ")
            ))
        }
    }
}

fn node_args(args: &Value) -> Result<(&str, Option<&str>), String> {
    let node = args
        .get("node")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "requires `node` (a handle or event id, 4+ char prefix)".to_string())?;
    Ok((node, args.get("session_id").and_then(|v| v.as_str())))
}

pub fn node_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "node": {"type": "string", "description": "Arc/exchange/sentence handle or event id; 4+ char prefix resolves"},
            "session_id": {"type": "string", "description": "Session to look in (required to address events)"}
        },
        "required": ["node"],
        "additionalProperties": false
    })
}

/// Children of a node: exchanges of an arc, sentences of an exchange, events of a sentence.
async fn children(
    store: &Arc<dyn EventStore>,
    node: &Node,
    story: &SessionStory,
) -> Result<Vec<Value>, String> {
    let sid = &story.session_id;
    Ok(match node.kind {
        Kind::Arc => {
            let arc = story.arc_by_handle(&node.id).ok_or("arc vanished")?;
            arc.metadata
                .get("exchanges")
                .and_then(|v| v.as_array())
                .map(|hs| {
                    hs.iter()
                        .filter_map(|h| h.as_str())
                        .filter_map(|h| story.exchange_by_handle(h))
                        .map(|e| exchange_view(sid, e))
                        .collect()
                })
                .unwrap_or_default()
        }
        Kind::Exchange => {
            let ex = story
                .exchange_by_handle(&node.id)
                .ok_or("exchange vanished")?;
            story
                .sentences_of_exchange(ex)
                .into_iter()
                .map(|s| sentence_view(sid, s))
                .collect()
        }
        Kind::Sentence => {
            let s = story
                .sentences
                .iter()
                .find(|s| sentence_handle(s) == node.id)
                .ok_or("sentence vanished")?;
            let wanted: std::collections::HashSet<&str> =
                s.event_ids.iter().map(String::as_str).collect();
            let events = store
                .session_events(sid)
                .await
                .map_err(|e| format!("session_events failed: {e}"))?;
            events
                .iter()
                .filter(|ev| {
                    ev.get("id")
                        .and_then(|v| v.as_str())
                        .map(|id| wanted.contains(id))
                        .unwrap_or(false)
                })
                .map(|ev| event_view(sid, ev))
                .collect()
        }
        Kind::Event => Vec::new(),
    })
}

/// Ancestors of a node, nearest first, up to its arc.
fn ancestors(node: &Node, story: &SessionStory) -> Vec<Value> {
    let sid = &story.session_id;
    let mut out = Vec::new();
    let exchange_handle = match node.kind {
        Kind::Arc => None,
        Kind::Exchange => Some(node.id.clone()),
        Kind::Sentence => {
            let s = story
                .sentences
                .iter()
                .find(|s| sentence_handle(s) == node.id);
            s.and_then(|s| s.event_ids.first())
                .and_then(|id| story.exchange_containing(id))
                .and_then(|e| meta_str(e, "handle"))
                .map(String::from)
        }
        Kind::Event => {
            if let Some(s) = story.sentence_containing(&node.id) {
                out.push(sentence_view(sid, s));
            }
            story
                .exchange_containing(&node.id)
                .and_then(|e| meta_str(e, "handle"))
                .map(String::from)
        }
    };
    if let Some(h) = exchange_handle {
        if node.kind != Kind::Exchange {
            if let Some(e) = story.exchange_by_handle(&h) {
                out.push(exchange_view(sid, e));
            }
        }
        if let Some(a) = story.arc_of_exchange(&h) {
            out.push(arc_view(sid, a));
        }
    }
    out
}

/// `story_descend { node, session_id? }` → children.
pub async fn story_descend(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let (prefix, sid) = node_args(&args)?;
    let (node, story) = resolve_node(store, prefix, sid).await?;
    Ok(Value::Array(children(store, &node, &story).await?))
}

/// `story_surface { node, session_id? }` → ancestors, nearest first.
pub async fn story_surface(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let (prefix, sid) = node_args(&args)?;
    let (node, story) = resolve_node(store, prefix, sid).await?;
    Ok(Value::Array(ancestors(&node, &story)))
}

/// `story_context { node, session_id? }` → the node with its ancestors and
/// its siblings (the children of its parent). Never a naked node.
pub async fn story_context(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let (prefix, sid) = node_args(&args)?;
    let (node, story) = resolve_node(store, prefix, sid).await?;
    let up = ancestors(&node, &story);
    let siblings = match up.first() {
        Some(parent) => {
            let parent_id = parent
                .get("handle")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = match parent["kind"].as_str() {
                Some("arc") => Kind::Arc,
                Some("exchange") => Kind::Exchange,
                _ => Kind::Sentence,
            };
            let parent_node = Node {
                kind,
                session_id: story.session_id.clone(),
                id: parent_id,
                view: parent.clone(),
            };
            children(store, &parent_node, &story).await?
        }
        None => story
            .arcs
            .iter()
            .map(|a| arc_view(&story.session_id, a))
            .collect(),
    };
    let siblings_total = siblings.len();
    let siblings = window_siblings(siblings, &node.id);
    Ok(json!({
        "node": node.view,
        "ancestors": up,
        "siblings": siblings,
        "siblings_total": siblings_total,
    }))
}

// ═══════════════════════════════════════════════════════════════════
// story_search · story_related
// ═══════════════════════════════════════════════════════════════════

pub fn story_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Case-insensitive substring over arc questions/resolutions/entities and exchange prompts/results"},
            "session_id": {"type": "string", "description": "Restrict to one session; otherwise the newest 50 sessions are scanned"},
            "limit": {"type": "integer", "minimum": 1, "description": "Max hits (default 20)"}
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

pub fn story_related_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "handle": {"type": "string", "description": "Arc handle (16 hex) or 4+ char prefix"},
            "session_id": {"type": "string", "description": "Session of the arc (recommended)"},
            "limit": {"type": "integer", "minimum": 1, "description": "Max related arcs (default 10)"}
        },
        "required": ["handle"],
        "additionalProperties": false
    })
}

fn matches(hay: Option<&str>, needle: &str) -> bool {
    hay.map(|h| h.to_lowercase().contains(needle))
        .unwrap_or(false)
}

fn entity_keys(p: &PatternEvent) -> Vec<String> {
    p.metadata
        .get("entities")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// `story_search { query, session_id?, limit? }` → hits, arcs before
/// exchanges, more matched fields first. Substring search over the story
/// layer's own text; a handle is what comes back, not content.
pub async fn story_search(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty())
        .ok_or_else(|| "story_search requires a non-empty `query`".to_string())?;
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    // B-11: the candidate set is the whole store unless a session is named.
    // Memory records count too: what one host narrated is how the next
    // host finds the arc.
    let (patterns, records): (Vec<PatternEvent>, Vec<MemoryRecord>) = match session_id {
        Some(sid) => {
            let story = load_story(store, sid).await?;
            let mut ps = story.arcs.clone();
            ps.extend(story.exchanges.iter().cloned());
            let rs = store
                .session_memory(sid)
                .await
                .map_err(|e| format!("session_memory failed: {e}"))?
                .into_iter()
                .filter(|r| r.payload.to_string().to_lowercase().contains(&query))
                .collect();
            (ps, rs)
        }
        None => {
            let ps = store
                .search_story(&query, limit.saturating_mul(4).max(50))
                .await
                .map_err(|e| format!("search_story failed: {e}"))?;
            let rs = store
                .search_memory(&query, limit)
                .await
                .map_err(|e| format!("search_memory failed: {e}"))?;
            (ps, rs)
        }
    };

    let mut hits: Vec<(usize, u8, Value)> = Vec::new(); // (matched count, kind rank, view)
    for p in &patterns {
        let sid = p.session_id.as_str();
        match p.pattern_type.as_str() {
            "story.arc" => {
                let mut matched = Vec::new();
                if matches(meta_str(p, "question"), &query) {
                    matched.push("question");
                }
                if matches(meta_str(p, "resolution"), &query) {
                    matched.push("resolution");
                }
                if entity_keys(p)
                    .iter()
                    .any(|k| k.to_lowercase().contains(&query))
                {
                    matched.push("entities");
                }
                if !matched.is_empty() {
                    hits.push((
                        matched.len(),
                        0,
                        json!({
                            "kind": "arc",
                            "handle": meta_str(p, "handle"),
                            "session_id": sid,
                            "question": meta_str(p, "question"),
                            "matched": matched,
                        }),
                    ));
                }
            }
            "story.exchange" => {
                let mut matched = Vec::new();
                if matches(meta_str(p, "user_prompt"), &query) {
                    matched.push("user_prompt");
                }
                if matches(meta_str(p, "eval_result"), &query) {
                    matched.push("eval_result");
                }
                if entity_keys(p)
                    .iter()
                    .any(|k| k.to_lowercase().contains(&query))
                {
                    matched.push("entities");
                }
                if !matched.is_empty() {
                    hits.push((
                        matched.len(),
                        1,
                        json!({
                            "kind": "exchange",
                            "handle": meta_str(p, "handle"),
                            "session_id": sid,
                            "user_prompt": meta_str(p, "user_prompt"),
                            "matched": matched,
                        }),
                    ));
                }
            }
            _ => {}
        }
    }
    for r in &records {
        let title = r
            .payload
            .get("title")
            .and_then(|t| t.as_str())
            .map(|t| t.to_string());
        if let Some(hit) = hits.iter_mut().find(|(_, _, v)| v["handle"] == r.handle) {
            if let Some(m) = hit.2["matched"].as_array_mut() {
                if !m.iter().any(|x| x == "memory") {
                    m.push(json!("memory"));
                    hit.0 += 1;
                }
            }
            if title.is_some() && hit.2.get("title").is_none() {
                hit.2["title"] = json!(title);
            }
        } else {
            hits.push((
                1,
                0,
                json!({
                    "kind": "arc",
                    "handle": r.handle,
                    "session_id": r.session_id,
                    "title": title,
                    "matched": ["memory"],
                }),
            ));
        }
    }
    hits.sort_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
    Ok(Value::Array(
        hits.into_iter().take(limit).map(|(_, _, v)| v).collect(),
    ))
}

/// `story_related { handle, session_id?, limit? }` → arcs sharing entities
/// with the given arc, most shared first. Scans the candidate sessions.
pub async fn story_related(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let prefix = args
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "story_related requires `handle`".to_string())?;
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let found = resolve_arc(store, prefix, session_id).await?;
    let mine_handle = meta_str(&found.pattern, "handle").unwrap_or("").to_string();
    let mine: std::collections::BTreeSet<String> =
        entity_keys(&found.pattern).into_iter().collect();
    if mine.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }

    let mut scored: Vec<(Vec<String>, Value)> = Vec::new();
    // The arc's own session first, then the newest sessions: pointers
    // across reach other sessions whether or not the caller named one
    // (the session id locates the arc, it does not fence the search).
    let mut sessions = vec![found.session_id.clone()];
    for sid in candidate_sessions(store, None).await? {
        if sid != found.session_id {
            sessions.push(sid);
        }
    }
    for sid in sessions {
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .map_err(|e| format!("session_patterns failed: {e}"))?;
        for a in &arcs {
            if meta_str(a, "handle") == Some(mine_handle.as_str()) {
                continue;
            }
            let shared: Vec<String> = entity_keys(a)
                .into_iter()
                .filter(|k| mine.contains(k))
                .collect();
            if !shared.is_empty() {
                scored.push((
                    shared.clone(),
                    json!({
                        "handle": meta_str(a, "handle"),
                        "session_id": sid,
                        "question": meta_str(a, "question"),
                        "shared": shared,
                    }),
                ));
            }
        }
    }
    scored.sort_by_key(|(shared, _)| std::cmp::Reverse(shared.len()));
    Ok(Value::Array(
        scored.into_iter().take(limit).map(|(_, v)| v).collect(),
    ))
}

// ═══════════════════════════════════════════════════════════════════
// D-05: what hosts wrote about a node, merged into its summary
// ═══════════════════════════════════════════════════════════════════

/// Merge memory records for `handle` into a summary: `title` and `slots`
/// from the earliest enrichment, then `enrichments`, `readings`,
/// `verdicts`, `sagas`, `keeps` — each author-stamped. Fields stay absent
/// when nothing was written, so a host can tell "unread" from "empty".
async fn merge_memory(
    store: &Arc<dyn EventStore>,
    handle: &str,
    summary: &mut Value,
) -> Result<(), String> {
    let records = store
        .memory_for_handle(handle)
        .await
        .map_err(|e| format!("memory_for_handle failed: {e}"))?;
    if records.is_empty() {
        return Ok(());
    }
    use open_story_patterns::story::MemoryKind;
    let mut enrichments = Vec::new();
    let mut readings = Vec::new();
    let mut verdicts = Vec::new();
    let mut sagas = Vec::new();
    let mut keeps = Vec::new();
    for r in &records {
        let mut item = r.payload.clone();
        if let Some(obj) = item.as_object_mut() {
            obj.insert(
                "author".into(),
                serde_json::to_value(&r.author).unwrap_or(Value::Null),
            );
            obj.insert("created_at".into(), json!(r.created_at));
            if let Some(st) = r.standing {
                obj.insert(
                    "standing".into(),
                    serde_json::to_value(st).unwrap_or(Value::Null),
                );
            }
        }
        match r.kind {
            MemoryKind::Enrichment => enrichments.push(item),
            MemoryKind::Reading => readings.push(item),
            MemoryKind::Verdict => verdicts.push(item),
            MemoryKind::Saga => sagas.push(item),
            MemoryKind::Keep => keeps.push(item),
        }
    }
    if let Some(first) = enrichments.first() {
        if let Some(t) = first.get("title") {
            summary["title"] = t.clone();
        }
        if let Some(sl) = first.get("slots") {
            summary["slots"] = sl.clone();
        }
    }
    for (key, items) in [
        ("enrichments", enrichments),
        ("readings", readings),
        ("verdicts", verdicts),
        ("sagas", sagas),
        ("keeps", keeps),
    ] {
        if !items.is_empty() {
            summary[key] = Value::Array(items);
        }
    }
    Ok(())
}

/// Text fields a caller's `width` clips. Views carry whole text by default;
/// a host with a budget asks for a width and gets every one of these
/// clipped at a char boundary, recursively, nothing else touched.
const WIDTH_FIELDS: &[&str] = &[
    "user_prompt",
    "eval_result",
    "question",
    "resolution",
    "title",
    "summary",
];

/// Pure: the `width` a call asked for, if any.
pub fn width_of(args: &Value) -> Option<usize> {
    args.get("width")
        .and_then(|w| w.as_u64())
        .map(|w| w as usize)
}

/// Pure: `value` with every `WIDTH_FIELDS` string clipped to `width`.
pub fn clip_view(value: Value, width: Option<usize>) -> Value {
    let Some(w) = width else { return value };
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::String(s) if WIDTH_FIELDS.contains(&k.as_str()) => {
                            Value::String(truncate(&s, w))
                        }
                        other => clip_view(other, width),
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| clip_view(v, width)).collect())
        }
        other => other,
    }
}
