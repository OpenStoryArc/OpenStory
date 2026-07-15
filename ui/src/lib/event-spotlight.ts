/** Data side of the Event Spotlight (presentation mode): locate ONE event
 *  inside a session's conversation and reduce it to a renderable
 *  {role, timestamp, text}. Reuses the exact fetch path the conversation view
 *  uses — GET /api/sessions/{id}/conversation with `before_seq` paging — and
 *  the same payload text extraction (textFromContent, as in event-graph.ts).
 *  Nothing is ever truncated (project principle): the FULL text is returned;
 *  the overlay scrolls instead. */

import type { ConversationEntry, ViewRecord } from "@/types/view-record";
import { textFromContent } from "@/lib/message-images";

export interface SpotlightEvent {
  readonly role: "user" | "assistant" | "system";
  /** ISO timestamp of the event (null when the entry carries none). */
  readonly timestamp: string | null;
  /** Full markdown-renderable text — never truncated. */
  readonly text: string;
}

/** Events per page — matches ConversationView's window. */
export const SPOTLIGHT_PAGE_EVENTS = 500;
/** Bounded backward walk: 20 pages × 500 events covers any demo-scale session
 *  without letting a bad eventId walk an unbounded history. */
export const SPOTLIGHT_MAX_PAGES = 20;

interface PagedConversation {
  entries?: ConversationEntry[];
  next_before_seq?: number;
}

/** Does this entry carry `eventId`? tool_roundtrips hide their ids inside the
 *  call/result ViewRecords; every other entry has a top-level id. */
export function entryMatchesEvent(entry: ConversationEntry, eventId: string): boolean {
  if (entry.entry_type === "tool_roundtrip") {
    return entry.call.id === eventId || entry.result?.id === eventId;
  }
  return entry.id === eventId;
}

/** Best-effort full text of a ViewRecord payload (tool call/result). */
function recordText(record: ViewRecord): string {
  const payload = record.payload as Record<string, unknown>;
  const raw =
    (typeof payload?.text === "string" ? payload.text : undefined) ??
    textFromContent(payload?.content as never);
  if (raw && raw.trim()) return raw;
  // No prose to show — render the structured payload honestly, in full.
  return "```json\n" + JSON.stringify(payload ?? null, null, 2) + "\n```";
}

/** Reduce a matched entry to the spotlight's {role, timestamp, text}. */
export function spotlightFromEntry(entry: ConversationEntry): SpotlightEvent {
  switch (entry.entry_type) {
    case "user_message":
      return {
        role: "user",
        timestamp: entry.timestamp,
        text: textFromContent(entry.payload.content),
      };
    case "assistant_message":
      return {
        role: "assistant",
        timestamp: entry.timestamp,
        text: textFromContent(entry.payload.content),
      };
    case "reasoning":
      return {
        role: "assistant",
        timestamp: entry.timestamp,
        text: entry.payload.content ?? entry.payload.summary.join("\n"),
      };
    case "tool_roundtrip":
      return {
        role: "system",
        timestamp: entry.call.timestamp ?? null,
        text: recordText(entry.result ?? entry.call),
      };
    default:
      return {
        role: "system",
        timestamp: entry.timestamp ?? null,
        text: recordText(entry as unknown as ViewRecord),
      };
  }
}

/** Find `eventId` in `sessionId`'s conversation, walking `before_seq` pages
 *  backward (newest window first, then older) up to SPOTLIGHT_MAX_PAGES.
 *  Returns null when the event isn't in the walked window. `fetchImpl` is
 *  injectable for tests. */
export async function fetchSpotlightEvent(
  sessionId: string,
  eventId: string,
  opts: { signal?: AbortSignal; fetchImpl?: typeof fetch } = {},
): Promise<SpotlightEvent | null> {
  const doFetch = opts.fetchImpl ?? fetch;
  const base = `/api/sessions/${encodeURIComponent(sessionId)}/conversation`;
  let beforeSeq: number | null = null;

  for (let page = 0; page < SPOTLIGHT_MAX_PAGES; page++) {
    const url =
      beforeSeq == null
        ? `${base}?limit=${SPOTLIGHT_PAGE_EVENTS}`
        : `${base}?limit=${SPOTLIGHT_PAGE_EVENTS}&before_seq=${beforeSeq}`;
    const res = await doFetch(url, { signal: opts.signal });
    if (!res.ok) throw new Error(`fetchSpotlightEvent ${sessionId}: ${res.status}`);
    const data = (await res.json()) as PagedConversation;
    const entries = data.entries ?? [];
    const hit = entries.find((e) => entryMatchesEvent(e, eventId));
    if (hit) return spotlightFromEntry(hit);
    if (data.next_before_seq == null) return null; // history exhausted
    beforeSeq = data.next_before_seq;
  }
  return null;
}
