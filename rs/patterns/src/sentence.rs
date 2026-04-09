//! SentenceDetector — turns each eval-apply step into a natural language sentence.
//!
//! Layer 5 of the five-layer model:
//!   Layer 5: Sentence     "Claude wrote 8 Scheme files after reading 6 sources"
//!   Layer 4: Domain       +3 created, ~1 modified, 2 cmd ok (ToolOutcome)
//!   Layer 3: Structure    eval → apply × 12 → CONTINUE (EvalApplyDetector)
//!   Layer 2: Events       CloudEvents
//!   Layer 1: Raw          JSONL transcript bytes
//!
//! Each StructuralTurn becomes a sentence with grammatical structure:
//!   Subject (Claude) + Verb (action) + Object (what)
//!   + Adverbial (why — from human message)
//!   + Subordinate clauses (supporting actions)
//!   + Predicate (outcome: answered or continued)
//!
//! Prototype source: docs/research/eval-apply-prototype/sentence.ts (20 tests)

use crate::eval_apply::{ApplyRecord, StructuralTurn};
use crate::{PatternEvent, TurnDetector};

// ═══════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════

/// How a tool serves the turn's narrative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolRole {
    Preparatory,  // Read, Grep, Glob, WebSearch — research before acting
    Creative,     // Write, Edit, git commit — producing artifacts
    Verificatory, // test, build, ls — checking work
    Delegatory,   // Agent — handing off
    Interactive,  // AskUser, ToolSearch — coordination
}

/// A subordinate clause: "after reading 6 sources"
#[derive(Debug, Clone)]
pub struct SubordinateClause {
    pub role: ToolRole,
    pub verb: &'static str,
    pub object: String,
    pub tool_calls: usize,
}

/// The complete sentence for one turn.
#[derive(Debug, Clone)]
pub struct TurnSentence {
    pub subject: String,
    pub verb: String,
    pub object: String,
    pub adverbial: Option<String>,
    pub subordinates: Vec<SubordinateClause>,
    pub predicate: String,
    pub one_liner: String,
}

// ═══════════════════════════════════════════════════════════════════
// Tool classification — pure function
// ═══════════════════════════════════════════════════════════════════

const PREPARATORY_TOOLS: &[&str] = &["Read", "Grep", "Glob", "WebSearch", "WebFetch"];
const CREATIVE_TOOLS: &[&str] = &["Write", "Edit"];
const DELEGATORY_TOOLS: &[&str] = &["Agent"];
const INTERACTIVE_TOOLS: &[&str] = &["AskUserQuestion", "ToolSearch", "ExitPlanMode"];

/// Classify a tool call by its role in the turn's narrative.
/// Prototype source: sentence.ts:48-66
pub fn classify_tool(name: &str, input: &str) -> ToolRole {
    if PREPARATORY_TOOLS.contains(&name) {
        return ToolRole::Preparatory;
    }
    if CREATIVE_TOOLS.contains(&name) {
        return ToolRole::Creative;
    }
    if DELEGATORY_TOOLS.contains(&name) {
        return ToolRole::Delegatory;
    }
    if INTERACTIVE_TOOLS.contains(&name) {
        return ToolRole::Interactive;
    }

    // Bash depends on the command
    if name == "Bash" {
        let lower = input.to_lowercase();
        if regex_match_any(&lower, &["test", "spec", "jest", "vitest", "pytest", "cargo test", "npm test"]) {
            return ToolRole::Verificatory;
        }
        if regex_match_any(&lower, &["install", "brew ", "apt ", "npm i ", "pip "]) {
            return ToolRole::Preparatory;
        }
        if regex_match_any(&lower, &["git commit", "git push", "git add", "git tag"]) {
            return ToolRole::Creative;
        }
        if regex_match_any(&lower, &["git status", "git log", "git diff", "git branch"]) {
            return ToolRole::Preparatory;
        }
        if regex_match_any(&lower, &["mkdir", "chmod", "cp ", "mv "]) {
            return ToolRole::Creative;
        }
        return ToolRole::Verificatory; // default for Bash: checking something
    }

    ToolRole::Verificatory
}

fn regex_match_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

// ═══════════════════════════════════════════════════════════════════
// Sentence building — pure functions
// ═══════════════════════════════════════════════════════════════════

/// Build a sentence from a StructuralTurn.
/// Prototype source: sentence.ts:72-109
pub fn build_sentence(turn: &StructuralTurn) -> TurnSentence {
    let subject = "Claude".to_string();

    // Classify all tools
    let classified: Vec<(&ApplyRecord, ToolRole)> = turn
        .applies
        .iter()
        .map(|a| (a, classify_tool(&a.tool_name, &a.input_summary)))
        .collect();

    // Group by role
    let by_role = group_by_role(&classified);

    // Determine dominant role and verb
    let (verb, object) = extract_verb_and_object(turn, &classified, &by_role);

    // Extract adverbial from human message
    let adverbial = turn.human.as_ref().and_then(|h| {
        let content = h.content.trim();
        if content.is_empty() {
            None
        } else {
            let short = if content.len() > 60 {
                format!("{}...", truncate_str(content, 57))
            } else {
                content.to_string()
            };
            Some(format!("\"{short}\""))
        }
    });

    // Build subordinate clauses (non-dominant roles)
    let dominant_role = get_dominant_role(&by_role);
    let subordinates = build_subordinates(&by_role, dominant_role);

    // Predicate
    let predicate = if turn.is_terminal {
        "answered".to_string()
    } else {
        "continued".to_string()
    };

    // Compose one-liner
    let one_liner = compose_one_liner(&subject, &verb, &object, &adverbial, &subordinates, &predicate);

    TurnSentence {
        subject,
        verb,
        object,
        adverbial,
        subordinates,
        predicate,
        one_liner,
    }
}

fn extract_verb_and_object(
    turn: &StructuralTurn,
    classified: &[(&ApplyRecord, ToolRole)],
    by_role: &std::collections::HashMap<ToolRole, Vec<&ApplyRecord>>,
) -> (String, String) {
    // No tools → pure response
    if classified.is_empty() {
        return infer_from_eval(turn);
    }

    let creative = by_role.get(&ToolRole::Creative).map(|v| v.as_slice()).unwrap_or(&[]);
    let preparatory = by_role.get(&ToolRole::Preparatory).map(|v| v.as_slice()).unwrap_or(&[]);
    let verificatory = by_role.get(&ToolRole::Verificatory).map(|v| v.as_slice()).unwrap_or(&[]);
    let delegatory = by_role.get(&ToolRole::Delegatory).map(|v| v.as_slice()).unwrap_or(&[]);

    if !creative.is_empty() {
        return extract_creative_verb(creative);
    }
    if !delegatory.is_empty() {
        return extract_delegatory_verb(delegatory);
    }
    if !preparatory.is_empty() && verificatory.is_empty() {
        return extract_preparatory_verb(preparatory);
    }
    if !verificatory.is_empty() {
        return extract_verificatory_verb(verificatory, preparatory);
    }

    ("worked on".to_string(), summarize_inputs(classified.iter().map(|(a, _)| *a).collect()))
}

fn infer_from_eval(turn: &StructuralTurn) -> (String, String) {
    let content = turn.eval.as_ref().map(|e| e.content.as_str()).unwrap_or("");

    if content.len() > 20 {
        return ("explained".to_string(), extract_topic(content));
    }

    ("answered".to_string(), String::new())
}

fn extract_creative_verb(creative: &[&ApplyRecord]) -> (String, String) {
    let writes: Vec<&&ApplyRecord> = creative.iter().filter(|a| a.tool_name == "Write").collect();
    let edits: Vec<&&ApplyRecord> = creative.iter().filter(|a| a.tool_name == "Edit").collect();
    let commits: Vec<&&ApplyRecord> = creative
        .iter()
        .filter(|a| a.input_summary.contains("git commit") || a.input_summary.contains("git push"))
        .collect();

    if !commits.is_empty() {
        return ("committed".to_string(), "changes".to_string());
    }
    if !writes.is_empty() {
        return ("wrote".to_string(), summarize_files(writes.iter().map(|a| &a.input_summary).collect()));
    }
    if !edits.is_empty() {
        return ("edited".to_string(), summarize_files(edits.iter().map(|a| &a.input_summary).collect()));
    }
    ("created".to_string(), summarize_inputs(creative.to_vec()))
}

fn extract_delegatory_verb(delegatory: &[&ApplyRecord]) -> (String, String) {
    if delegatory.len() == 1 {
        let desc = &delegatory[0].input_summary;
        return ("delegated".to_string(), if desc.is_empty() { "a sub-task".to_string() } else { desc.clone() });
    }
    ("delegated".to_string(), format!("{} sub-tasks", delegatory.len()))
}

fn extract_preparatory_verb(preparatory: &[&ApplyRecord]) -> (String, String) {
    let reads: Vec<&&ApplyRecord> = preparatory.iter().filter(|a| a.tool_name == "Read").collect();
    let greps: Vec<&&ApplyRecord> = preparatory.iter().filter(|a| a.tool_name == "Grep").collect();

    if !greps.is_empty() {
        return ("searched for".to_string(), greps[0].input_summary.clone());
    }
    if !reads.is_empty() {
        return ("read".to_string(), summarize_files(reads.iter().map(|a| &a.input_summary).collect()));
    }
    ("explored".to_string(), summarize_inputs(preparatory.to_vec()))
}

fn extract_verificatory_verb(
    verificatory: &[&ApplyRecord],
    preparatory: &[&ApplyRecord],
) -> (String, String) {
    let tests: Vec<&&ApplyRecord> = verificatory
        .iter()
        .filter(|a| {
            let lower = a.input_summary.to_lowercase();
            lower.contains("test") || lower.contains("spec") || lower.contains("jest") || lower.contains("vitest")
        })
        .collect();
    if !tests.is_empty() {
        let cmd = &tests[0].input_summary;
        let short = truncate_str(cmd, 60);
        return ("ran tests".to_string(), short.to_string());
    }
    if preparatory.len() > verificatory.len() {
        return extract_preparatory_verb(preparatory);
    }
    ("checked".to_string(), summarize_inputs(verificatory.to_vec()))
}

// ═══════════════════════════════════════════════════════════════════
// Subordinate clauses
// ═══════════════════════════════════════════════════════════════════

const ROLE_ORDER: &[ToolRole] = &[
    ToolRole::Preparatory,
    ToolRole::Creative,
    ToolRole::Verificatory,
    ToolRole::Delegatory,
    ToolRole::Interactive,
];

fn role_verb(role: ToolRole) -> &'static str {
    match role {
        ToolRole::Preparatory => "after reading",
        ToolRole::Creative => "writing",
        ToolRole::Verificatory => "while testing",
        ToolRole::Delegatory => "by delegating to",
        ToolRole::Interactive => "asking about",
    }
}

fn build_subordinates(
    by_role: &std::collections::HashMap<ToolRole, Vec<&ApplyRecord>>,
    dominant: Option<ToolRole>,
) -> Vec<SubordinateClause> {
    let mut clauses = Vec::new();
    for &role in ROLE_ORDER {
        if Some(role) == dominant {
            continue;
        }
        if let Some(tools) = by_role.get(&role) {
            if tools.is_empty() {
                continue;
            }
            clauses.push(SubordinateClause {
                role,
                verb: role_verb(role),
                object: summarize_for_clause(role, tools),
                tool_calls: tools.len(),
            });
        }
    }
    clauses
}

fn summarize_for_clause(role: ToolRole, tools: &[&ApplyRecord]) -> String {
    match role {
        ToolRole::Preparatory => {
            let reads: Vec<&&ApplyRecord> = tools.iter().filter(|a| a.tool_name == "Read").collect();
            if !reads.is_empty() {
                return summarize_files(reads.iter().map(|a| &a.input_summary).collect());
            }
            format!("{} source{}", tools.len(), if tools.len() > 1 { "s" } else { "" })
        }
        ToolRole::Creative => summarize_files(tools.iter().map(|a| &a.input_summary).collect()),
        ToolRole::Verificatory => format!("{} check{}", tools.len(), if tools.len() > 1 { "s" } else { "" }),
        ToolRole::Delegatory => tools.first().map(|a| a.input_summary.clone()).unwrap_or_else(|| "a sub-agent".to_string()),
        ToolRole::Interactive => format!("{} interaction{}", tools.len(), if tools.len() > 1 { "s" } else { "" }),
    }
}

// ═══════════════════════════════════════════════════════════════════
// One-liner composition
// ═══════════════════════════════════════════════════════════════════

fn compose_one_liner(
    subject: &str,
    verb: &str,
    object: &str,
    adverbial: &Option<String>,
    subordinates: &[SubordinateClause],
    predicate: &str,
) -> String {
    let mut parts = vec![if object.is_empty() {
        format!("{subject} {verb}")
    } else {
        format!("{subject} {verb} {object}")
    }];

    // Add subordinate clauses (max 2 for brevity)
    for clause in subordinates.iter().take(2) {
        parts.push(format!("{} {}", clause.verb, clause.object));
    }

    if let Some(adv) = adverbial {
        parts.push(format!("because {adv}"));
    }

    format!("{} \u{2192} {predicate}", parts.join(", "))
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn get_dominant_role(
    by_role: &std::collections::HashMap<ToolRole, Vec<&ApplyRecord>>,
) -> Option<ToolRole> {
    if by_role.get(&ToolRole::Creative).map(|v| !v.is_empty()).unwrap_or(false) {
        return Some(ToolRole::Creative);
    }
    if by_role.get(&ToolRole::Delegatory).map(|v| !v.is_empty()).unwrap_or(false) {
        return Some(ToolRole::Delegatory);
    }
    let has_prep = by_role.get(&ToolRole::Preparatory).map(|v| !v.is_empty()).unwrap_or(false);
    let has_verif = by_role.get(&ToolRole::Verificatory).map(|v| !v.is_empty()).unwrap_or(false);
    if has_prep && !has_verif {
        return Some(ToolRole::Preparatory);
    }
    if has_verif {
        return Some(ToolRole::Verificatory);
    }
    None
}

fn group_by_role<'a>(
    classified: &[(&'a ApplyRecord, ToolRole)],
) -> std::collections::HashMap<ToolRole, Vec<&'a ApplyRecord>> {
    let mut map = std::collections::HashMap::new();
    for (apply, role) in classified {
        map.entry(*role).or_insert_with(Vec::new).push(*apply);
    }
    map
}

fn summarize_files(paths: Vec<&String>) -> String {
    let names: Vec<&str> = paths
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.rsplit('/').next().unwrap_or(p)
        })
        .collect();

    if names.is_empty() {
        return "files".to_string();
    }
    if names.len() == 1 {
        return names[0].to_string();
    }
    if names.len() <= 3 {
        return names.join(", ");
    }

    // Count extensions to detect dominant type
    let mut ext_counts = std::collections::HashMap::new();
    for name in &names {
        if let Some(dot) = name.rfind('.') {
            let ext = &name[dot..];
            *ext_counts.entry(ext).or_insert(0) += 1;
        }
    }
    if let Some((&ext, &count)) = ext_counts.iter().max_by_key(|(_, c)| **c) {
        if count > names.len() / 2 {
            let lang = ext_to_language(ext);
            return format!("{} {} files", names.len(), lang);
        }
    }
    format!("{} files", names.len())
}

fn summarize_inputs(applies: Vec<&ApplyRecord>) -> String {
    if applies.is_empty() {
        return String::new();
    }
    if applies.len() == 1 {
        let s = &applies[0].input_summary;
        if s.len() > 60 { format!("{}...", truncate_str(s, 57)) } else { s.clone() }
    } else {
        format!("{} operations", applies.len())
    }
}

fn extract_topic(content: &str) -> String {
    let cleaned = content.to_lowercase();
    let cleaned = cleaned.trim_start_matches(|c: char| !c.is_alphanumeric());
    if cleaned.len() > 50 {
        format!("{}...", truncate_str(cleaned, 47))
    } else {
        cleaned.to_string()
    }
}

/// Truncate a string to at most `max_bytes` bytes at a char boundary.
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn ext_to_language(ext: &str) -> &str {
    match ext {
        ".scm" => "Scheme",
        ".ts" => "TypeScript",
        ".js" => "JavaScript",
        ".rs" => "Rust",
        ".py" => "Python",
        ".md" => "Markdown",
        ".html" => "HTML",
        ".css" => "CSS",
        ".json" => "JSON",
        ".toml" => "TOML",
        ".yaml" => "YAML",
        ".sh" => "shell",
        _ => "source",
    }
}

// ═══════════════════════════════════════════════════════════════════
// SentenceDetector — TurnDetector implementation
// ═══════════════════════════════════════════════════════════════════

pub struct SentenceDetector;

impl Default for SentenceDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SentenceDetector {
    pub fn new() -> Self {
        Self
    }
}

impl TurnDetector for SentenceDetector {
    fn feed_turn(&mut self, turn: &StructuralTurn) -> Vec<PatternEvent> {
        let sentence = build_sentence(turn);

        vec![PatternEvent {
            pattern_type: "turn.sentence".to_string(),
            session_id: turn.session_id.clone(),
            event_ids: turn.event_ids.clone(),
            started_at: turn.timestamp.clone(),
            ended_at: turn.timestamp.clone(),
            summary: sentence.one_liner.clone(),
            metadata: serde_json::json!({
                // Sentence grammar
                "turn": turn.turn_number,
                "subject": sentence.subject,
                "verb": sentence.verb,
                "object": sentence.object,
                "adverbial": sentence.adverbial,
                "predicate": sentence.predicate,
                "subordinates": sentence.subordinates.iter().map(|s| {
                    serde_json::json!({
                        "role": format!("{:?}", s.role),
                        "verb": s.verb,
                        "object": s.object,
                        "tool_calls": s.tool_calls,
                    })
                }).collect::<Vec<_>>(),
                // Full turn data for live rendering
                "scope_depth": turn.scope_depth,
                "human": turn.human.as_ref().map(|h| serde_json::json!({
                    "content": h.content,
                    "timestamp": h.timestamp,
                })),
                "thinking": turn.thinking.as_ref().map(|t| serde_json::json!({
                    "summary": t.summary,
                })),
                "eval": turn.eval.as_ref().map(|e| serde_json::json!({
                    "content": e.content,
                    "timestamp": e.timestamp,
                    "decision": e.decision,
                    "stop_reason": e.stop_reason,
                })),
                "applies": turn.applies.iter().map(|a| serde_json::json!({
                    "tool_name": a.tool_name,
                    "input_summary": a.input_summary,
                    "output_summary": a.output_summary,
                    "is_error": a.is_error,
                    "is_agent": a.is_agent,
                    "tool_outcome": a.tool_outcome,
                })).collect::<Vec<_>>(),
                "env_size": turn.env_size,
                "env_delta": turn.env_delta,
                "stop_reason": turn.stop_reason,
                "is_terminal": turn.is_terminal,
                "duration_ms": turn.duration_ms,
            }),
        }]
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        vec![]
    }

    fn name(&self) -> &str {
        "sentence"
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tests — ported from prototype sentence-test.ts (20 tests)
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_apply::{EvalOutput, HumanInput};

    fn make_turn(overrides: impl FnOnce(&mut StructuralTurn)) -> StructuralTurn {
        let mut turn = StructuralTurn {
            session_id: "test-session".to_string(),
            turn_number: 1,
            scope_depth: 0,
            human: None,
            thinking: None,
            eval: None,
            applies: vec![],
            env_size: 5,
            env_delta: 3,
            stop_reason: "end_turn".to_string(),
            is_terminal: true,
            timestamp: "2026-04-03T01:00:00Z".to_string(),
            duration_ms: None,
            event_ids: vec![],
        };
        overrides(&mut turn);
        turn
    }

    fn apply(name: &str, input: &str) -> ApplyRecord {
        ApplyRecord {
            tool_name: name.to_string(),
            input_summary: input.to_string(),
            output_summary: String::new(),
            is_error: false,
            is_agent: name == "Agent",
            tool_outcome: None,
        }
    }

    // ── Tool classification (12 tests) ──

    #[test]
    fn read_is_preparatory() {
        assert_eq!(classify_tool("Read", "/src/main.rs"), ToolRole::Preparatory);
    }

    #[test]
    fn grep_is_preparatory() {
        assert_eq!(classify_tool("Grep", "pattern: TODO"), ToolRole::Preparatory);
    }

    #[test]
    fn write_is_creative() {
        assert_eq!(classify_tool("Write", "/scheme/01-types.scm"), ToolRole::Creative);
    }

    #[test]
    fn edit_is_creative() {
        assert_eq!(classify_tool("Edit", "/README.md"), ToolRole::Creative);
    }

    #[test]
    fn bash_with_test_is_verificatory() {
        assert_eq!(classify_tool("Bash", "npx tsx test.ts"), ToolRole::Verificatory);
    }

    #[test]
    fn bash_with_cargo_test_is_verificatory() {
        assert_eq!(classify_tool("Bash", "cd rs && cargo test"), ToolRole::Verificatory);
    }

    #[test]
    fn bash_with_install_is_preparatory() {
        assert_eq!(classify_tool("Bash", "brew install chibi-scheme"), ToolRole::Preparatory);
    }

    #[test]
    fn bash_with_git_commit_is_creative() {
        assert_eq!(classify_tool("Bash", "git commit -m 'fix'"), ToolRole::Creative);
    }

    #[test]
    fn bash_with_git_push_is_creative() {
        assert_eq!(classify_tool("Bash", "git push fork main"), ToolRole::Creative);
    }

    #[test]
    fn agent_is_delegatory() {
        assert_eq!(classify_tool("Agent", "Explore the codebase"), ToolRole::Delegatory);
    }

    #[test]
    fn bash_generic_is_verificatory() {
        assert_eq!(classify_tool("Bash", "ls -la"), ToolRole::Verificatory);
    }

    #[test]
    fn web_search_is_preparatory() {
        assert_eq!(classify_tool("WebSearch", "MIT lambda papers"), ToolRole::Preparatory);
    }

    // ── Sentence building (8 tests) ──

    #[test]
    fn text_only_turn_produces_explanatory_verb() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "What is a coalgebra?".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "A coalgebra is the dual of an algebra.".to_string(),
                timestamp: String::new(),
                stop_reason: Some("end_turn".to_string()),
                decision: "text_only".to_string(),
            });
        });
        let s = build_sentence(&turn);
        assert!(
            s.verb == "explained" || s.verb == "answered" || s.verb == "responded",
            "verb should be explanatory, got: {}",
            s.verb
        );
        assert!(s.adverbial.is_some(), "should have adverbial");
        assert!(
            s.adverbial.as_ref().unwrap().contains("coalgebra"),
            "adverbial should reference the question"
        );
        assert!(!s.one_liner.is_empty(), "should produce a one-liner");
    }

    #[test]
    fn single_read_turn() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "Show me the types".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Here are the types...".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.applies = vec![apply("Read", "/src/core/lib.rs")];
        });
        let s = build_sentence(&turn);
        assert!(s.verb == "read" || s.verb == "examined", "verb: {}", s.verb);
        assert!(
            s.object.contains("lib.rs") || s.object.contains("file"),
            "object: {}",
            s.object
        );
    }

    #[test]
    fn write_heavy_turn() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "write it in Scheme".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Let me write the code".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.is_terminal = false;
            t.applies = vec![
                apply("Read", "/src/query/lib.rs"),
                apply("Read", "/src/api/lib.rs"),
                apply("Write", "/scheme/01-types.scm"),
                apply("Write", "/scheme/02-stream.scm"),
                apply("Write", "/scheme/03-tools.scm"),
                apply("Bash", "chibi-scheme test.scm"),
                apply("Bash", "chibi-scheme test.scm"),
            ];
        });
        let s = build_sentence(&turn);
        assert_eq!(s.verb, "wrote", "verb should be 'wrote'");
        assert!(
            s.object.contains("3") || s.object.contains("Scheme"),
            "object should mention files: {}",
            s.object
        );
        assert!(!s.subordinates.is_empty(), "should have subordinate clauses");
        let roles: Vec<ToolRole> = s.subordinates.iter().map(|c| c.role).collect();
        assert!(roles.contains(&ToolRole::Preparatory), "should have preparatory clause");
        assert!(roles.contains(&ToolRole::Verificatory), "should have verificatory clause");
    }

    #[test]
    fn agent_delegation_turn() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "tell me about it".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Let me explore".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.applies = vec![apply("Agent", "Explore claurst project")];
        });
        let s = build_sentence(&turn);
        assert!(
            s.verb == "delegated" || s.verb == "explored",
            "verb: {}",
            s.verb
        );
    }

    #[test]
    fn mixed_turn_has_ordered_subordinates() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "fix the bug".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Fixed".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.applies = vec![
                apply("Read", "main.rs"),
                apply("Read", "lib.rs"),
                apply("Edit", "main.rs"),
                apply("Bash", "cargo test"),
            ];
        });
        let s = build_sentence(&turn);
        let roles: Vec<ToolRole> = s.subordinates.iter().map(|c| c.role).collect();
        if roles.contains(&ToolRole::Preparatory) && roles.contains(&ToolRole::Verificatory) {
            let prep_idx = roles.iter().position(|r| *r == ToolRole::Preparatory).unwrap();
            let verif_idx = roles.iter().position(|r| *r == ToolRole::Verificatory).unwrap();
            assert!(prep_idx < verif_idx, "preparatory before verificatory");
        }
    }

    #[test]
    fn one_liner_includes_subject() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "What files are here?".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Here are the files.".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.applies = vec![apply("Bash", "ls")];
        });
        let s = build_sentence(&turn);
        assert!(s.one_liner.contains("Claude"), "one-liner should have subject");
        assert!(s.one_liner.len() > 10, "one-liner should be substantial");
    }

    #[test]
    fn no_human_message_still_works() {
        let turn = make_turn(|t| {
            t.human = None;
            t.eval = Some(EvalOutput {
                content: "Continuing...".to_string(),
                timestamp: String::new(),
                stop_reason: None,
                decision: "tool_use".to_string(),
            });
            t.applies = vec![apply("Bash", "git status")];
        });
        let s = build_sentence(&turn);
        assert!(s.adverbial.is_none(), "no human message → no adverbial");
        assert!(!s.one_liner.is_empty(), "one-liner still produced");
    }

    #[test]
    fn no_tools_is_pure_response() {
        let turn = make_turn(|t| {
            t.human = Some(HumanInput {
                content: "that is cool".to_string(),
                timestamp: String::new(),
            });
            t.eval = Some(EvalOutput {
                content: "Yeah, it is the dual of an algebra.".to_string(),
                timestamp: String::new(),
                stop_reason: Some("end_turn".to_string()),
                decision: "text_only".to_string(),
            });
            t.applies = vec![];
        });
        let s = build_sentence(&turn);
        assert!(
            !s.verb.contains("wrote") && !s.verb.contains("read"),
            "should not be action verb: {}",
            s.verb
        );
        assert!(s.subordinates.is_empty(), "no subordinates for pure text");
    }
}
