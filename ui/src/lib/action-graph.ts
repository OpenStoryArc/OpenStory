/** The ActionGraph — every possible movement through OpenStory, as DATA.
 *
 *  This is NOT the DOM (what Playwright would crawl — incidental, markup-tied).
 *  It's the SEMANTIC action space: nodes are entity kinds, edges are the moves
 *  the data affords, each realized (or not) by a control verb. It's the same
 *  vocabulary the agent-in-UI seam already speaks — `focus_event` is just
 *  `navigateToEntity("event", id)` — so every edge here is drivable via the MCP
 *  and replayable.
 *
 *  A DEAD END is an edge the data has (`from`→`to` is a real relationship) that
 *  no control verb walks yet (`via: null`). The navigabilityReport turns "this
 *  click doesn't lead anywhere" into a number and a list — testable, and walkable
 *  live by the MCP surveyor (scripts/nav_survey.mjs). */

/** The entities we store — the nodes of the graph. */
export type EntityKind =
  | "person" | "project" | "session" | "subagent"
  | "turn" | "plan" | "sentence"
  | "event" | "toolcall" | "result" | "file" | "error";

/** How the UI walks an edge today. A real control verb is drivable via the seam;
 *  `inherent` means the destination is already on-screen (no navigation needed);
 *  `null` is a DEAD END — the data connects, the UI doesn't. */
export type NavVerb = "open_view" | "focus_event" | "toggle" | "query" | "inherent";

export interface DataEdge {
  readonly from: EntityKind;
  readonly to: EntityKind;
  readonly label: string;
  readonly via: NavVerb | null;
  /** Optional concrete hint for the live surveyor (which view / target). */
  readonly note?: string;
}

/** The data model as a walkable graph — one declaration, the whole root system.
 *  Adding a row here (with a `via`) makes the edge navigable everywhere that
 *  consumes this map; leaving `via: null` marks it as a branch to grow. */
export const ENTITY_EDGES: readonly DataEdge[] = [
  { from: "person", to: "session", label: "owns", via: "query", note: "filter by user" },
  { from: "project", to: "session", label: "contains", via: "query", note: "filter by project" },
  { from: "session", to: "subagent", label: "spawns", via: "open_view", note: "explore graph / canvas delegation" },
  { from: "session", to: "turn", label: "has", via: "open_view", note: "story" },
  { from: "session", to: "plan", label: "produces", via: "open_view", note: "explore plans" },
  { from: "session", to: "event", label: "has", via: "open_view", note: "explore / live" },
  { from: "turn", to: "sentence", label: "summarized", via: "inherent", note: "both on Story" },
  { from: "turn", to: "event", label: "source ↗", via: "focus_event", note: "just closed" },
  { from: "event", to: "turn", label: "its turn ↑", via: "focus_event", note: "#/story/SES/event/ID — card link + seam" },
  // ── the branches that stop in mid-air (data connected, UI isn't) ──
  { from: "subagent", to: "session", label: "parent ↑", via: "open_view", note: "summary strip ↑ parent (parent_session_id on /summary)" },
  { from: "toolcall", to: "result", label: "paired ⇄", via: "focus_event", note: "card ⇄ link jumps the round trip (tool-pair map)" },
  { from: "toolcall", to: "file", label: "writes", via: null },
  { from: "file", to: "session", label: "impact ↺", via: "query", note: "#/search?q=path — facet link + seam searchQuery" },
  { from: "error", to: "event", label: "locus", via: "focus_event", note: "summary \"failed →\" deep-links the first-error event everywhere" },
  { from: "plan", to: "turn", label: "authored by", via: null },
];

export interface NavReport {
  /** Every data edge considered. */
  readonly total: number;
  /** Edges a verb walks (drivable or inherent). */
  readonly realized: DataEdge[];
  /** Edges no verb walks — the dead ends. */
  readonly deadEnds: DataEdge[];
  /** Realized edges driven by a real control verb (what the MCP survey walks). */
  readonly drivable: DataEdge[];
  /** realized / total, 0..1. */
  readonly coverage: number;
}

/** Pure: score a set of data edges for navigability. */
export function navigabilityReport(edges: readonly DataEdge[]): NavReport {
  const realized = edges.filter((e) => e.via !== null);
  const deadEnds = edges.filter((e) => e.via === null);
  const drivable = edges.filter((e) => e.via !== null && e.via !== "inherent");
  return {
    total: edges.length,
    realized,
    deadEnds,
    drivable,
    coverage: edges.length === 0 ? 0 : realized.length / edges.length,
  };
}
