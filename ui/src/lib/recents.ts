/** Frecency for recently-viewed sessions — pure ranking + edge persistence.
 *
 *  "Attention memory": rank sessions by how recently AND how often you've opened
 *  them, so a senior dev revisiting work sees where they just were. The ranking
 *  is a pure function (state + now → ids); localStorage is a thin edge wrapper.
 */

export interface RecentEntry {
  readonly id: string;
  readonly count: number;
  readonly lastVisit: number; // epoch ms
}

export interface RecentsState {
  readonly entries: RecentEntry[];
}

export const EMPTY_RECENTS: RecentsState = { entries: [] };

const MAX_ENTRIES = 50;

/** Recency weight in coarse buckets — recency dominates across buckets. */
function recencyScore(ageMs: number): number {
  const h = ageMs / 3_600_000;
  if (h < 1) return 100;
  if (h < 24) return 70;
  if (h < 24 * 7) return 40;
  if (h < 24 * 30) return 20;
  return 10;
}

/** Frecency score: recency bucket dominates; frequency breaks ties within it. */
function frecency(entry: RecentEntry, now: number): number {
  return recencyScore(now - entry.lastVisit) + entry.count * 2;
}

/** Record a visit to `id` at `now`, returning a new state (pure). */
export function recordVisit(state: RecentsState, id: string, now: number): RecentsState {
  const existing = state.entries.find((e) => e.id === id);
  const rest = state.entries.filter((e) => e.id !== id);
  const updated: RecentEntry = existing
    ? { id, count: existing.count + 1, lastVisit: now }
    : { id, count: 1, lastVisit: now };

  const entries = [updated, ...rest];
  if (entries.length <= MAX_ENTRIES) return { entries };

  // Evict the lowest-frecency entry to stay under the cap.
  const ranked = [...entries].sort((a, b) => frecency(b, now) - frecency(a, now));
  return { entries: ranked.slice(0, MAX_ENTRIES) };
}

/** Session ids ranked by frecency (best first). */
export function rankRecents(state: RecentsState, now: number): string[] {
  return [...state.entries]
    .sort((a, b) => frecency(b, now) - frecency(a, now) || b.lastVisit - a.lastVisit)
    .map((e) => e.id);
}

// ── edge persistence (localStorage) ────────────────────────────────────────

const STORAGE_KEY = "openstory.recents.v1";

export function loadRecents(): RecentsState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return EMPTY_RECENTS;
    const parsed = JSON.parse(raw);
    if (parsed && Array.isArray(parsed.entries)) return parsed as RecentsState;
  } catch {
    /* ignore corrupt/unavailable storage */
  }
  return EMPTY_RECENTS;
}

export function saveRecents(state: RecentsState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    /* ignore quota/unavailable storage */
  }
}
