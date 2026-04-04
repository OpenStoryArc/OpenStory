# Implementation: Eval-Apply Detector

*Concrete implementation plan for the eval-apply structural view.*

This is a step-by-step guide. Each step produces something testable. TDD throughout — same method we used for the Scheme code.

## Step 0: Understand What Exists

The pattern detection system is already built. Five detectors run independently on ViewRecord streams:

```rust
pub trait Detector: Send + Sync {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent>;
    fn flush(&mut self) -> Vec<PatternEvent>;
    fn name(&self) -> &str;
}
```

`FeedContext` gives each detector:
- `record: &ViewRecord` — the typed event
- `depth: u16` — tree depth (0 = root, >0 = sub-agent)
- `parent_uuid: Option<&str>` — parent event

`PatternEvent` is the output — persisted to SQLite, broadcast via WebSocket, cached in memory. No new infrastructure needed.

Detectors are registered in `PatternPipeline::new()` (`rs/server/src/ingest.rs`, line ~100). Adding ours means one line: `detectors.push(Box::new(EvalApplyDetector::new()))`.

## Step 1: The Detector (Rust)

**File:** `rs/patterns/src/eval_apply.rs`

### Types

```rust
use crate::{Detector, FeedContext, PatternEvent};
use open_story_views::RecordBody;

/// Phase of the eval-apply cycle
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Eval,       // model produced a response
    Apply,      // tool dispatched
    TurnEnd,    // coalgebra step complete
    ScopeOpen,  // Agent tool → nested loop
    ScopeClose, // nested loop returned
    Compact,    // GC fired
}

/// Internal state
#[derive(Debug, Default)]
struct TurnState {
    turn_number: u32,
    scope_depth: u32,
    env_size: u32,
    pending_tool_calls: Vec<String>,  // tool names awaiting results
    current_turn_ids: Vec<String>,    // event IDs in this turn
    current_turn_start: Option<String>, // timestamp
}

pub struct EvalApplyDetector {
    state: TurnState,
    /// Stack of scope depths for nested Agent calls.
    /// When an Agent tool_call appears, we push the current depth.
    /// When the depth decreases, we pop and emit ScopeClose.
    scope_stack: Vec<u32>,
}
```

### Core Logic

```rust
impl Detector for EvalApplyDetector {
    fn name(&self) -> &str { "eval_apply" }

    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        let id = record.id.clone();
        let ts = record.timestamp.clone();
        let mut events = Vec::new();

        // Track scope changes via depth
        let depth = ctx.depth as u32;
        if depth > self.state.scope_depth {
            self.state.scope_depth = depth;
            self.scope_stack.push(depth);
            events.push(self.emit(Phase::ScopeOpen, &ts, &[&id]));
        } else if depth < self.state.scope_depth && !self.scope_stack.is_empty() {
            self.scope_stack.pop();
            events.push(self.emit(Phase::ScopeClose, &ts, &[&id]));
            self.state.scope_depth = depth;
        }

        self.state.current_turn_ids.push(id.clone());
        if self.state.current_turn_start.is_none() {
            self.state.current_turn_start = Some(ts.clone());
        }

        match &record.body {
            RecordBody::AssistantMessage { .. } => {
                self.state.turn_number += 1;
                self.state.env_size += 1;
                events.push(self.emit(Phase::Eval, &ts, &[&id]));
            }

            RecordBody::ToolCall { name, .. } => {
                self.state.pending_tool_calls.push(name.clone());
                events.push(self.emit(Phase::Apply, &ts, &[&id]));
            }

            RecordBody::ToolResult { .. } => {
                self.state.pending_tool_calls.pop();
                self.state.env_size += 1;
                // Don't emit separately — Apply covers the round trip
            }

            RecordBody::UserMessage { .. } => {
                self.state.env_size += 1;
            }

            RecordBody::TurnEnd { .. } => {
                let turn_ids: Vec<String> =
                    std::mem::take(&mut self.state.current_turn_ids);
                let start = self.state.current_turn_start
                    .take()
                    .unwrap_or_else(|| ts.clone());
                events.push(self.emit_turn_end(&start, &ts, &turn_ids));
            }

            RecordBody::SystemEvent { ref subtype, .. }
                if subtype.as_deref() == Some("system.compact") =>
            {
                events.push(self.emit(Phase::Compact, &ts, &[&id]));
            }

            _ => {}
        }

        events
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        // Emit any in-progress turn
        if !self.state.current_turn_ids.is_empty() {
            let ids = std::mem::take(&mut self.state.current_turn_ids);
            let start = self.state.current_turn_start
                .take()
                .unwrap_or_default();
            vec![self.emit_turn_end(&start, &start, &ids)]
        } else {
            vec![]
        }
    }
}
```

### Emission Helpers

```rust
impl EvalApplyDetector {
    pub fn new() -> Self {
        Self {
            state: TurnState::default(),
            scope_stack: Vec::new(),
        }
    }

    fn emit(&self, phase: Phase, ts: &str, ids: &[&str]) -> PatternEvent {
        PatternEvent {
            pattern_type: format!("eval_apply.{}", phase_str(&phase)),
            session_id: String::new(), // filled by pipeline
            event_ids: ids.iter().map(|s| s.to_string()).collect(),
            started_at: ts.to_string(),
            ended_at: ts.to_string(),
            summary: self.phase_summary(&phase),
            metadata: serde_json::json!({
                "phase": phase_str(&phase),
                "turn": self.state.turn_number,
                "scope_depth": self.state.scope_depth,
                "env_size": self.state.env_size,
            }),
        }
    }

    fn emit_turn_end(
        &self, start: &str, end: &str, ids: &[String]
    ) -> PatternEvent {
        let has_tools = self.state.turn_number > 0; // rough proxy
        let stop = if has_tools { "tool_use → CONTINUE" }
                   else { "end_turn → TERMINATE" };
        PatternEvent {
            pattern_type: "eval_apply.turn_end".to_string(),
            session_id: String::new(),
            event_ids: ids.to_vec(),
            started_at: start.to_string(),
            ended_at: end.to_string(),
            summary: format!(
                "Turn {} (depth {}): {} | env: {} messages",
                self.state.turn_number,
                self.state.scope_depth,
                stop,
                self.state.env_size,
            ),
            metadata: serde_json::json!({
                "phase": "turn_end",
                "turn": self.state.turn_number,
                "scope_depth": self.state.scope_depth,
                "env_size": self.state.env_size,
                "stop_reason": stop,
            }),
        }
    }

    fn phase_summary(&self, phase: &Phase) -> String {
        match phase {
            Phase::Eval => format!(
                "Turn {}: eval (model examines environment, produces expression)",
                self.state.turn_number
            ),
            Phase::Apply => format!(
                "Turn {}: apply ({})",
                self.state.turn_number,
                self.state.pending_tool_calls.last()
                    .map(|s| s.as_str()).unwrap_or("?")
            ),
            Phase::ScopeOpen => format!(
                "Compound procedure: nested eval-apply at depth {}",
                self.state.scope_depth
            ),
            Phase::ScopeClose => format!(
                "Scope closed, returning to depth {}",
                self.state.scope_depth
            ),
            Phase::Compact => format!(
                "GC: context compaction (env was {} messages)",
                self.state.env_size
            ),
            Phase::TurnEnd => format!(
                "Coalgebra step {} complete",
                self.state.turn_number
            ),
        }
    }
}

fn phase_str(phase: &Phase) -> &'static str {
    match phase {
        Phase::Eval => "eval",
        Phase::Apply => "apply",
        Phase::TurnEnd => "turn_end",
        Phase::ScopeOpen => "scope_open",
        Phase::ScopeClose => "scope_close",
        Phase::Compact => "compact",
    }
}
```

### Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // Use the same test helpers as other detectors

    #[test]
    fn simple_text_response_is_one_eval_one_turn() {
        let mut det = EvalApplyDetector::new();
        let events = feed_sequence(&mut det, &[
            record(RecordBody::UserMessage { content: "hello".into() }),
            record(RecordBody::AssistantMessage { content: "hi".into() }),
            record(RecordBody::TurnEnd {}),
        ]);
        assert!(events.iter().any(|e| e.pattern_type == "eval_apply.eval"));
        assert!(events.iter().any(|e| e.pattern_type == "eval_apply.turn_end"));
        assert_eq!(
            events.iter().filter(|e| e.pattern_type == "eval_apply.apply").count(),
            0
        );
    }

    #[test]
    fn tool_use_is_eval_then_apply() {
        let mut det = EvalApplyDetector::new();
        let events = feed_sequence(&mut det, &[
            record(RecordBody::UserMessage { content: "list files".into() }),
            record(RecordBody::AssistantMessage { content: "checking...".into() }),
            record(RecordBody::ToolCall { name: "Bash".into(), .. }),
            record(RecordBody::ToolResult { content: "file1.txt".into(), .. }),
            record(RecordBody::TurnEnd {}),
        ]);
        let types: Vec<&str> = events.iter()
            .map(|e| e.pattern_type.as_str()).collect();
        assert!(types.contains(&"eval_apply.eval"));
        assert!(types.contains(&"eval_apply.apply"));
        assert!(types.contains(&"eval_apply.turn_end"));
    }

    #[test]
    fn agent_tool_opens_scope() {
        let mut det = EvalApplyDetector::new();
        // Simulate depth increasing when agent sub-events arrive
        let events = feed_sequence_with_depth(&mut det, &[
            (record(RecordBody::ToolCall { name: "Agent".into(), .. }), 0),
            (record(RecordBody::UserMessage { .. }), 1),  // sub-agent
            (record(RecordBody::AssistantMessage { .. }), 1),
            (record(RecordBody::TurnEnd {}), 1),
            (record(RecordBody::ToolResult { .. }), 0),   // back to root
        ]);
        assert!(events.iter().any(|e| e.pattern_type == "eval_apply.scope_open"));
        assert!(events.iter().any(|e| e.pattern_type == "eval_apply.scope_close"));
    }

    #[test]
    fn env_size_tracks_messages() {
        let mut det = EvalApplyDetector::new();
        feed_sequence(&mut det, &[
            record(RecordBody::UserMessage { .. }),
            record(RecordBody::AssistantMessage { .. }),
            record(RecordBody::ToolCall { .. }),
            record(RecordBody::ToolResult { .. }),
            record(RecordBody::TurnEnd {}),
        ]);
        // user + assistant + tool_result = 3 messages in env
        let turn_end = events.iter()
            .find(|e| e.pattern_type == "eval_apply.turn_end")
            .unwrap();
        assert_eq!(turn_end.metadata["env_size"], 3);
    }
}
```

## Step 2: Register the Detector

**File:** `rs/server/src/ingest.rs`, in `PatternPipeline::new()`:

```rust
// Existing detectors:
detectors.push(Box::new(TestCycleDetector::new()));
detectors.push(Box::new(GitFlowDetector::new()));
detectors.push(Box::new(ErrorRecoveryDetector::new()));
detectors.push(Box::new(AgentDelegationDetector::new()));
detectors.push(Box::new(TurnPhaseDetector::new()));
// Add:
detectors.push(Box::new(EvalApplyDetector::new()));
```

One line. Everything else — persistence, broadcast, caching — is handled by the existing pipeline.

## Step 3: Verify with Real Data

Before building UI, verify the detector produces sensible output on real sessions:

```bash
# Run tests
cd rs && cargo test -p open-story-patterns

# Start the server, let it process existing sessions
just serve

# Query patterns for our session
curl -s localhost:3002/api/sessions/ca2bc88e.../records | \
  python3 -c "
import json, sys
for r in json.load(sys.stdin):
    if r.get('record_type') == 'pattern':
        p = r['payload']
        if p.get('pattern_type', '').startswith('eval_apply'):
            print(f\"{p['pattern_type']:30s} {p['summary']}\")
"
```

Expected output for our session:
```
eval_apply.eval                Turn 1: eval (model examines environment...)
eval_apply.apply               Turn 1: apply (Agent)
eval_apply.scope_open          Compound procedure: nested eval-apply at depth 1
eval_apply.scope_close         Scope closed, returning to depth 0
eval_apply.turn_end            Turn 1 (depth 0): tool_use → CONTINUE | env: 3 messages
eval_apply.eval                Turn 2: eval (model examines environment...)
...
```

If this looks right, move to UI. If not, iterate on the detector logic. Don't build UI on broken foundations.

## Step 4: The View Model (TypeScript)

**File:** `ui/src/lib/structural-view.ts`

Pure function. No side effects. Takes raw data, returns a tree.

```typescript
import type { ViewRecord, PatternEvent } from './types';

export interface StructuralTurn {
  turnNumber: number;
  scopeDepth: number;
  eval: {
    messageId: string;
    content: string;
    stopReason: string;
  } | null;
  applies: Array<{
    toolName: string;
    inputSummary: string;
    outputSummary: string;
    isAgent: boolean;
    nestedTurns: StructuralTurn[];  // populated if isAgent
  }>;
  envSize: number;
  isTerminal: boolean;
  timestamp: string;
}

export interface StructuralSession {
  turns: StructuralTurn[];
  totalEvals: number;
  totalApplies: number;
  maxScopeDepth: number;
  compactionCount: number;
}

/**
 * Build the structural view from raw records + eval-apply patterns.
 *
 * Pure function: same input → same output.
 */
export function buildStructuralSession(
  records: ViewRecord[],
  patterns: PatternEvent[],
): StructuralSession {
  const evalApplyPatterns = patterns
    .filter(p => p.pattern_type.startsWith('eval_apply.'));

  // Group patterns by turn number
  const turnGroups = groupByTurn(evalApplyPatterns);

  // Build the turn tree
  const turns = buildTurnTree(turnGroups, records);

  return {
    turns,
    totalEvals: evalApplyPatterns
      .filter(p => p.pattern_type === 'eval_apply.eval').length,
    totalApplies: evalApplyPatterns
      .filter(p => p.pattern_type === 'eval_apply.apply').length,
    maxScopeDepth: Math.max(
      0, ...evalApplyPatterns.map(p => p.metadata?.scope_depth ?? 0)
    ),
    compactionCount: evalApplyPatterns
      .filter(p => p.pattern_type === 'eval_apply.compact').length,
  };
}
```

### Tests (BDD style, matching OpenStory conventions)

```typescript
import { describe, it, expect } from 'vitest';
import { buildStructuralSession } from './structural-view';

describe('buildStructuralSession', () => {
  it('should produce one turn for a simple text response', () => {
    const session = buildStructuralSession(
      [userRecord('hello'), assistantRecord('hi'), turnEndRecord()],
      [evalPattern(1), turnEndPattern(1, 'end_turn')],
    );
    expect(session.turns).toHaveLength(1);
    expect(session.turns[0].isTerminal).toBe(true);
    expect(session.turns[0].applies).toHaveLength(0);
  });

  it('should show eval then apply for tool use', () => {
    const session = buildStructuralSession(
      [userRecord('list files'), assistantRecord('checking...'),
       toolCallRecord('Bash'), toolResultRecord('files'), turnEndRecord()],
      [evalPattern(1), applyPattern(1, 'Bash'), turnEndPattern(1, 'tool_use')],
    );
    expect(session.turns[0].eval).not.toBeNull();
    expect(session.turns[0].applies).toHaveLength(1);
    expect(session.turns[0].applies[0].toolName).toBe('Bash');
    expect(session.turns[0].isTerminal).toBe(false);
  });

  it('should nest Agent tool calls as compound procedures', () => {
    const session = buildStructuralSession(
      records, // ... records with depth > 0 for sub-agent
      [...patterns, scopeOpenPattern(), scopeClosePattern()],
    );
    const agentApply = session.turns[0].applies
      .find(a => a.isAgent);
    expect(agentApply).toBeDefined();
    expect(agentApply!.nestedTurns.length).toBeGreaterThan(0);
  });
});
```

## Step 5: The React Component

**File:** `ui/src/components/StructuralView.tsx`

```tsx
import { StructuralSession, StructuralTurn } from '../lib/structural-view';

interface Props {
  session: StructuralSession;
  showAnnotations: boolean;
}

export function StructuralView({ session, showAnnotations }: Props) {
  return (
    <div className="space-y-4">
      <SessionHeader session={session} />
      {session.turns.map(turn => (
        <TurnCard
          key={turn.turnNumber}
          turn={turn}
          showAnnotations={showAnnotations}
        />
      ))}
    </div>
  );
}

function TurnCard({ turn, showAnnotations }: {
  turn: StructuralTurn;
  showAnnotations: boolean;
}) {
  return (
    <div
      className="border border-[#3b4261] rounded-lg p-4"
      style={{ marginLeft: `${turn.scopeDepth * 24}px` }}
    >
      {/* Turn header */}
      <div className="flex items-center justify-between mb-2">
        <span className="text-[#7aa2f7] font-mono text-sm">
          Turn {turn.turnNumber}
          {turn.scopeDepth > 0 && ` (depth ${turn.scopeDepth})`}
        </span>
        <span className="text-xs">
          {turn.isTerminal
            ? <Badge color="green">TERMINATE</Badge>
            : <Badge color="blue">CONTINUE</Badge>
          }
        </span>
      </div>

      {/* Eval phase */}
      {turn.eval && (
        <PhaseBlock
          label="EVAL"
          color="#9ece6a"
          annotation={showAnnotations
            ? "The model examines the conversation and produces content blocks. This is eval in the metacircular evaluator."
            : undefined
          }
        >
          <p className="text-sm truncate">{turn.eval.content}</p>
        </PhaseBlock>
      )}

      {/* Apply phases */}
      {turn.applies.map((apply, i) => (
        <PhaseBlock
          key={i}
          label={`APPLY: ${apply.toolName}`}
          color="#e0af68"
          annotation={showAnnotations
            ? "Tool dispatch. The tool name is the operator, the input is the operand. This is apply."
            : undefined
          }
        >
          {apply.isAgent && apply.nestedTurns.length > 0 && (
            <div className="mt-2 border-l-2 border-[#565f89] pl-3">
              <p className="text-xs text-[#565f89] mb-2">
                Nested scope (compound procedure)
              </p>
              {apply.nestedTurns.map(nested => (
                <TurnCard
                  key={nested.turnNumber}
                  turn={nested}
                  showAnnotations={showAnnotations}
                />
              ))}
            </div>
          )}
        </PhaseBlock>
      ))}

      {/* Environment counter */}
      <div className="text-xs text-[#565f89] mt-2">
        Environment: {turn.envSize} messages
      </div>
    </div>
  );
}

function PhaseBlock({ label, color, annotation, children }: {
  label: string;
  color: string;
  annotation?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-2 pl-3" style={{ borderLeft: `3px solid ${color}` }}>
      <span className="text-xs font-mono" style={{ color }}>
        {label}
      </span>
      {annotation && (
        <p className="text-xs text-[#565f89] italic mt-1">{annotation}</p>
      )}
      <div className="mt-1">{children}</div>
    </div>
  );
}
```

## Step 6: Wire It Up

**File:** `ui/src/components/SessionView.tsx` (or wherever the session detail view lives)

Add a toggle:

```tsx
const [showStructure, setShowStructure] = useState(false);
const [showAnnotations, setShowAnnotations] = useState(true);

// In the header:
<Toggle
  label="Show structure"
  checked={showStructure}
  onChange={setShowStructure}
/>

// In the body:
{showStructure ? (
  <StructuralView
    session={buildStructuralSession(records, patterns)}
    showAnnotations={showAnnotations}
  />
) : (
  <TimelineView records={records} /> // existing view
)}
```

## Implementation Order

1. **Detector + tests** — Rust, `rs/patterns/src/eval_apply.rs`. ~150 lines. TDD.
2. **Register** — one line in `ingest.rs`.
3. **Verify on real data** — curl + script, no UI yet.
4. **View model + tests** — TypeScript, `ui/src/lib/structural-view.ts`. Pure function. BDD.
5. **Component** — React, `ui/src/components/StructuralView.tsx`. Render the tree.
6. **Toggle** — wire into session view.
7. **Annotations** — educational tooltips. Link to Scheme code and SICP.

Steps 1-3 are backend only. Steps 4-7 are frontend only. They can be done by different agents in parallel once step 3 confirms the detector works.

## What You Get

A student clicks "Show structure" on any agent session in OpenStory and watches the metacircular evaluator run. Each turn is a coalgebra step. Each tool call is apply. Each model response is eval. Nested agents are nested scopes. Compaction is visible GC. The annotations link to SICP.

The same structure they can run in 600 lines of Scheme. The same structure in a forty-year-old textbook. Now alive, in real time, in their browser.
