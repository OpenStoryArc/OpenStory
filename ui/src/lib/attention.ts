/**
 * Attention — the pure tree of what the mirror shows.
 *
 *   data  ──►  Attention  ──►  pixels
 *                 ▲
 *                 └── Intent (navigate_to / control)  [fold]
 *
 * HashRoute is the bookmarkable spine. Ephemeral presentation (spotlight,
 * title, canvas selection) rides alongside. Agents drive Attention; they
 * never drive the DOM.
 *
 * Pure: no React, no I/O. fold is total where Intent is well-formed.
 */

import type { HashRoute } from "@/lib/hash-route";
import { CANVAS_MODES, type CanvasMode } from "@/lib/canvas-modes";
import {
  planNavigateTo,
  normalizeApplyOpen,
  normalizeAgentOpen,
  parseSidebarFacet,
  type ControlStep,
  type NavigateToParams,
} from "@/lib/nav-path";
import type { BrushExtent } from "@/lib/sessions-scatter";
import type { UIControlAction } from "@/lib/ui-control";

/** Does Attention's apply-open satisfy the intent's applyOpen request? */
function applyOpenSatisfied(
  have: HashRoute["storyApplyOpen"],
  want: NavigateToParams["applyOpen"],
): boolean {
  if (want === undefined) return true;
  const needed = normalizeApplyOpen(want);
  if (needed === undefined) return true;
  if (needed === "all") return have === "all";
  if (have === "all") return true;
  if (!have) return false;
  return needed.every((i) => have.includes(i));
}

/** Does Attention's agent-open cover every requested agent session id? */
function agentOpenSatisfied(
  have: HashRoute["storyAgentOpen"],
  want: NavigateToParams["agentOpen"],
): boolean {
  if (!want || want.length === 0) return true;
  const needed = normalizeAgentOpen(want);
  if (!needed || needed.length === 0) return true;
  const set = new Set(have ?? []);
  return needed.every((k) => set.has(k));
}

/** Does canvas expandedKeys cover every requested expand key? */
function expandKeysSatisfied(
  have: readonly string[] | undefined,
  want: readonly string[] | undefined,
): boolean {
  if (!want || want.length === 0) return true;
  const set = new Set(have ?? []);
  return want.every((k) => set.has(k.trim()) || set.has(k));
}

/**
 * Normalize a scatter brush box from intent / set params into data-space BrushExtent.
 * Returns null when required numeric fields are missing.
 */
export function normalizeScatterBrush(
  raw: NavigateToParams["scatterBrush"] | Record<string, unknown> | null | undefined,
): BrushExtent | null {
  if (!raw || typeof raw !== "object") return null;
  const num = (v: unknown): number | null => {
    const n = typeof v === "number" ? v : typeof v === "string" ? Number(v) : NaN;
    return Number.isFinite(n) ? n : null;
  };
  const ev0 = num((raw as { ev0?: unknown }).ev0);
  const ev1 = num((raw as { ev1?: unknown }).ev1);
  const tok0 = num((raw as { tok0?: unknown }).tok0);
  const tok1 = num((raw as { tok1?: unknown }).tok1);
  if (ev0 === null || ev1 === null || tok0 === null || tok1 === null) return null;
  const iz = (raw as { includeZero?: unknown }).includeZero;
  return {
    ev0,
    ev1,
    tok0,
    tok1,
    includeZero: iz === true || iz === "true",
  };
}

/** Does Attention's scatter brush cover the requested brush (exact data box)? */
function scatterBrushSatisfied(
  have: BrushExtent | undefined,
  want: NavigateToParams["scatterBrush"] | undefined,
): boolean {
  if (!want) return true;
  const needed = normalizeScatterBrush(want);
  if (!needed) return true;
  if (!have) return false;
  return (
    have.ev0 === needed.ev0 &&
    have.ev1 === needed.ev1 &&
    have.tok0 === needed.tok0 &&
    have.tok1 === needed.tok1 &&
    have.includeZero === needed.includeZero
  );
}

// ═══════════════════════════════════════════════════════════════════
// Attention value
// ═══════════════════════════════════════════════════════════════════

/** Ephemeral full-screen presentation (not in the hash). */
export type Spotlight =
  | { readonly kind: "event"; readonly sessionId: string; readonly eventId: string; readonly clipAt?: string }
  | { readonly kind: "title"; readonly message: string };

/** Group-by dimensions the canvas hierarchy understands. */
export type CanvasGroupBy = "day" | "user" | "agent" | "status" | "host" | "project";

/** Canvas subtree of attention (mode + selection + chart knobs). */
export interface CanvasAttention {
  readonly mode?: CanvasMode;
  readonly selectedSessionId?: string;
  readonly flowAgent?: string;
  readonly groupBy?: CanvasGroupBy;
  readonly metric?: "events" | "tokens";
  /**
   * Board hierarchy expand path — group/project keys from sessions-canvas
   * (`g:user`, `p:user:project`). Agent-driveable drill; human clicks merge in.
   */
  readonly expandedKeys?: readonly string[];
  /**
   * Scatter efficiency brush — data-space box (events × tokens).
   * Same shape as `set scatter.brush` / pointsInBrush.
   */
  readonly scatterBrush?: BrushExtent;
}

/** Normalize expand key list (trim, drop empty, unique, stable order). */
export function normalizeExpandKeys(
  keys: readonly string[] | undefined | null,
): readonly string[] | undefined {
  if (!keys) return undefined;
  const out = [
    ...new Set(keys.map((k) => k.trim()).filter((k) => k.length > 0)),
  ].sort();
  return out.length > 0 ? out : [];
}

/** Union of expand key sets (stable sorted). Empty array is a valid "collapse all". */
export function mergeExpandKeys(
  base: readonly string[] | undefined,
  add: readonly string[] | undefined,
): readonly string[] {
  return normalizeExpandKeys([...(base ?? []), ...(add ?? [])]) ?? [];
}

/** Toggle one key in/out of the expand set. */
export function toggleExpandKey(
  base: readonly string[] | undefined,
  key: string,
): readonly string[] {
  const k = key.trim();
  if (!k) return base ?? [];
  const set = new Set(base ?? []);
  if (set.has(k)) set.delete(k);
  else set.add(k);
  return [...set].sort();
}

const GROUP_BYS = new Set<string>(["day", "user", "agent", "status", "host", "project"]);
const isGroupBy = (g: string | undefined): g is CanvasGroupBy => !!g && GROUP_BYS.has(g);
const isMetric = (m: string | undefined): m is "events" | "tokens" =>
  m === "events" || m === "tokens";

/**
 * The whole tree of attention — one immutable value.
 * `route` is always present (bookmarkable spine).
 */
export interface Attention {
  readonly route: HashRoute;
  readonly spotlight: Spotlight | null;
  readonly presentMessage: string | null;
  readonly canvas: CanvasAttention;
}

export const emptyAttention = (route: HashRoute = { view: "live" }): Attention => ({
  route,
  spotlight: null,
  presentMessage: null,
  canvas: {},
});

export const attentionFromRoute = (route: HashRoute): Attention =>
  emptyAttention(route);

// ═══════════════════════════════════════════════════════════════════
// Pure projections
// ═══════════════════════════════════════════════════════════════════

export const attentionSummary = (a: Attention): string => {
  const { route, spotlight, canvas } = a;
  const bits: string[] = [route.view];
  if (route.sessionId) bits.push(`session:${route.sessionId.slice(0, 8)}`);
  if (route.eventId) bits.push(`event:${route.eventId.slice(0, 8)}`);
  if (route.storyDetails) bits.push("details");
  if (route.storyEvalOpen) bits.push("eval");
  if (route.storyApplyOpen === "all") bits.push("apply:all");
  else if (route.storyApplyOpen?.length) bits.push(`apply:${route.storyApplyOpen.join(",")}`);
  if (route.storyAgentOpen?.length) bits.push(`agents:${route.storyAgentOpen.length}`);
  if (route.detailView) bits.push(route.detailView);
  if (canvas.mode) bits.push(`canvas:${canvas.mode}`);
  if (canvas.groupBy) bits.push(`by:${canvas.groupBy}`);
  if (canvas.metric) bits.push(`metric:${canvas.metric}`);
  if (canvas.expandedKeys?.length) bits.push(`exp:${canvas.expandedKeys.length}`);
  if (canvas.scatterBrush) bits.push("brush");
  if (canvas.selectedSessionId) bits.push(`sel:${canvas.selectedSessionId.slice(0, 8)}`);
  if (spotlight?.kind === "event") bits.push("spotlight:event");
  if (spotlight?.kind === "title") bits.push("spotlight:title");
  return bits.join(" · ");
};

/** Does this Attention satisfy a navigate_to intent? (land predicate) */
export function attentionSatisfies(
  a: Attention,
  intent: NavigateToParams,
): boolean {
  const id = (intent.id ?? "").trim();
  switch (intent.kind) {
    case "event": {
      if (a.spotlight?.kind === "event" && intent.spotlight) {
        return a.spotlight.eventId === id;
      }
      if (a.route.eventId !== id) return false;
      if (intent.sessionId && a.route.sessionId !== intent.sessionId) return false;
      if (intent.details && !a.route.storyDetails) return false;
      if (intent.evalOpen && !a.route.storyEvalOpen) return false;
      if (!applyOpenSatisfied(a.route.storyApplyOpen, intent.applyOpen)) return false;
      if (!agentOpenSatisfied(a.route.storyAgentOpen, intent.agentOpen)) return false;
      if (intent.expandAll) {
        if (!a.route.storyDetails || !a.route.storyEvalOpen || !a.route.storyEventsOpen) {
          return false;
        }
        if (a.route.storyApplyOpen !== "all") return false;
      }
      if (intent.view === "explore") return a.route.view === "explore";
      return a.route.view === "story" || a.route.view === "explore";
    }
    case "session": {
      if (intent.canvasMode) {
        if (
          a.route.view !== "canvas" ||
          a.canvas.mode !== intent.canvasMode ||
          a.canvas.selectedSessionId !== id
        ) {
          return false;
        }
        if (!expandKeysSatisfied(a.canvas.expandedKeys, intent.expandKeys)) return false;
        return scatterBrushSatisfied(a.canvas.scatterBrush, intent.scatterBrush);
      }
      return a.route.sessionId === id && (a.route.view === "explore" || a.route.view === "story");
    }
    case "canvas": {
      if (a.route.view !== "canvas") return false;
      if (intent.canvasMode && a.canvas.mode !== intent.canvasMode) return false;
      if (intent.groupBy && a.canvas.groupBy !== intent.groupBy) return false;
      if (!expandKeysSatisfied(a.canvas.expandedKeys, intent.expandKeys)) return false;
      return scatterBrushSatisfied(a.canvas.scatterBrush, intent.scatterBrush);
    }
    case "file":
      return (
        a.route.view === "explore" &&
        a.route.detailView === "search" &&
        !!a.route.searchQuery
      );
    case "person":
      return a.route.view === "explore" && a.route.explore?.filters?.user === id;
    case "project":
      return a.route.view === "explore" && a.route.explore?.filters?.project === id;
    case "day":
      return a.route.view === "explore" && a.route.explore?.filters?.day === id;
    case "facet": {
      const parsed = parseSidebarFacet(id, intent.facet);
      if (!parsed) return false;
      return (
        a.route.view === "explore" &&
        a.route.explore?.filters?.[parsed.key] === parsed.value
      );
    }
    case "sentence":
    case "turn":
      return (
        a.route.view === "story" &&
        !!a.route.eventId &&
        (intent.details === false || !!a.route.storyDetails)
      );
    case "reel":
      return a.route.view === "reels" && a.route.reelId === id;
    default:
      return false;
  }
}

// ═══════════════════════════════════════════════════════════════════
// Fold: Intent → Attention  (denotational semantics of navigate_to)
// ═══════════════════════════════════════════════════════════════════

const isMode = (m: string | undefined): m is CanvasMode =>
  !!m && (CANVAS_MODES as readonly string[]).includes(m);

/**
 * Direct fold of a high-level intent into Attention — the artful expression.
 * Prefer this over multi-step when sinks can read Attention (route + canvas).
 */
export function foldIntent(
  base: Attention,
  intent: NavigateToParams,
): Attention | null {
  const id = (intent.id ?? "").trim();
  if (!id && intent.kind !== "canvas") return null;

  // Canvas graph
  if (intent.kind === "canvas" || intent.canvasMode) {
    const mode = isMode(intent.canvasMode) ? intent.canvasMode : base.canvas.mode;
    const selectedSessionId =
      (intent.sessionId || (intent.kind === "session" ? id : "") || "").trim() ||
      base.canvas.selectedSessionId;
    const groupBy = isGroupBy(intent.groupBy) ? intent.groupBy : base.canvas.groupBy;
    const metric = isMetric(intent.metric) ? intent.metric : base.canvas.metric;
    const expandedKeys =
      intent.expandKeys !== undefined
        ? mergeExpandKeys(base.canvas.expandedKeys, intent.expandKeys)
        : base.canvas.expandedKeys;
    const scatterBrush =
      intent.scatterBrush !== undefined
        ? normalizeScatterBrush(intent.scatterBrush) ?? undefined
        : base.canvas.scatterBrush;
    return {
      ...base,
      route: { view: "canvas" },
      spotlight: null,
      presentMessage: null,
      canvas: {
        ...base.canvas,
        ...(mode ? { mode } : {}),
        ...(selectedSessionId ? { selectedSessionId } : {}),
        ...(intent.agent ? { flowAgent: intent.agent } : {}),
        ...(groupBy ? { groupBy } : {}),
        ...(metric ? { metric } : {}),
        ...(intent.expandKeys !== undefined ? { expandedKeys } : {}),
        ...(intent.scatterBrush !== undefined && scatterBrush
          ? { scatterBrush }
          : intent.scatterBrush !== undefined
            ? { scatterBrush: undefined }
            : {}),
      },
    };
  }

  // Day (heatmap cell)
  if (intent.kind === "day" || intent.day) {
    const day = (intent.day || id).trim();
    if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return null;
    return {
      ...base,
      route: {
        view: "explore",
        explore: { filters: { day } },
      },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Event
  if (intent.kind === "event") {
    const sessionId = (intent.sessionId ?? "").trim();
    if (!sessionId) return null;
    if (intent.spotlight) {
      return {
        ...base,
        route: base.route,
        spotlight: { kind: "event", sessionId, eventId: id },
        presentMessage: null,
        canvas: base.canvas,
      };
    }
    const view = intent.view === "explore" ? "explore" : "story";
    const expandAll = intent.expandAll === true;
    const applyOpen = normalizeApplyOpen(intent.applyOpen, expandAll);
    const agentOpen = normalizeAgentOpen(intent.agentOpen);
    const details =
      expandAll ||
      intent.details === true ||
      applyOpen !== undefined ||
      agentOpen !== undefined;
    const evalOpen =
      expandAll ||
      intent.evalOpen === true ||
      applyOpen !== undefined ||
      agentOpen !== undefined;
    const eventsOpen = expandAll || intent.eventsOpen === true;
    return {
      ...base,
      route: {
        view,
        sessionId,
        eventId: id,
        ...(view === "explore" ? { detailView: "events" as const } : {}),
        ...(view === "story" &&
        (details || evalOpen || eventsOpen || applyOpen !== undefined || agentOpen !== undefined)
          ? {
              storyDetails: true,
              ...(evalOpen ? { storyEvalOpen: true } : {}),
              ...(eventsOpen ? { storyEventsOpen: true } : {}),
              ...(applyOpen !== undefined ? { storyApplyOpen: applyOpen } : {}),
              ...(agentOpen !== undefined ? { storyAgentOpen: agentOpen } : {}),
            }
          : {}),
      },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Turn / sentence
  if (intent.kind === "sentence" || intent.kind === "turn") {
    const sessionId = (intent.sessionId ?? "").trim();
    const eventId = (intent.eventId || id).trim();
    if (!sessionId || !eventId) return null;
    return foldIntent(base, {
      kind: "event",
      id: eventId,
      sessionId,
      view: "story",
      details: intent.details !== false,
    });
  }

  // Session
  if (intent.kind === "session") {
    if (intent.canvasMode) {
      return foldIntent(base, {
        kind: "canvas",
        id,
        sessionId: id,
        canvasMode: intent.canvasMode,
        agent: intent.agent,
        groupBy: intent.groupBy,
        metric: intent.metric,
        expandKeys: intent.expandKeys,
        scatterBrush: intent.scatterBrush,
      });
    }
    const view = intent.view === "story" ? "story" : "explore";
    return {
      ...base,
      route: { view, sessionId: id },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Reel — bookmarkable spine (#/reels/REEL_ID[?autoplay=1]); mirrors the
  // Session branch above (route built directly, no canvas/spotlight state).
  if (intent.kind === "reel") {
    return {
      ...base,
      route: {
        view: "reels",
        reelId: id,
        ...(intent.autoplay ? { reelAutoplay: true } : {}),
      },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // File
  if (intent.kind === "file") {
    const q = id.includes("/") ? id.slice(id.lastIndexOf("/") + 1) : id;
    return {
      ...base,
      route: { view: "explore", detailView: "search", searchQuery: q },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Person / project
  if (intent.kind === "person") {
    return {
      ...base,
      route: { view: "explore", explore: { filters: { user: id } } },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }
  if (intent.kind === "project") {
    return {
      ...base,
      route: { view: "explore", explore: { filters: { project: id } } },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Sidebar facet chip as named entity (chip-id or structured facet+value).
  if (intent.kind === "facet") {
    const parsed = parseSidebarFacet(id, intent.facet);
    if (!parsed) return null;
    return {
      ...base,
      route: {
        view: "explore",
        explore: { filters: { [parsed.key]: parsed.value } },
      },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  // Subagent
  if (intent.kind === "subagent") {
    const parent = (intent.parentSessionId || intent.sessionId || "").trim();
    if (!parent) {
      return {
        ...base,
        route: { view: "explore", sessionId: id },
        spotlight: null,
        presentMessage: null,
        canvas: {},
      };
    }
    return {
      ...base,
      route: { view: "explore", sessionId: parent, detailView: "graph" },
      spotlight: null,
      presentMessage: null,
      canvas: {},
    };
  }

  return null;
}

// ═══════════════════════════════════════════════════════════════════
// Fold: low-level UIControlAction → Attention
// ═══════════════════════════════════════════════════════════════════

export function foldControl(base: Attention, action: UIControlAction): Attention {
  switch (action.type) {
    case "navigate":
      return {
        ...base,
        route: action.route,
        spotlight: null,
        // keep canvas only if staying on canvas
        canvas: action.route.view === "canvas" ? base.canvas : {},
      };
    case "spotlight":
      return {
        ...base,
        spotlight: {
          kind: "event",
          sessionId: action.sessionId,
          eventId: action.eventId,
          clipAt: action.clipAt,
        },
        presentMessage: null,
      };
    case "title":
      return {
        ...base,
        spotlight: { kind: "title", message: action.message },
        presentMessage: action.message,
      };
    case "present":
      return {
        ...base,
        presentMessage: action.message || base.presentMessage,
        route: action.route ?? base.route,
        spotlight: null,
      };
    case "toggle":
      if (action.target === "spotlight" && action.value === "off") {
        return { ...base, spotlight: null, presentMessage: null };
      }
      if (action.target === "canvas.mode" && isMode(action.value)) {
        return {
          ...base,
          route: { view: "canvas" },
          canvas: { ...base.canvas, mode: action.value },
          spotlight: null,
        };
      }
      if (action.target === "canvas.groupBy" && isGroupBy(action.value)) {
        return {
          ...base,
          route: { view: "canvas" },
          canvas: { ...base.canvas, groupBy: action.value },
        };
      }
      if (action.target === "canvas.metric" && isMetric(action.value)) {
        return {
          ...base,
          route: { view: "canvas" },
          canvas: { ...base.canvas, metric: action.value },
        };
      }
      // Board drill: toggle one hierarchy key open/closed.
      if (action.target === "canvas.expand" && action.value.trim()) {
        return {
          ...base,
          route: { view: "canvas" },
          canvas: {
            ...base.canvas,
            expandedKeys: toggleExpandKey(base.canvas.expandedKeys, action.value),
          },
        };
      }
      if (action.target === "story.details" && (action.value === "open" || action.value === "on")) {
        if (base.route.view === "story" && base.route.sessionId && base.route.eventId) {
          return {
            ...base,
            route: { ...base.route, storyDetails: true },
          };
        }
      }
      return base;
    case "set":
      if (action.target === "story.details") {
        const sessionId =
          typeof action.params.sessionId === "string"
            ? action.params.sessionId
            : base.route.sessionId;
        const eventId =
          typeof action.params.eventId === "string"
            ? action.params.eventId
            : base.route.eventId;
        const open =
          action.params.open === true ||
          action.params.open === "true" ||
          action.params.value === "open";
        const applyOpen = normalizeApplyOpen(
          action.params.applyOpen as NavigateToParams["applyOpen"],
        );
        const agentOpen = normalizeAgentOpen(
          Array.isArray(action.params.agentOpen)
            ? (action.params.agentOpen as string[])
            : typeof action.params.agentOpen === "string"
              ? (action.params.agentOpen as string).split(",")
              : undefined,
        );
        if (sessionId && eventId && open) {
          const evalOpen =
            action.params.evalOpen === true ||
            action.params.evalOpen === "true" ||
            applyOpen !== undefined ||
            agentOpen !== undefined;
          return {
            ...base,
            route: {
              view: "story",
              sessionId,
              eventId,
              storyDetails: true,
              storyEvalOpen: evalOpen,
              storyEventsOpen:
                action.params.eventsOpen === true || action.params.eventsOpen === "true",
              ...(applyOpen !== undefined ? { storyApplyOpen: applyOpen } : {}),
              ...(agentOpen !== undefined ? { storyAgentOpen: agentOpen } : {}),
            },
            spotlight: null,
          };
        }
      }
      if (action.target === "canvas.select_session") {
        const sessionId =
          typeof action.params.sessionId === "string"
            ? action.params.sessionId.trim()
            : typeof action.params.id === "string"
              ? action.params.id.trim()
              : "";
        if (sessionId) {
          return {
            ...base,
            route: { view: "canvas" },
            canvas: { ...base.canvas, selectedSessionId: sessionId },
          };
        }
      }
      // Board drill: set keys (replace). Empty array = collapse all.
      if (action.target === "canvas.expand") {
        const raw = action.params.keys ?? action.params.expandedKeys;
        const keys = Array.isArray(raw)
          ? raw.filter((k): k is string => typeof k === "string")
          : typeof action.params.key === "string"
            ? [action.params.key]
            : undefined;
        if (keys !== undefined) {
          return {
            ...base,
            route: { view: "canvas" },
            canvas: {
              ...base.canvas,
              expandedKeys: normalizeExpandKeys(keys) ?? [],
            },
          };
        }
      }
      // Scatter efficiency brush — data-space box on Attention.
      if (action.target === "scatter.brush") {
        const brush = normalizeScatterBrush(action.params);
        if (brush) {
          return {
            ...base,
            route: { view: "canvas" },
            canvas: {
              ...base.canvas,
              mode: base.canvas.mode ?? "scatter",
              scatterBrush: brush,
            },
          };
        }
      }
      if (action.target === "canvas.flow.agent") {
        const agent =
          typeof action.params.agent === "string"
            ? action.params.agent
            : typeof action.params.value === "string"
              ? action.params.value
              : "";
        if (agent) {
          return {
            ...base,
            canvas: { ...base.canvas, flowAgent: agent },
          };
        }
      }
      return base;
    case "navigate_sequence":
      // Sequence denotation is foldSteps(interpret) — caller supplies interpret
      // to avoid import cycles. Identity here; App uses foldSteps explicitly.
      return base;
    default:
      return base;
  }
}

/** Fold a planned step list (from planNavigateTo) into Attention. Pure. */
export function foldSteps(
  base: Attention,
  steps: readonly ControlStep[],
  interpret: (action: string, params: unknown) => UIControlAction | null,
): Attention {
  return steps.reduce<Attention>((att, s) => {
    const a = interpret(s.action, s.params);
    if (!a || a.type === "navigate_sequence") return att;
    return foldControl(att, a);
  }, base);
}

/**
 * Preferred pure path for navigate_to:
 *   intent ─foldIntent► Attention
 * Falls back to planNavigateTo ▹ foldSteps when foldIntent is null.
 */
export function realizeIntent(
  base: Attention,
  intent: NavigateToParams,
  interpret: (action: string, params: unknown) => UIControlAction | null,
): Attention | null {
  const direct = foldIntent(base, intent);
  if (direct) return direct;
  const steps = planNavigateTo(intent);
  if (!steps || steps.length === 0) return null;
  return foldSteps(base, steps, interpret);
}

// ═══════════════════════════════════════════════════════════════════
// Imperative shell ports — apply pure Attention to the world
// ═══════════════════════════════════════════════════════════════════

/** Side-effect ports for materializing Attention (functional core / imperative shell). */
export interface AttentionPorts {
  readonly navigate: (route: HashRoute) => void;
  readonly setSpotlight: (
    s: { sessionId: string; eventId: string; clipAt?: string } | null,
  ) => void;
  readonly setTitleCard: (message: string | null) => void;
  /** Optional: push canvas knobs to control$ sinks that still listen there. */
  readonly injectControl?: (
    action: string,
    params: Record<string, unknown>,
    issuer?: string,
  ) => void;
}

/**
 * Materialize Attention into the UI shell. Pure value in; effects only here.
 * Canvas mode/selection also injectControl so existing sinks stay wired.
 */
export function materializeAttention(
  att: Attention,
  ports: AttentionPorts,
  issuer = "agent",
): void {
  ports.navigate(att.route);

  if (att.spotlight?.kind === "event") {
    ports.setSpotlight({
      sessionId: att.spotlight.sessionId,
      eventId: att.spotlight.eventId,
      clipAt: att.spotlight.clipAt,
    });
    ports.setTitleCard(null);
  } else if (att.spotlight?.kind === "title") {
    ports.setTitleCard(att.spotlight.message);
    ports.setSpotlight(null);
  } else {
    ports.setSpotlight(null);
    ports.setTitleCard(null);
  }

  const inject = ports.injectControl;
  if (!inject) return;

  if (att.route.view === "canvas" && att.canvas.mode) {
    inject("toggle", { target: "canvas.mode", value: att.canvas.mode }, issuer);
  }
  if (att.canvas.groupBy) {
    inject("toggle", { target: "canvas.groupBy", value: att.canvas.groupBy }, issuer);
  }
  if (att.canvas.metric) {
    inject("toggle", { target: "canvas.metric", value: att.canvas.metric }, issuer);
  }
  if (att.canvas.selectedSessionId) {
    inject(
      "set",
      { target: "canvas.select_session", sessionId: att.canvas.selectedSessionId },
      issuer,
    );
  }
  if (att.canvas.flowAgent) {
    inject("set", { target: "canvas.flow.agent", agent: att.canvas.flowAgent }, issuer);
  }
  // canvas.expand / expandedKeys: no dual inject. Sequence path
  // (realizeIntent / foldSteps) commits Attention first; SessionsCanvas paints
  // via canvasAttention$. Direct set/toggle canvas.expand still reaches
  // control$ via WS / injectControl sequence hops.
  // scatter.brush: no dual inject (same contract; ScatterView + scatterPaintFromBrush).
}
