import {
  Observable,
  Subject,
  timer,
  EMPTY,
  BehaviorSubject,
} from "rxjs";
import { webSocket } from "rxjs/webSocket";
import { retry, tap, catchError, switchMap } from "rxjs/operators";
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

/** Start WebSocket connection with auto-reconnect */
export function connect(url?: string): () => void {
  if (active) return () => {};
  active = true;

  const wsUrl =
    url ??
    `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.host}/ws`;

  const log = (msg: string, ...args: unknown[]) =>
    console.debug(`%c[ws]%c ${msg}`, "color:#2ac3de;font-weight:bold", "color:inherit", ...args);

  const sub = timer(0)
    .pipe(
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
      catchError((err) => {
        log("error: %o", err);
        status$.next("disconnected");
        return EMPTY;
      }),
      retry({ delay: () => timer(2000) }),
    )
    .subscribe();

  return () => {
    sub.unsubscribe();
    active = false;
    status$.next("disconnected");
  };
}
