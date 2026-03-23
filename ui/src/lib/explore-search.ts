/** Search utilities for the Explore event timeline.
 *  Searches across WireRecord payload fields — text, tool names, file paths, commands. */

import type { WireRecord } from "@/types/wire-record";

/** Filter records to those matching a search query (case-insensitive). */
export function searchRecords(records: readonly WireRecord[], query: string): WireRecord[] {
  if (!query.trim()) return [...records];
  const q = query.toLowerCase();
  return records.filter((r) => recordMatchesQuery(r, q));
}

/** Check if a record matches a search query. */
function recordMatchesQuery(r: WireRecord, q: string): boolean {
  // Check record type
  if (r.record_type.toLowerCase().includes(q)) return true;

  const payload = r.payload as Record<string, unknown>;

  // Text content (user messages, assistant messages, thinking)
  if (typeof payload.text === "string" && payload.text.toLowerCase().includes(q)) return true;

  // Tool name
  if (typeof payload.name === "string" && payload.name.toLowerCase().includes(q)) return true;

  // Tool input fields
  const rawInput = payload.raw_input as Record<string, unknown> | undefined;
  if (rawInput) {
    if (matchesAnyStringField(rawInput, q)) return true;
  }

  // Tool result output
  if (typeof payload.output === "string" && payload.output.toLowerCase().includes(q)) return true;

  return false;
}

/** Check if any string field in an object matches the query. */
function matchesAnyStringField(obj: Record<string, unknown>, q: string): boolean {
  for (const value of Object.values(obj)) {
    if (typeof value === "string" && value.toLowerCase().includes(q)) return true;
  }
  return false;
}

/** Segment a string into parts for highlighting matches. */
export interface HighlightSegment {
  readonly text: string;
  readonly isMatch: boolean;
}

/** Split text into segments, marking portions that match the query. */
export function highlightMatch(text: string, query: string): HighlightSegment[] {
  if (!query.trim() || !text) return [{ text, isMatch: false }];

  const segments: HighlightSegment[] = [];
  const lowerText = text.toLowerCase();
  const lowerQuery = query.toLowerCase();
  let lastIndex = 0;

  let searchFrom = 0;
  while (searchFrom < lowerText.length) {
    const matchIndex = lowerText.indexOf(lowerQuery, searchFrom);
    if (matchIndex === -1) break;

    // Text before match
    if (matchIndex > lastIndex) {
      segments.push({ text: text.slice(lastIndex, matchIndex), isMatch: false });
    }

    // The match itself (preserving original case)
    segments.push({ text: text.slice(matchIndex, matchIndex + query.length), isMatch: true });

    lastIndex = matchIndex + query.length;
    searchFrom = lastIndex;
  }

  // Remaining text after last match
  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), isMatch: false });
  }

  return segments.length > 0 ? segments : [{ text, isMatch: false }];
}
