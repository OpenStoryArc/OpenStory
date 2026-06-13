/**
 * Shape-layer detail — types + pure transforms over `/api/sessions/{id}/shapes`.
 *
 * Mirrors the Rust `ShapeRow` (rs/shapes) and the prototype DB columns. All
 * functions here are pure (data in, ranked data out) so they're unit-tested in
 * isolation; the fetch + render lives in `ShapesPanel.tsx`.
 */

import type { ShapeRow } from "@/types/websocket";

export type { ShapeRow };

export interface RankedItem {
  readonly value: string;
  readonly count: number;
}

/**
 * Count occurrences of `key` across rows of `shapeType`, ranked descending
 * (ties broken by value asc — matches the Rust cross-shape ordering). Handles
 * both scalar string fields (program, top_segment) and array fields
 * (naming_tokens, flags) by unnesting arrays.
 */
export function rankField(
  rows: readonly ShapeRow[],
  shapeType: string,
  key: string,
  limit = 15,
): RankedItem[] {
  const counts = new Map<string, number>();
  for (const row of rows) {
    if (row.shape_type !== shapeType) continue;
    const raw = row.data[key];
    const values = Array.isArray(raw) ? raw : raw == null ? [] : [raw];
    for (const v of values) {
      const s = String(v);
      if (s === "") continue;
      counts.set(s, (counts.get(s) ?? 0) + 1);
    }
  }
  return Array.from(counts.entries())
    .map(([value, count]) => ({ value, count }))
    .sort((a, b) => b.count - a.count || a.value.localeCompare(b.value))
    .slice(0, limit);
}

/** Sum a numeric `change-shape` field across all change rows. */
export function sumChangeField(rows: readonly ShapeRow[], key: string): number {
  let total = 0;
  for (const row of rows) {
    if (row.shape_type !== "change-shape") continue;
    const v = row.data[key];
    if (typeof v === "number") total += v;
  }
  return total;
}

/** Count rows of a given shape type. */
export function countByType(rows: readonly ShapeRow[], shapeType: string): number {
  return rows.reduce((n, r) => (r.shape_type === shapeType ? n + 1 : n), 0);
}
