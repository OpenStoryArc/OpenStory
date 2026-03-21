import { interval, map, startWith, shareReplay } from "rxjs";

/** Emits the current timestamp every 2 seconds. Shared across all consumers. */
export const tick$ = interval(2000).pipe(
  startWith(0),
  map(() => Date.now()),
  shareReplay(1),
);
