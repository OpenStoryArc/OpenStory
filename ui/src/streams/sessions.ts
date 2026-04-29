import { Observable, Subject, merge } from "rxjs";
import { scan, map, tap, filter, shareReplay, startWith, bufferTime } from "rxjs/operators";
import type { ViewRecord } from "@/types/view-record";
import type { WireRecord, PatternView } from "@/types/wire-record";
import type { WsMessage, ServerPatternEvent, SessionLabel } from "@/types/websocket";

// ═══════════════════════════════════════════════════════════════════
// Enriched reducer — durable/ephemeral + tree index
// ═══════════════════════════════════════════════════════════════════

/** Session state — global flat WireRecord array, lazy-populated.
 *
 *  Records are NOT seeded from `initial_state` (that handshake is
 *  sidebar-only after feat/lazy-load-initial-state). They arrive via:
 *    - live `enriched` WS messages (append directly), and
 *    - per-session REST fetch on session-open (`session_records_loaded`).
 *
 *  `loadedSessions` is a cache key set: the UI consults it before
 *  fetching, so opening the same session twice doesn't double-fetch. */
export interface EnrichedSessionState {
  readonly records: readonly WireRecord[];
  readonly currentEphemeral: WireRecord | null;
  readonly patterns: readonly PatternView[];
  readonly treeIndex: ReadonlyMap<string, { depth: number; parent_uuid: string | null }>;
  readonly sessionLabels: Readonly<Record<string, SessionLabel>>;
  readonly loadedSessions: ReadonlySet<string>;
}

export const EMPTY_ENRICHED_STATE: EnrichedSessionState = {
  records: [],
  currentEphemeral: null,
  patterns: [],
  treeIndex: new Map(),
  sessionLabels: {},
  loadedSessions: new Set(),
};

/** Action types for the enriched reducer. */
export type EnrichedAction =
  | {
      readonly kind: "initial_state";
      readonly patterns: readonly PatternView[];
      readonly sessionLabels: Readonly<Record<string, SessionLabel>>;
    }
  | {
      readonly kind: "session_records_loaded";
      readonly session_id: string;
      readonly records: readonly WireRecord[];
    }
  | {
      readonly kind: "enriched";
      readonly session_id: string;
      readonly records: readonly WireRecord[];
      readonly ephemeral: readonly (WireRecord | ViewRecord)[];
      readonly patterns?: readonly PatternView[];
      readonly session_label?: string;
      readonly session_branch?: string;
      readonly total_input_tokens?: number;
      readonly total_output_tokens?: number;
    };

/** Append `incoming` to `existing`, skipping any record whose id is
 *  already present. Returns the merged array and the set of newly added
 *  IDs (for treeIndex updates). Preserves order: existing first, then
 *  new in input order. */
function mergeUniqueById(
  existing: readonly WireRecord[],
  incoming: readonly WireRecord[],
): { merged: WireRecord[]; added: WireRecord[] } {
  if (incoming.length === 0) {
    return { merged: existing as WireRecord[], added: [] };
  }
  const seen = new Set<string>();
  for (const r of existing) seen.add(r.id);
  const added: WireRecord[] = [];
  for (const r of incoming) {
    if (seen.has(r.id)) continue;
    seen.add(r.id);
    added.push(r);
  }
  if (added.length === 0) {
    return { merged: existing as WireRecord[], added: [] };
  }
  return { merged: (existing as WireRecord[]).concat(added), added };
}

/** Pure enriched reducer — handles initial_state, session_records_loaded, and enriched messages. */
export function enrichedReducer(
  state: EnrichedSessionState,
  action: EnrichedAction,
): EnrichedSessionState {
  switch (action.kind) {
    case "initial_state": {
      // Sidebar-only handshake: seed labels and any patterns the server
      // has already detected for recent sessions. Records stay empty —
      // the UI lazy-loads per session.
      return {
        ...state,
        patterns: [...action.patterns],
        sessionLabels: { ...action.sessionLabels },
      };
    }
    case "session_records_loaded": {
      const { merged, added } = mergeUniqueById(state.records, action.records);
      let treeIndex = state.treeIndex;
      if (added.length > 0) {
        const mutable = new Map(state.treeIndex);
        for (const r of added) {
          mutable.set(r.id, { depth: r.depth, parent_uuid: r.parent_uuid });
        }
        treeIndex = mutable;
      }
      const loadedSessions = new Set(state.loadedSessions);
      loadedSessions.add(action.session_id);
      return {
        ...state,
        records: merged,
        treeIndex,
        loadedSessions,
      };
    }
    case "enriched": {
      // 1. Append durable records (dedup by id — covers the case where
      //    a live event arrives during an in-flight REST fetch and ends
      //    up duplicated when the response lands).
      const { merged: newRecords, added } = mergeUniqueById(state.records, action.records);

      // 2. Update tree index for newly-added durable records only
      let treeIndex: ReadonlyMap<string, { depth: number; parent_uuid: string | null }> = state.treeIndex;
      if (added.length > 0) {
        const mutable = new Map(state.treeIndex);
        for (const r of added) {
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

      // 5. Merge label updates (including token counts)
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

      return {
        ...state,
        records: newRecords,
        currentEphemeral,
        patterns: newPatterns,
        treeIndex,
        sessionLabels,
      };
    }
  }
}

/** Map a server PatternEvent to the client-side PatternView shape. */
export function toPatternView(pe: ServerPatternEvent): PatternView {
  return {
    type: pe.pattern_type,
    label: pe.summary,
    session_id: pe.session_id,
    events: [...pe.event_ids],
    metadata: pe.metadata,
  };
}

// ═══════════════════════════════════════════════════════════════════
// Stream builders
// ═══════════════════════════════════════════════════════════════════

/** Side-channel for actions dispatched outside the WebSocket stream
 *  (per-session REST loads). Components push to this via
 *  `dispatchSessionRecordsLoaded` and the main stream merges it in. */
const externalActions$ = new Subject<EnrichedAction>();

/** Push a `session_records_loaded` action into the global state stream.
 *  Called by `Timeline` after a successful REST fetch. */
export function dispatchSessionRecordsLoaded(
  session_id: string,
  records: readonly WireRecord[],
): void {
  externalActions$.next({ kind: "session_records_loaded", session_id, records });
}

/** Options for buildSessionState$. */
export interface BuildSessionStateOptions {
  /** Buffer window in ms. 0 = no batching (backward compat). Default: 16 (~60fps). */
  readonly batchMs?: number;
}

/** Build the main state stream from WebSocket messages plus any
 *  external actions (REST-loaded session records). Records arrive
 *  lazily — `initial_state` no longer seeds them. */
export function buildSessionState$(
  ws$: Observable<WsMessage>,
  options: BuildSessionStateOptions = {},
): Observable<EnrichedSessionState> {
  const batchMs = options.batchMs ?? 16;
  const wsActions$: Observable<EnrichedAction | null> = ws$.pipe(
    map((msg): EnrichedAction | null => {
      switch (msg.kind) {
        case "initial_state": {
          const patterns: PatternView[] = msg.patterns
            ? msg.patterns.map(toPatternView)
            : [];
          const sessionLabels = msg.session_labels ?? {};
          return {
            kind: "initial_state",
            patterns,
            sessionLabels,
          };
        }
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
            session_label: msg.session_label,
            session_branch: msg.session_branch,
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
          };
        default:
          return null;
      }
    }),
  );

  const log = (msg: string, ...args: unknown[]) =>
    console.debug(`%c[state]%c ${msg}`, "color:#bb9af7;font-weight:bold", "color:inherit", ...args);

  const actions$ = merge(wsActions$, externalActions$).pipe(
    filter((a): a is EnrichedAction => a !== null),
    tap((action) => {
      if (action.kind === "initial_state") {
        log("initial_state: %d sessions, %d patterns",
          Object.keys(action.sessionLabels).length, action.patterns.length);
      } else if (action.kind === "session_records_loaded") {
        log("session_records_loaded %s → %d records",
          action.session_id?.slice(0, 8), action.records.length);
      } else if (action.kind === "enriched") {
        log("enriched %s → %d records",
          action.session_id?.slice(0, 8), action.records?.length);
      }
    }),
  );

  if (batchMs > 0) {
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

  return actions$.pipe(
    scan(enrichedReducer, EMPTY_ENRICHED_STATE),
    tap((state) => log("state: %d records", state.records.length)),
    startWith(EMPTY_ENRICHED_STATE),
    shareReplay(1),
  );
}
