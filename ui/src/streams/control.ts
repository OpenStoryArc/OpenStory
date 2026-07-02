/** control$ — interpreted agent view-intents as a stream. The component-local
 *  half of the agent-in-UI write seam: views (Canvas, Heatmap, Story) subscribe
 *  and apply the toggle actions that target them, so state that isn't in the URL
 *  can still be driven — while the UI stays a pure sink (it reacts to the
 *  stream, it never drives itself). Navigate/present are handled centrally in
 *  App; this stream is what views listen to for their own controls. */

import { filter, map, type Observable } from "rxjs";
import { wsMessages$ } from "@/streams/connection";
import { interpretControl, type UIControlAction } from "@/lib/ui-control";
import type { ControlMessage } from "@/types/websocket";

export function controlActions$(): Observable<UIControlAction> {
  return wsMessages$().pipe(
    filter((m): m is ControlMessage => m.kind === "control"),
    map((m) => interpretControl(m.action, m.params)),
    filter((a): a is UIControlAction => a != null),
  );
}
