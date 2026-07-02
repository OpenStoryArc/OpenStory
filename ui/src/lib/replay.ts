/** The REPLAY engine — the payoff of the agent-in-UI seam.
 *
 *  interaction ↔ command are INVERSES: a captured interaction ("where the human
 *  went") maps back to the control action that would drive the dashboard there.
 *  So replaying a journey is just feeding its interaction stream back through the
 *  control seam. FORWARD retraces the path; BACKWARD reverses the list to rewind.
 *
 *  Pure: interactions in, ordered control steps out. The driver (a script or an
 *  in-UI affordance) POSTs each step to /api/control on its `atMs` schedule. No
 *  I/O here, so the whole vocabulary mapping is testable in isolation. */

import type { Interaction } from "@/lib/interaction";
import type { ControlParams } from "@/lib/ui-control";

/** One replay step: fire `action`+`params` at `atMs` after replay start. The
 *  {action, params} pair is exactly what /api/control accepts, so a step drives
 *  the UI with no translation — it's the inverse of the interaction it came from. */
export interface ReplayStep {
  readonly atMs: number;
  readonly action: string;
  readonly params: ControlParams;
}

export interface ReplayOptions {
  readonly direction: "forward" | "backward";
  /** Playback speed. 1 = base cadence; 2 = twice as fast (half the gaps). */
  readonly tempo: number;
}

/** Base gap between steps at tempo 1, when interactions carry no timing of their
 *  own. A comfortable ~1s "watch it move" cadence; tempo scales it. */
const BASE_STEP_MS = 1000;

/** Map a single interaction → the inverse control {action, params}, or null if
 *  this kind doesn't drive anything (e.g. a bare "view"). This is the crux: each
 *  Interaction variant names exactly the control verb that reproduces it. */
function toControl(i: Interaction): { action: string; params: ControlParams } | null {
  switch (i.kind) {
    case "navigate": {
      const params: ControlParams = { view: i.view };
      if (i.session_id) params.sessionId = i.session_id;
      if (i.detailView) params.detailView = i.detailView;
      if (i.eventId) params.eventId = i.eventId;
      return { action: "open_view", params };
    }
    case "select": {
      // With an eventId this is the finest grain — drive straight to the event.
      // Without one, the whole session is the target: open it.
      if (i.eventId) {
        return { action: "focus_event", params: { sessionId: i.session_id, eventId: i.eventId, view: i.view } };
      }
      return { action: "open_view", params: { view: i.view, sessionId: i.session_id } };
    }
    case "zoom": {
      if (!i.mode) return null;
      return { action: "toggle", params: { target: "canvas.mode", value: i.mode } };
    }
    case "filter": {
      const filters = (i.filters ?? {}) as Record<string, unknown>;
      return { action: "query", params: { ...filters } };
    }
    default:
      return null;
  }
}

/** Turn a captured interaction stream into an ordered, scheduled list of control
 *  steps. FORWARD preserves order (retrace); BACKWARD reverses it (rewind). The
 *  first step is always at atMs 0; each subsequent gap is BASE_STEP_MS / tempo. */
export function replay(interactions: readonly Interaction[], opts: ReplayOptions): ReplayStep[] {
  const ordered = opts.direction === "backward" ? [...interactions].reverse() : interactions;
  const tempo = opts.tempo > 0 ? opts.tempo : 1;
  const gap = BASE_STEP_MS / tempo;

  const steps: ReplayStep[] = [];
  for (const i of ordered) {
    const ctrl = toControl(i);
    if (!ctrl) continue; // skip uninterpretable kinds, keep the schedule dense
    steps.push({ atMs: steps.length * gap, action: ctrl.action, params: ctrl.params });
  }
  return steps;
}
