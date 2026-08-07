/** control$ — interpreted agent view-intents as a stream. The component-local
 *  half of the agent-in-UI write seam: views (Canvas, Heatmap, Story) subscribe
 *  and apply the toggle actions that target them, so state that isn't in the URL
 *  can still be driven — while the UI stays a pure sink (it reacts to the
 *  stream, it never drives itself). Navigate/present are handled centrally in
 *  App; this stream is what views listen to for their own controls.
 *
 *  Local inject: navigate_to sequences apply steps without a second WS round
 *  trip so canvas.mode / canvas.select_session sinks still fire. */

import { filter, map, merge, type Observable, Subject } from "rxjs";
import { wsMessages$ } from "@/streams/connection";
import { interpretControl, type UIControlAction } from "@/lib/ui-control";
import type { ControlMessage } from "@/types/websocket";

const localControl$ = new Subject<ControlMessage>();

/** Inject a control intent for local sequence execution (navigate_to hops). */
export function injectControl(
  action: string,
  params: Record<string, unknown>,
  issuer = "agent:sequence",
): void {
  localControl$.next({
    kind: "control",
    action,
    params,
    issuer,
  } as ControlMessage);
}

export function controlActions$(): Observable<UIControlAction> {
  const fromMsg = (m: ControlMessage) => interpretControl(m.action, m.params ?? {});
  return merge(
    wsMessages$().pipe(filter((m): m is ControlMessage => m.kind === "control")),
    localControl$,
  ).pipe(
    map(fromMsg),
    filter((a): a is UIControlAction => a != null),
  );
}
