import { Observable } from "rxjs";
import { scan, map, tap, filter, shareReplay, startWith, bufferTime } from "rxjs/operators";
import type { ViewRecord } from "@/types/view-record";
import type { WireRecord, PatternView } from "@/types/wire-record";
import type { WsMessage, ServerPatternEvent, SessionLabel } from "@/types/websocket";

// ═══════════════════════════════════════════════════════════════════
// Enriched reducer — durable/ephemeral + filter deltas + tree index
// ═══════════════════════════════════════════════════════════════════

/** New session state shape with enriched data. */
export interface EnrichedSessionState {
  readonly records: readonly WireRecord[];
  readonly currentEphemeral: WireRecord | null;
  readonly patterns: readonly PatternView[];
  /** Per-session filter counts: session_id → { filter_name → count }. */
  readonly filterCounts: Readonly<Record<string, Readonly<Record<string, number>>>>;
  readonly treeIndex: ReadonlyMap<string, { depth: number; parent_uuid: string | null }>;
  /** Session labels: session_id → { label, branch }. */
  readonly sessionLabels: Readonly<Record<string, SessionLabel>>;
  /** Agent labels: agent_id (or delegation event_id) → description. */
  readonly agentLabels: Readonly<Record<string, string>>;
}

export const EMPTY_ENRICHED_STATE: EnrichedSessionState = {
  records: [],
  currentEphemeral: null,
  patterns: [],
  filterCounts: {},
  treeIndex: new Map(),
  sessionLabels: {},
  agentLabels: {},
};

/** Action types for the enriched reducer. */
export type EnrichedAction =
  | {
      readonly kind: "initial_state";
      readonly records: readonly WireRecord[];
      readonly patterns: readonly PatternView[];
      readonly filterCounts: Readonly<Record<string, Readonly<Record<string, number>>>>;
      readonly sessionLabels: Readonly<Record<string, SessionLabel>>;
      readonly agentLabels: Readonly<Record<string, string>>;
    }
  | {
      readonly kind: "enriched";
      readonly session_id: string;
      readonly records: readonly WireRecord[];
      readonly ephemeral: readonly (WireRecord | ViewRecord)[];
      readonly patterns?: readonly PatternView[];
      readonly filter_deltas: Readonly<Record<string, number>>;
      readonly session_label?: string;
      readonly session_branch?: string;
      readonly agent_labels?: Readonly<Record<string, string>>;
      readonly total_input_tokens?: number;
      readonly total_output_tokens?: number;
    };

/** Pure enriched reducer — handles initial_state and enriched messages. */
export function enrichedReducer(
  state: EnrichedSessionState,
  action: EnrichedAction,
): EnrichedSessionState {
  switch (action.kind) {
    case "initial_state": {
      const treeIndex = new Map<string, { depth: number; parent_uuid: string | null }>();
      for (const r of action.records) {
        treeIndex.set(r.id, { depth: r.depth, parent_uuid: r.parent_uuid });
      }
      return {
        records: [...action.records],
        currentEphemeral: null,
        patterns: [...(action.patterns ?? [])],
        filterCounts: { ...action.filterCounts },
        treeIndex,
        sessionLabels: { ...action.sessionLabels },
        agentLabels: { ...action.agentLabels },
      };
    }
    case "enriched": {
      // 1. Append durable records.
      //    concat() creates a new array — O(n) per call but correct.
      //    For 500-record cap from initial_state + streaming appends,
      //    this is acceptable. The server batches records per message
      //    so individual appends are infrequent relative to batch size.
      const newRecords: readonly WireRecord[] =
        action.records.length > 0
          ? (state.records as WireRecord[]).concat(action.records as WireRecord[])
          : state.records;

      // 2. Update tree index for durable records only
      let treeIndex: ReadonlyMap<string, { depth: number; parent_uuid: string | null }> = state.treeIndex;
      if (action.records.length > 0) {
        const mutable = new Map(state.treeIndex);
        for (const r of action.records) {
          mutable.set(r.id, { depth: r.depth, parent_uuid: r.parent_uuid });
        }
        treeIndex = mutable;
      }

      // 3. Set ephemeral (last one wins — not accumulated)
      const currentEphemeral =
        action.ephemeral.length > 0
          ? (action.ephemeral[action.ephemeral.length - 1] as WireRecord)
          : state.currentEphemeral;

      // 4. Accumulate patterns
      const newPatterns =
        action.patterns && action.patterns.length > 0
          ? [...state.patterns, ...action.patterns]
          : state.patterns;

      // 5. Apply filter deltas to the correct session
      let filterCounts = state.filterCounts;
      const deltas = action.filter_deltas;
      if (Object.keys(deltas).length > 0) {
        const sessionId = action.session_id;
        const existing = state.filterCounts[sessionId] ?? {};
        const updated: Record<string, number> = { ...existing };
        for (const [key, delta] of Object.entries(deltas)) {
          updated[key] = (existing[key] ?? 0) + delta;
        }
        filterCounts = { ...state.filterCounts, [sessionId]: updated };
      }

      // 6. Merge label updates (including token counts)
      let sessionLabels = state.sessionLabels;
      if (action.session_label || action.session_branch || action.total_input_tokens != null) {
        const existing = state.sessionLabels[action.session_id] ?? { label: null, branch: null };
        sessionLabels = {
          ...state.sessionLabels,
          [action.session_id]: {
            label: action.session_label ?? existing.label,
            branch: action.session_branch ?? existing.branch,
            total_input_tokens: action.total_input_tokens ?? existing.total_input_tokens,
            total_output_tokens: action.total_output_tokens ?? existing.total_output_tokens,
          },
        };
      }

      let agentLabels = state.agentLabels;
      if (action.agent_labels && Object.keys(action.agent_labels).length > 0) {
        agentLabels = { ...state.agentLabels, ...action.agent_labels };
      }

      return {
        records: newRecords,
        currentEphemeral,
        patterns: newPatterns,
        filterCounts,
        treeIndex,
        sessionLabels,
        agentLabels,
      };
    }
  }
}

/** Derive flat filter counts for a given view.
 *  When sessionFilter is set, returns that session's counts.
 *  When null, aggregates across all sessions. */
export function getFilterCounts(
  perSession: Readonly<Record<string, Readonly<Record<string, number>>>>,
  sessionFilter: string | null,
): Readonly<Record<string, number>> {
  if (sessionFilter) {
    return perSession[sessionFilter] ?? {};
  }
  const agg: Record<string, number> = {};
  for (const sessionCounts of Object.values(perSession)) {
    for (const [key, val] of Object.entries(sessionCounts)) {
      agg[key] = (agg[key] ?? 0) + val;
    }
  }
  return agg;
}

/** Map a server PatternEvent to the client-side PatternView shape. */
export function toPatternView(pe: ServerPatternEvent): PatternView {
  return {
    type: pe.pattern_type,
    label: pe.summary,
    events: [...pe.event_ids],
    metadata: pe.metadata,
  };
}

// ═══════════════════════════════════════════════════════════════════
// Stream builders
// ═══════════════════════════════════════════════════════════════════

/** Options for buildSessionState$. */
export interface BuildSessionStateOptions {
  /** Buffer window in ms. 0 = no batching (backward compat). Default: 16 (~60fps). */
  readonly batchMs?: number;
}

/** Build the main state stream from WebSocket messages.
 *  Uses enrichedReducer — state holds WireRecord[] with full tree metadata.
 *  When batchMs > 0, buffers actions into time windows and folds each batch,
 *  reducing React renders from 100/s to ~60/s during sustained streaming. */
export function buildSessionState$(
  ws$: Observable<WsMessage>,
  options: BuildSessionStateOptions = {},
): Observable<EnrichedSessionState> {
  const batchMs = options.batchMs ?? 16;
  const wsActions$: Observable<EnrichedAction | null> = ws$.pipe(
    map((msg): EnrichedAction | null => {
      switch (msg.kind) {
        case "initial_state":
          if ("records" in msg) {
            // Enriched initial_state: WireRecords + filter_counts
            const filterCounts: Record<string, Record<string, number>> =
              "filter_counts" in msg && msg.filter_counts
                ? { ...msg.filter_counts }
                : {};
            const patterns: PatternView[] = "patterns" in msg && msg.patterns
              ? msg.patterns.map(toPatternView)
              : [];
            const sessionLabels = "session_labels" in msg && msg.session_labels
              ? msg.session_labels
              : {};
            const agentLabels = "agent_labels" in msg && msg.agent_labels
              ? msg.agent_labels
              : {};
            return {
              kind: "initial_state",
              records: msg.records,
              patterns,
              filterCounts,
              sessionLabels,
              agentLabels,
            };
          }
          if ("view_records" in msg) {
            // Legacy initial_state: ViewRecords → wrap as WireRecords with defaults
            const records: WireRecord[] = msg.view_records.map((vr) => ({
              ...vr,
              depth: 0,
              parent_uuid: null,
              truncated: false,
              payload_bytes: 0,
            }));
            return {
              kind: "initial_state",
              records,
              patterns: [],
              filterCounts: {},
              sessionLabels: {},
              agentLabels: {},
            };
          }
          return null;
        case "enriched": {
          const patterns: PatternView[] = msg.patterns
            ? msg.patterns.map(toPatternView)
            : [];
          return {
            kind: "enriched",
            session_id: msg.session_id,
            records: [...msg.records],
            ephemeral: [...msg.ephemeral],
            patterns,
            filter_deltas: msg.filter_deltas,
            session_label: msg.session_label,
            session_branch: msg.session_branch,
            agent_labels: msg.agent_labels,
            total_input_tokens: msg.total_input_tokens,
            total_output_tokens: msg.total_output_tokens,
          };
        }
        case "view_records":
          // Legacy view_records → wrap as enriched durable
          return {
            kind: "enriched",
            session_id: msg.session_id,
            records: msg.view_records.map((vr) => ({
              ...vr,
              depth: 0,
              parent_uuid: null,
              truncated: false,
              payload_bytes: 0,
            })),
            ephemeral: [],
            filter_deltas: {},
          };
        default:
          return null;
      }
    }),
  );

  const log = (msg: string, ...args: unknown[]) =>
    console.debug(`%c[state]%c ${msg}`, "color:#bb9af7;font-weight:bold", "color:inherit", ...args);

  // Filter nulls and add logging
  const actions$ = wsActions$.pipe(
    filter((a): a is EnrichedAction => a !== null),
    tap((action) => {
      if (action.kind === "initial_state") log("initial_state: %d records", action.records.length);
      else if (action.kind === "enriched") log("enriched %s → %d records", action.session_id?.slice(0, 8), action.records?.length);
    }),
  );

  if (batchMs > 0) {
    // Batch actions into time windows, fold each batch through reducer
    return actions$.pipe(
      bufferTime(batchMs),
      filter((batch) => batch.length > 0),
      scan(
        (state, batch) => batch.reduce(enrichedReducer, state),
        EMPTY_ENRICHED_STATE,
      ),
      tap((state) => log("state: %d records (batched)", state.records.length)),
      startWith(EMPTY_ENRICHED_STATE),
      shareReplay(1),
    );
  }

  // batchMs=0: no batching, each action produces one emission (backward compat)
  return actions$.pipe(
    scan(enrichedReducer, EMPTY_ENRICHED_STATE),
    tap((state) => log("state: %d records", state.records.length)),
    startWith(EMPTY_ENRICHED_STATE),
    shareReplay(1),
  );
}
