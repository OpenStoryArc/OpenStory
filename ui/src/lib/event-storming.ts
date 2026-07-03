/** OpenStory, mapped onto the Event Storming grammar — as DATA.
 *
 *  Event Storming's stickies: 🟧 domain events (what happened), 🟦 commands
 *  (intents), 🟨 aggregates (consistency roots) + actors (who), 🟪 policies
 *  ("when event → then command"), 🟩 read models (for decisions), 🟥 external
 *  systems. OpenStory was built event-first, so the mapping is close to 1:1 — and
 *  the interesting twist is TWO bounded contexts (observed `events.*`, read-only;
 *  authored `ui.*`, where commands live) with an anti-corruption layer between.
 *
 *  This is the board rendered by the Storm tab. Interactivity = graph traversal:
 *  neighborsOf() lights a sticky's causes + effects; journeyEdges() lights a
 *  user journey's path. All pure. */

export type StickyKind = "actor" | "command" | "aggregate" | "event" | "policy" | "readmodel" | "external";
export type StormContext = "observed" | "authored";

export interface Sticky {
  readonly id: string;
  readonly kind: StickyKind;
  readonly label: string;
  readonly context: StormContext;
  readonly note?: string;
}
export interface Flow {
  readonly from: string;
  readonly to: string;
}
export interface Journey {
  readonly id: string;
  readonly name: string;
  /** Ordered sticky ids — a path through the grammar = one E2E requirement. */
  readonly path: readonly string[];
  readonly note?: string;
}

// ── The board ──────────────────────────────────────────────────────
export const STICKIES: readonly Sticky[] = [
  // actors
  { id: "human", kind: "actor", label: "You", context: "authored", note: "The human navigating the mirror." },
  { id: "agent", kind: "actor", label: "Agent (MCP)", context: "authored", note: "Drives + follows the UI through the seam." },
  { id: "watcher", kind: "actor", label: "Watcher", context: "observed", note: "Reads transcript files, translates to CloudEvents. Read-only." },
  // external
  { id: "coding_agent", kind: "external", label: "Coding agent", context: "observed", note: "The observed subject — Claude Code, pi-mono. Its work is mirrored, never touched." },
  { id: "nats", kind: "external", label: "NATS JetStream", context: "observed", note: "The bus. events.* (observed) and ui.* (authored) are separate streams — the sovereignty partition." },
  // commands (authored)
  { id: "cmd_openview", kind: "command", label: "open_view", context: "authored", note: "Navigate to a view / session." },
  { id: "cmd_focus", kind: "command", label: "focus_event", context: "authored", note: "Drill to one exact event — navigateToEntity('event')." },
  { id: "cmd_toggle", kind: "command", label: "toggle", context: "authored", note: "Switch a view control (canvas lens…)." },
  { id: "cmd_query", kind: "command", label: "query", context: "authored", note: "Filter the data." },
  { id: "cmd_present", kind: "command", label: "present", context: "authored", note: "Agent shows the human something." },
  { id: "cmd_interact", kind: "command", label: "navigate / select", context: "authored", note: "The human's own moves, captured as commands." },
  // aggregates
  { id: "session", kind: "aggregate", label: "Session", context: "observed", note: "The consistency root — owns turns, events, plans. Everything is session-scoped." },
  { id: "viewing", kind: "aggregate", label: "Viewing session", context: "authored", note: "openstory-ui — the human's own navigation stream, a first-class aggregate." },
  // events (observed)
  { id: "ev_prompt", kind: "event", label: "user.prompt", context: "observed" },
  { id: "ev_tooluse", kind: "event", label: "assistant.tool_use", context: "observed" },
  { id: "ev_turn", kind: "event", label: "turn.complete", context: "observed" },
  { id: "ev_error", kind: "event", label: "system.error", context: "observed" },
  // events (authored)
  { id: "ev_interaction", kind: "event", label: "interaction.*", context: "authored", note: "Emitted on ui.* when you move." },
  { id: "ev_control", kind: "event", label: "control.*", context: "authored", note: "Emitted on ui.* when an agent drives." },
  { id: "ev_annotation", kind: "event", label: "annotation.*", context: "authored" },
  // policies
  { id: "pol_persist", kind: "policy", label: "persist", context: "observed", note: "When an event arrives → store (SQLite + JSONL), dedup." },
  { id: "pol_patterns", kind: "policy", label: "detect patterns", context: "observed", note: "When a turn completes → emit turn.sentence / eval-apply." },
  { id: "pol_project", kind: "policy", label: "project", context: "observed", note: "When an event arrives → update the read model." },
  { id: "pol_broadcast", kind: "policy", label: "broadcast", context: "authored", note: "When an event arrives → push to every dashboard (WS)." },
  { id: "pol_pacing", kind: "policy", label: "act in rests", context: "authored", note: "When the user is resting (tempo) → an agent may act." },
  // read models
  { id: "rm_projection", kind: "readmodel", label: "SessionProjection", context: "observed", note: "Tokens, status, metadata — materialized." },
  { id: "rm_uistate", kind: "readmodel", label: "ui_state + tempo", context: "authored", note: "Where you are + your rhythm." },
  { id: "rm_actiongraph", kind: "readmodel", label: "ActionGraph", context: "authored", note: "What's navigable — the command surface, testable." },
];

export const FLOWS: readonly Flow[] = [
  // observed pipeline
  { from: "coding_agent", to: "watcher" },
  { from: "watcher", to: "session" },
  { from: "session", to: "ev_prompt" },
  { from: "session", to: "ev_tooluse" },
  { from: "session", to: "ev_turn" },
  { from: "session", to: "ev_error" },
  { from: "ev_tooluse", to: "nats" },
  { from: "ev_turn", to: "nats" },
  { from: "nats", to: "pol_persist" },
  { from: "nats", to: "pol_patterns" },
  { from: "nats", to: "pol_project" },
  { from: "pol_patterns", to: "ev_turn" },
  { from: "pol_project", to: "rm_projection" },
  // authored — commands
  { from: "human", to: "cmd_interact" },
  { from: "human", to: "cmd_openview" },
  { from: "agent", to: "cmd_openview" },
  { from: "agent", to: "cmd_focus" },
  { from: "agent", to: "cmd_present" },
  { from: "agent", to: "cmd_toggle" },
  { from: "agent", to: "cmd_query" },
  { from: "cmd_interact", to: "viewing" },
  { from: "cmd_openview", to: "viewing" },
  { from: "cmd_focus", to: "viewing" },
  { from: "cmd_present", to: "viewing" },
  { from: "cmd_openview", to: "rm_actiongraph" },
  { from: "cmd_focus", to: "rm_actiongraph" },
  // authored — events → policies → read model
  { from: "viewing", to: "ev_interaction" },
  { from: "viewing", to: "ev_control" },
  { from: "viewing", to: "ev_annotation" },
  { from: "ev_interaction", to: "nats" },
  { from: "ev_control", to: "nats" },
  { from: "nats", to: "pol_broadcast" },
  { from: "pol_broadcast", to: "rm_uistate" },
  { from: "pol_project", to: "rm_uistate" },
  // the pacing loop — read model feeds a policy that triggers an agent command
  { from: "rm_uistate", to: "pol_pacing" },
  { from: "pol_pacing", to: "agent" },
];

export const JOURNEYS: readonly Journey[] = [
  { id: "j_observe", name: "Read a session's story", path: ["human", "cmd_openview", "session", "ev_turn", "cmd_focus", "viewing"],
    note: "Open the Story, see the turns, drill a turn to its source event." },
  { id: "j_ingest", name: "Ingest agent work", path: ["coding_agent", "watcher", "session", "ev_tooluse", "nats", "pol_persist", "rm_projection"],
    note: "A coding agent works; the watcher mirrors it into a session, stored + projected." },
  { id: "j_drive", name: "Agent drives the mirror", path: ["agent", "cmd_present", "viewing", "ev_control", "nats", "pol_broadcast", "rm_uistate"],
    note: "An agent shows the human something; the dashboard reacts." },
  { id: "j_follow", name: "Follow the user & act in rests", path: ["human", "cmd_interact", "viewing", "ev_interaction", "pol_project", "rm_uistate", "pol_pacing", "agent"],
    note: "The human moves; the agent reads their rhythm and acts in the gaps." },
];

// ── graph helpers (the interactivity substrate) ────────────────────
const byId = new Map(STICKIES.map((s) => [s.id, s]));
export function stickyById(id: string): Sticky | undefined {
  return byId.get(id);
}

/** A sticky's causes (upstream) and effects (downstream) — for click-highlight. */
export function neighborsOf(flows: readonly Flow[], id: string): { upstream: string[]; downstream: string[] } {
  const upstream: string[] = [];
  const downstream: string[] = [];
  for (const f of flows) {
    if (f.to === id) upstream.push(f.from);
    if (f.from === id) downstream.push(f.to);
  }
  return { upstream, downstream };
}

/** Consecutive pairs of a journey path → the flow edges to light up. */
export function journeyEdges(journey: Journey): Flow[] {
  const edges: Flow[] = [];
  for (let i = 1; i < journey.path.length; i++) edges.push({ from: journey.path[i - 1]!, to: journey.path[i]! });
  return edges;
}
