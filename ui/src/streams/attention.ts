/**
 * Reactive Attention store — the event-driven half of the attention tree.
 *
 *   control / navigate_to  ──►  fold  ──►  attention$  ──►  views (sink)
 *                                    │
 *                                    └── also syncs HashRoute via ports
 *
 * Views remain sinks: they subscribe and apply; they don't invent drives.
 * Functional core lives in lib/attention.ts; this is the reactive shell.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import { distinctUntilChanged, map, shareReplay } from "rxjs/operators";
import {
  attentionFromRoute,
  emptyAttention,
  foldIntent,
  foldControl,
  type Attention,
} from "@/lib/attention";
import type { HashRoute } from "@/lib/hash-route";
import type { NavigateToParams } from "@/lib/nav-path";
import type { UIControlAction } from "@/lib/ui-control";

const attentionSubject = new BehaviorSubject<Attention>(emptyAttention());

/** Hot stream of Attention (distinct by JSON spine for cheap compare). */
export function attention$(): Observable<Attention> {
  return attentionSubject.pipe(
    distinctUntilChanged((a, b) => attentionKey(a) === attentionKey(b)),
    shareReplay({ bufferSize: 1, refCount: true }),
  );
}

export function currentAttention(): Attention {
  return attentionSubject.getValue();
}

/** Seed / sync from the bookmarkable route (browser hash is source of truth for spine). */
export function syncAttentionFromRoute(route: HashRoute): void {
  const cur = attentionSubject.getValue();
  attentionSubject.next({
    ...cur,
    route,
    // hash navigation clears ephemeral spotlight unless route is still story+event
    spotlight:
      cur.spotlight?.kind === "event" &&
      route.view === "story" &&
      route.eventId === cur.spotlight.eventId
        ? cur.spotlight
        : null,
  });
}

/** Apply a pure Attention value (after foldIntent / foldControl). */
export function commitAttention(next: Attention): void {
  attentionSubject.next(next);
}

/** Fold a high-level intent into Attention and commit. Returns next or null. */
export function driveIntent(intent: NavigateToParams): Attention | null {
  const next = foldIntent(currentAttention(), intent);
  if (next) commitAttention(next);
  return next;
}

/** Fold a low-level control action into Attention and commit. */
export function driveControl(action: UIControlAction): Attention {
  const next = foldControl(currentAttention(), action);
  commitAttention(next);
  return next;
}

/** Canvas subtree only — for SessionsCanvas / ToolFlow sinks. */
export function canvasAttention$(): Observable<Attention["canvas"]> {
  return attention$().pipe(
    map((a) => a.canvas),
    distinctUntilChanged(
      (a, b) =>
        a.mode === b.mode &&
        a.selectedSessionId === b.selectedSessionId &&
        a.flowAgent === b.flowAgent &&
        a.groupBy === b.groupBy &&
        a.metric === b.metric &&
        (a.expandedKeys ?? []).join("\0") === (b.expandedKeys ?? []).join("\0") &&
        JSON.stringify(a.scatterBrush ?? null) === JSON.stringify(b.scatterBrush ?? null),
    ),
  );
}

function attentionKey(a: Attention): string {
  return JSON.stringify({
    r: a.route,
    s: a.spotlight,
    c: a.canvas,
    p: a.presentMessage,
  });
}

/** Test helper: reset store. */
export function _resetAttentionForTests(route: HashRoute = { view: "live" }): void {
  attentionSubject.next(attentionFromRoute(route));
}
