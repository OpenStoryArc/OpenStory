/**
 * Click-parity pathfinder — pure functional navigation algebra.
 *
 *   graph  = ENTITY_EDGES (via ≠ null)
 *   path   = shortestEntityPath(A, B)
 *   steps  = path ▹ map edgeToSteps ▹ flatten
 *   land   = landMatches(hash, step)
 *
 * No I/O. No mutation of inputs. Same graph + refs + context → same steps.
 * Interpretation (intent, meaning) is not this module's job — only structure.
 */

import {
  ENTITY_EDGES,
  type DataEdge,
  type EntityKind,
} from "@/lib/action-graph";
import { CANVAS_MODES } from "@/lib/canvas-modes";

// ═══════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════

export interface EntityRef {
  readonly kind: EntityKind;
  readonly id: string;
}

export interface NavContext {
  readonly sessionId?: string;
  readonly eventId?: string;
  readonly user?: string;
  readonly project?: string;
  readonly filePath?: string;
  readonly parentSessionId?: string;
  readonly preferView?: "story" | "explore";
  /** Heatmap day cell → explore filter (YYYY-MM-DD). */
  readonly day?: string;
  /** Tool-flow agent filter. */
  readonly agent?: string;
}

export interface ControlStep {
  readonly action: string;
  readonly params: Readonly<Record<string, unknown>>;
  readonly edge?: string;
  readonly landPattern?: RegExp;
}

export interface NavigateToParams {
  readonly kind: EntityKind | "canvas" | "day" | "reel";
  readonly id: string;
  readonly sessionId?: string;
  readonly eventId?: string;
  readonly user?: string;
  readonly project?: string;
  readonly filePath?: string;
  readonly parentSessionId?: string;
  readonly view?: "story" | "explore";
  readonly details?: boolean;
  /** Expand eval-apply under the turn. */
  readonly evalOpen?: boolean;
  /** Expand event-id list under the turn. */
  readonly eventsOpen?: boolean;
  /** Shorthand: details + eval + events. */
  readonly expandAll?: boolean;
  readonly canvasMode?: string;
  readonly spotlight?: boolean;
  readonly day?: string;
  readonly agent?: string;
  /** Canvas hierarchy group-by (user|day|agent|…). */
  readonly groupBy?: string;
  /** Canvas size metric for sunburst/treemap. */
  readonly metric?: "events" | "tokens";
  /** Reel: start playback immediately on landing. */
  readonly autoplay?: boolean;
}

/** FTS hit shape used only for pure context resolution (no network here). */
export interface SearchHit {
  readonly session_id?: string;
  readonly event_id?: string;
  readonly id?: string;
}

// ═══════════════════════════════════════════════════════════════════
// Tiny pure helpers
// ═══════════════════════════════════════════════════════════════════

const trim = (s: string | undefined | null): string => (s ?? "").trim();

const basename = (path: string): string => {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
};

const escapeRe = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const step = (
  action: string,
  params: Record<string, unknown>,
  edge: string,
  landPattern?: RegExp,
): ControlStep => ({ action, params, edge, landPattern });

// ═══════════════════════════════════════════════════════════════════
// Graph algebra
// ═══════════════════════════════════════════════════════════════════

const walkable = (edges: readonly DataEdge[]): readonly DataEdge[] =>
  edges.filter((e) => e.via !== null);

/** Adjacency list: kind → outgoing walkable edges. */
const adjacency = (edges: readonly DataEdge[]): ReadonlyMap<EntityKind, readonly DataEdge[]> => {
  const m = new Map<EntityKind, DataEdge[]>();
  for (const e of walkable(edges)) {
    const list = m.get(e.from) ?? [];
    list.push(e);
    m.set(e.from, list);
  }
  return m;
};

/**
 * BFS shortest path on entity kinds.
 * from === to → []; unreachable → null.
 */
export function shortestEntityPath(
  from: EntityKind,
  to: EntityKind,
  edges: readonly DataEdge[] = ENTITY_EDGES,
): DataEdge[] | null {
  if (from === to) return [];

  const adj = adjacency(edges);
  type Frame = { readonly kind: EntityKind; readonly path: readonly DataEdge[] };
  const queue: Frame[] = [{ kind: from, path: [] }];
  const seen = new Set<EntityKind>([from]);

  while (queue.length > 0) {
    const { kind, path } = queue.shift()!;
    for (const edge of adj.get(kind) ?? []) {
      if (seen.has(edge.to)) continue;
      const next = [...path, edge];
      if (edge.to === to) return next;
      seen.add(edge.to);
      queue.push({ kind: edge.to, path: next });
    }
  }
  return null;
}

/** Every ordered pair of kinds that has a walkable path (for surveys). */
export function allReachablePairs(
  edges: readonly DataEdge[] = ENTITY_EDGES,
): readonly { from: EntityKind; to: EntityKind; hops: number }[] {
  const kinds = [
    ...new Set(edges.flatMap((e) => [e.from, e.to])),
  ] as EntityKind[];
  return kinds.flatMap((from) =>
    kinds
      .map((to) => {
        const path = shortestEntityPath(from, to, edges);
        return path === null ? null : { from, to, hops: path.length };
      })
      .filter((x): x is { from: EntityKind; to: EntityKind; hops: number } => x !== null),
  );
}

// ═══════════════════════════════════════════════════════════════════
// Context enrichment (pure)
// ═══════════════════════════════════════════════════════════════════

export const enrichContext = (
  ctx: NavContext,
  from?: EntityRef,
  to?: EntityRef,
): NavContext => ({
  ...ctx,
  sessionId:
    trim(ctx.sessionId) ||
    (from?.kind === "session" ? from.id : "") ||
    (to?.kind === "session" ? to.id : "") ||
    trim(ctx.parentSessionId) ||
    undefined,
  eventId:
    trim(ctx.eventId) ||
    (from?.kind === "event" ? from.id : "") ||
    (to?.kind === "event" ? to.id : "") ||
    undefined,
  user: trim(ctx.user) || (from?.kind === "person" ? from.id : undefined),
  project: trim(ctx.project) || (from?.kind === "project" ? from.id : undefined),
  filePath:
    trim(ctx.filePath) ||
    (from?.kind === "file" ? from.id : "") ||
    (to?.kind === "file" ? to.id : "") ||
    undefined,
});

/**
 * Resolve sessionId for an event from FTS hits (pure).
 * Prefer exact event_id match; else first hit with a session.
 */
export function resolveSessionFromHits(
  eventId: string,
  hits: readonly SearchHit[],
): string | null {
  const eid = trim(eventId);
  if (!eid || hits.length === 0) return null;
  const exact = hits.find(
    (h) => trim(h.event_id) === eid || trim(h.id) === eid,
  );
  const hit = exact ?? hits[0];
  const sid = trim(hit?.session_id);
  return sid || null;
}

// ═══════════════════════════════════════════════════════════════════
// Edge → steps (table-driven)
// ═══════════════════════════════════════════════════════════════════

type Emitter = (ctx: NavContext, from?: EntityRef, to?: EntityRef) => ControlStep[] | null;

const need = (v: string | undefined, _label: string): string | null => {
  const t = trim(v);
  return t || null;
};

const focusEvent = (
  sid: string,
  eid: string,
  view: "story" | "explore",
  edge: string,
): ControlStep =>
  step(
    "focus_event",
    { sessionId: sid, eventId: eid, view },
    edge,
    view === "story"
      ? new RegExp(`story/${escapeRe(sid)}/event/${escapeRe(eid)}`)
      : new RegExp(`event/${escapeRe(eid)}`),
  );

const searchFile = (path: string, edge: string): ControlStep => {
  const q = basename(path);
  return step(
    "open_view",
    { view: "explore", detailView: "search", searchQuery: q },
    edge,
    /search\?q=/,
  );
};

const expandSentence: Emitter = (ctx) => {
  const sid = need(ctx.sessionId, "sessionId");
  const eid = need(ctx.eventId, "eventId");
  if (!sid || !eid) return null;
  return [
    focusEvent(sid, eid, "story", "turn→sentence"),
    step(
      "set",
      { target: "story.details", open: true, eventId: eid, sessionId: sid },
      "turn→sentence (details)",
      /details=1/,
    ),
  ];
};

/** Keyed by `from>to`. Each emitter is pure. */
const EMITTERS: Readonly<Record<string, Emitter>> = {
  "person>session": (ctx, from) => {
    const user = need(ctx.user, "user") || (from?.kind === "person" ? from.id : "");
    if (!user) return null;
    return [step("query", { user }, "person→session", new RegExp(`[?&]user=${escapeRe(user)}`))];
  },
  "project>session": (ctx, from) => {
    const project =
      need(ctx.project, "project") || (from?.kind === "project" ? from.id : "");
    if (!project) return null;
    return [
      step("query", { project }, "project→session", new RegExp(`[?&]project=${escapeRe(project)}`)),
    ];
  },
  "session>subagent": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    if (!sid) return null;
    return [
      step(
        "open_view",
        { view: "explore", sessionId: sid, detailView: "graph" },
        "session→subagent",
        new RegExp(`explore/${escapeRe(sid)}`),
      ),
    ];
  },
  "session>turn": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    if (!sid) return null;
    return [
      step("open_view", { view: "story", sessionId: sid }, "session→turn", new RegExp(`story/${escapeRe(sid)}`)),
    ];
  },
  "session>plan": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    if (!sid) return null;
    return [
      step(
        "open_view",
        { view: "explore", sessionId: sid, detailView: "plans" },
        "session→plan",
        new RegExp(`explore/${escapeRe(sid)}`),
      ),
    ];
  },
  "session>event": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    if (!sid) return null;
    const eid = need(ctx.eventId, "eventId");
    if (eid) return [focusEvent(sid, eid, "explore", "session→event")];
    return [
      step("open_view", { view: "explore", sessionId: sid }, "session→event", new RegExp(`explore/${escapeRe(sid)}`)),
    ];
  },
  "event>turn": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    const eid = need(ctx.eventId, "eventId");
    if (!sid || !eid) return null;
    return [focusEvent(sid, eid, "story", "event→turn")];
  },
  "turn>event": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    const eid = need(ctx.eventId, "eventId");
    if (!sid || !eid) return null;
    return [focusEvent(sid, eid, ctx.preferView === "story" ? "story" : "explore", "turn→event")];
  },
  "turn>sentence": expandSentence,
  "toolcall>result": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    const eid = need(ctx.eventId, "eventId");
    if (!sid || !eid) return null;
    return [focusEvent(sid, eid, "explore", "toolcall→result")];
  },
  "error>event": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    const eid = need(ctx.eventId, "eventId");
    if (!sid || !eid) return null;
    return [focusEvent(sid, eid, "explore", "error→event")];
  },
  "plan>turn": (ctx) => {
    const sid = need(ctx.sessionId, "sessionId");
    const eid = need(ctx.eventId, "eventId");
    if (!sid || !eid) return null;
    return [focusEvent(sid, eid, "story", "plan→turn")];
  },
  "subagent>session": (ctx) => {
    const parent = need(ctx.parentSessionId, "parent") || need(ctx.sessionId, "sessionId");
    if (!parent) return null;
    return [
      step(
        "open_view",
        { view: "explore", sessionId: parent },
        "subagent→session",
        new RegExp(`explore/${escapeRe(parent)}`),
      ),
    ];
  },
  "toolcall>file": (ctx, _f, to) => {
    const path = need(ctx.filePath, "file") || (to?.kind === "file" ? to.id : "");
    if (!path) return null;
    return [searchFile(path, "toolcall→file")];
  },
  "file>session": (ctx, from, to) => {
    const path =
      need(ctx.filePath, "file") ||
      (from?.kind === "file" ? from.id : "") ||
      (to?.kind === "file" ? to.id : "");
    if (!path) return null;
    return [searchFile(path, "file→session")];
  },
};

export function edgeToSteps(
  edge: DataEdge,
  ctx: NavContext,
  fromRef?: EntityRef,
  toRef?: EntityRef,
): ControlStep[] | null {
  if (edge.via === null) return null;
  const key = `${edge.from}>${edge.to}`;
  const emit = EMITTERS[key];
  if (emit) return emit(ctx, fromRef, toRef);
  // Generic fallback by verb
  const sid = need(ctx.sessionId, "sessionId");
  const eid = need(ctx.eventId, "eventId");
  if (edge.via === "focus_event" && sid && eid) {
    return [focusEvent(sid, eid, ctx.preferView ?? "explore", key)];
  }
  if (edge.via === "open_view" && sid) {
    return [
      step("open_view", { view: "explore", sessionId: sid }, key, new RegExp(escapeRe(sid))),
    ];
  }
  if (edge.via === "inherent") return [];
  return null;
}

// ═══════════════════════════════════════════════════════════════════
// planNav / planNavigateTo
// ═══════════════════════════════════════════════════════════════════

export function planNav(
  from: EntityRef,
  to: EntityRef,
  ctx: NavContext,
  edges: readonly DataEdge[] = ENTITY_EDGES,
): ControlStep[] | null {
  const enriched = enrichContext(ctx, from, to);
  const path = shortestEntityPath(from.kind, to.kind, edges);
  if (path === null) return null;
  if (path.length === 0) return [];

  const chunks = path.map((edge) => edgeToSteps(edge, enriched, from, to));
  if (chunks.some((c) => c === null)) return null;
  return chunks.flatMap((c) => c!);
}

const isCanvasMode = (m: string | undefined): m is string =>
  !!m && (CANVAS_MODES as readonly string[]).includes(m);

/** Canvas: open → mode → select session (human bar/dot click). */
const planCanvas = (target: NavigateToParams): ControlStep[] => {
  const steps: ControlStep[] = [
    step("open_view", { view: "canvas" }, "→canvas", /#\/canvas/),
  ];
  if (isCanvasMode(target.canvasMode)) {
    steps.push(
      step("toggle", { target: "canvas.mode", value: target.canvasMode }, "canvas.mode", /#\/canvas/),
    );
  }
  if (isCanvasMode(target.canvasMode) && target.agent) {
    steps.push(
      step(
        "set",
        { target: "canvas.flow.agent", agent: target.agent },
        "canvas.flow.agent",
        /#\/canvas/,
      ),
    );
  }
  if (target.groupBy) {
    steps.push(
      step("toggle", { target: "canvas.groupBy", value: target.groupBy }, "canvas.groupBy", /#\/canvas/),
    );
  }
  if (target.metric === "events" || target.metric === "tokens") {
    steps.push(
      step("toggle", { target: "canvas.metric", value: target.metric }, "canvas.metric", /#\/canvas/),
    );
  }
  const sessionId = trim(target.sessionId) || (target.kind === "session" ? trim(target.id) : "");
  if (sessionId) {
    steps.push(
      step(
        "set",
        { target: "canvas.select_session", sessionId },
        "canvas.select_session",
        /#\/canvas/,
      ),
    );
  }
  return steps;
};

/** Heatmap day cell → explore filtered by day (human click parity). */
const planDay = (day: string): ControlStep[] => [
  step("query", { day }, "heatmap→day", new RegExp(`[?&]day=${escapeRe(day)}`)),
];

/**
 * High-level hand: put attention on this entity.
 * Prefer over assembling open_view / focus_event by hand.
 */
export function planNavigateTo(target: NavigateToParams): ControlStep[] | null {
  const id = trim(target.id);
  if (!id && target.kind !== "canvas") return null;

  // Day locus (heatmap cell)
  if (target.kind === "day" || target.day) {
    const day = trim(target.day) || id;
    if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return null;
    return planDay(day);
  }

  // Canvas graphs
  if (target.kind === "canvas" || target.canvasMode) {
    return planCanvas(target);
  }

  // Reel — bookmarkable spine (#/reels/REEL_ID[?autoplay=1]), one open_view step.
  if (target.kind === "reel") {
    const params: Record<string, unknown> = { view: "reels", reelId: id };
    if (target.autoplay) params.autoplay = true;
    return [{ action: "open_view", params }];
  }

  // Event
  if (target.kind === "event") {
    const sessionId = trim(target.sessionId);
    const eventId = id;
    if (!sessionId) return null; // caller should resolveSessionFromHits first
    if (target.spotlight) {
      return [
        step("focus_event", { sessionId, eventId, spotlight: true }, "event spotlight"),
      ];
    }
    const view = target.view === "explore" ? "explore" : "story";
    const expandAll = target.expandAll === true;
    const details = expandAll || target.details === true;
    const evalOpen = expandAll || target.evalOpen === true;
    const eventsOpen = expandAll || target.eventsOpen === true;
    const steps: ControlStep[] = [focusEvent(sessionId, eventId, view, "→event")];
    if (view === "story" && (details || evalOpen || eventsOpen)) {
      steps.push(
        step(
          "set",
          {
            target: "story.details",
            open: true,
            sessionId,
            eventId,
            evalOpen,
            eventsOpen,
          },
          "story.expand",
          evalOpen && eventsOpen
            ? /details=1.*eval=1.*events=1|details=1/
            : /details=1/,
        ),
      );
    }
    return steps;
  }

  // Turn / sentence → story + details
  if (target.kind === "sentence" || target.kind === "turn") {
    const sessionId = trim(target.sessionId);
    const eventId = trim(target.eventId) || id;
    if (!sessionId || !eventId) return null;
    return planNavigateTo({
      kind: "event",
      id: eventId,
      sessionId,
      view: "story",
      details: target.details !== false,
      evalOpen: target.evalOpen,
      eventsOpen: target.eventsOpen,
      expandAll: target.expandAll,
    });
  }

  // Session
  if (target.kind === "session") {
    if (target.canvasMode) {
      return planCanvas({ ...target, kind: "canvas", sessionId: id, id });
    }
    const view = target.view === "story" ? "story" : "explore";
    return [
      step("open_view", { view, sessionId: id }, "→session", new RegExp(`${view}/${escapeRe(id)}`)),
    ];
  }

  if (target.kind === "file") {
    return [searchFile(id, "→file")];
  }
  if (target.kind === "person") {
    return [step("query", { user: id }, "→person", new RegExp(`[?&]user=${escapeRe(id)}`))];
  }
  if (target.kind === "project") {
    return [step("query", { project: id }, "→project", new RegExp(`[?&]project=${escapeRe(id)}`))];
  }
  if (target.kind === "subagent") {
    const parent = trim(target.parentSessionId) || trim(target.sessionId);
    if (!parent) {
      return [
        step("open_view", { view: "explore", sessionId: id }, "→subagent", new RegExp(`explore/${escapeRe(id)}`)),
      ];
    }
    return [
      step(
        "open_view",
        { view: "explore", sessionId: parent, detailView: "graph" },
        "→subagent graph",
        new RegExp(`explore/${escapeRe(parent)}`),
      ),
    ];
  }

  // Fallback: graph walk session → kind
  if (trim(target.sessionId) && target.kind !== "canvas" && target.kind !== "day") {
    return planNav(
      { kind: "session", id: target.sessionId! },
      { kind: target.kind as EntityKind, id },
      {
        sessionId: target.sessionId,
        eventId: target.eventId || (target.kind === "event" ? id : undefined),
        filePath: target.filePath || (target.kind === "file" ? id : undefined),
        user: target.user,
        project: target.project,
        parentSessionId: target.parentSessionId,
        preferView: target.view,
        day: target.day,
        agent: target.agent,
      },
    );
  }

  return null;
}

// ═══════════════════════════════════════════════════════════════════
// Land
// ═══════════════════════════════════════════════════════════════════

export function landMatches(hash: string, s: ControlStep): boolean {
  if (!s.landPattern) return true;
  const h = hash.startsWith("#") ? hash : `#${hash}`;
  return s.landPattern.test(h);
}

export function assertLandSequence(
  hashes: readonly string[],
  steps: readonly ControlStep[],
): { ok: boolean; failedAt?: number; hash?: string; edge?: string } {
  if (hashes.length !== steps.length) return { ok: false, failedAt: -1 };
  for (let i = 0; i < steps.length; i++) {
    if (!landMatches(hashes[i]!, steps[i]!)) {
      return { ok: false, failedAt: i, hash: hashes[i], edge: steps[i]!.edge };
    }
  }
  return { ok: true };
}

/** Fold steps into a single plan summary (for agents / logs). */
export const describePlan = (steps: readonly ControlStep[]): string =>
  steps.map((s, i) => `${i + 1}. ${s.action} ${s.edge ?? ""}`).join(" → ");
