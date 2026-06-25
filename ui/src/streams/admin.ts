/**
 * admin$ — the topology as an RxJS stream (UI is a sink).
 *
 * Composition:
 *
 *     fetch(/api/admin/topology)       ← one-shot seed (initial paint,
 *           │                            bookmark-friendly URL)
 *           ▼
 *      startWith(seed)
 *           │
 *           │     wsMessages$()
 *           │       │
 *           │       └── filter(kind === "admin_topology_changed")
 *           │              │
 *           │              ▼
 *           │           map(m => m.topology)
 *           ▼              │
 *      ┌────merge────────┘
 *      │
 *      ▼
 *   shareReplay(1)            ← memoize latest frame for late
 *                                subscribers (mounted/unmounted
 *                                AdminView doesn't re-fetch)
 *
 * SICP stream-with-memory at the UI layer. Each subscriber sees:
 *   - first emission: the cached frame (initial REST or last WS push)
 *   - each subsequent emission: a new topology snapshot
 */

import { Observable, defer, from, merge } from "rxjs";
import { filter, map, shareReplay, startWith, switchMap } from "rxjs/operators";

import { fetchTopology, type Topology } from "@/lib/admin-api";
import { wsMessages$ } from "@/streams/connection";

/** Build the admin topology stream. Cold until first subscribe; then
 *  fires REST seed, then forwards every WS topology push. */
export function buildAdminStream$(): Observable<Topology> {
  const seed$ = defer(() => from(fetchTopology()));

  const live$ = wsMessages$().pipe(
    filter((m): m is Extract<typeof m, { kind: "admin_topology_changed" }> =>
      m.kind === "admin_topology_changed",
    ),
    map((m) => m.topology),
  );

  return seed$.pipe(
    switchMap((seed) => merge(live$).pipe(startWith(seed))),
    shareReplay({ bufferSize: 1, refCount: false }),
  );
}

/** Process-wide singleton — same stream reused across AdminView mounts. */
let _admin$: Observable<Topology> | null = null;
export function admin$(): Observable<Topology> {
  if (!_admin$) {
    _admin$ = buildAdminStream$();
  }
  return _admin$;
}
