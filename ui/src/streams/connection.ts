import {
  Observable,
  Subject,
  defer,
  timer,
  BehaviorSubject,
} from "rxjs";
import { webSocket } from "rxjs/webSocket";
import { retry, repeat, tap, switchMap } from "rxjs/operators";
import type { WsMessage } from "@/types/websocket";

export type ConnectionStatus = "connecting" | "connected" | "disconnected";

const status$ = new BehaviorSubject<ConnectionStatus>("disconnected");
const messages$ = new Subject<WsMessage>();

let active = false;

/** Observable of connection status changes */
export function connectionStatus$(): Observable<ConnectionStatus> {
  return status$.asObservable();
}

/** Observable of all incoming WebSocket messages */
export function wsMessages$(): Observable<WsMessage> {
  return messages$.asObservable();
}

/** The reconnect policy, as a testable pipeline: resubscribe to the source
 *  after `delayMs` whether it ERRORS (network drop) or COMPLETES cleanly
 *  (server restart closes the socket). retry handles errors, repeat handles
 *  completes — both are needed; either alone leaves a dead dashboard. */
export function resilient<T>(makeSource: () => Observable<T>, delayMs: number): Observable<T> {
  return defer(makeSource).pipe(
    retry({ delay: () => timer(delayMs) }),
    repeat({ delay: () => timer(delayMs) }),
  );
}

/** Start WebSocket connection with auto-reconnect */
export function connect(url?: string): () => void {
  if (active) return () => {};
  active = true;

  const wsUrl =
    url ??
    `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.host}/ws`;

  const log = (msg: string, ...args: unknown[]) =>
    console.debug(`%c[ws]%c ${msg}`, "color:#2ac3de;font-weight:bold", "color:inherit", ...args);

  const sub = resilient(
    () =>
      timer(0).pipe(
        tap(() => {
          log("connecting to %s", wsUrl);
          status$.next("connecting");
        }),
        switchMap(() => {
          const ws$ = webSocket<WsMessage>({
            url: wsUrl,
            openObserver: { next: () => { log("connected"); status$.next("connected"); } },
            closeObserver: { next: () => { log("disconnected"); status$.next("disconnected"); } },
          });
          return ws$;
        }),
      ),
    2000,
  )
    .pipe(
      tap((msg) => {
        if (msg.kind === "session_list") {
          log("received session_list (%d sessions)", (msg as any).sessions?.length ?? 0);
        } else if (msg.kind === "view_records") {
          log("view_records %s (%d records)", (msg as any).session_id?.slice(0, 8), (msg as any).view_records?.length ?? 0);
        } else {
          log("message kind=%s", msg.kind);
        }
        messages$.next(msg);
      }),
    )
    .subscribe();

  return () => {
    sub.unsubscribe();
    active = false;
    status$.next("disconnected");
  };
}
