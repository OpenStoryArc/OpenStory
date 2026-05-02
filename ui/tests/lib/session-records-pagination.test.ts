/**
 * Spec: Timeline should load every record for a session, regardless of size.
 *
 * Background: PR #36 (feat/lazy-load-initial-state) introduced a paginated
 * /api/sessions/{id}/records endpoint with default limit=500, max=2000.
 * The server, when called with limit=500 against a session that has more
 * than 500 records, returns the *most recent 500* and silently drops the
 * older history (rs/server/src/api.rs:1289).
 *
 * `fetchSessionRecords` exposes a `beforeSeq` cursor for walking older
 * pages, but as of HEAD (master @ 0b01efb), no caller in ui/src/ ever
 * sends one — Timeline.tsx:321 fetches exactly one page of 500 and stops.
 * Sessions with >500 records render with their oldest events missing.
 *
 * This spec describes the contract the UI is expected to honor:
 * "When a user opens a session of any size, every record is eventually
 * loaded into the store." It fails today and is intended to drive the
 * fix (a backward cursor walk via `beforeSeq`, or a server-side switch
 * to streaming the full session).
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  fetchSessionRecords,
  fetchAllSessionRecords,
  streamSessionRecords,
  DEFAULT_PAGE_SIZE,
} from "@/lib/session-records";
import type { WireRecord } from "@/types/wire-record";
import {
  enrichedReducer,
  EMPTY_ENRICHED_STATE,
  type EnrichedAction,
} from "@/streams/sessions";

/** Build N synthetic records with monotonically increasing seq. */
function makeRecords(sessionId: string, count: number): WireRecord[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `evt-${i + 1}`,
    seq: i + 1,
    session_id: sessionId,
    timestamp: new Date(Date.UTC(2026, 0, 1, 0, 0, i)).toISOString(),
    record_type: "tool_call",
    payload: { call_id: `c${i}`, name: "Read", input: {}, raw_input: {}, typed_input: { tool: "read", file_path: "/x" } },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
  })) as WireRecord[];
}

/** Mimic the server: oldest-first within the most-recent `limit` records,
 *  filtered to seq < before_seq when provided. Mirrors api.rs:1275-1293. */
function serverRespond(all: WireRecord[], url: URL): WireRecord[] {
  const limit = Math.min(
    Math.max(parseInt(url.searchParams.get("limit") ?? "500", 10), 1),
    2000,
  );
  const beforeSeq = url.searchParams.get("before_seq");
  let filtered = beforeSeq != null ? all.filter((r) => r.seq < Number(beforeSeq)) : [...all];
  filtered.sort((a, b) => a.seq - b.seq);
  if (filtered.length > limit) filtered = filtered.slice(filtered.length - limit);
  return filtered;
}

describe("Timeline lazy-load pagination", () => {
  const SESSION_ID = "big-session";
  const TOTAL_RECORDS = 600; // exceeds DEFAULT_PAGE_SIZE
  let serverData: WireRecord[];
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    serverData = makeRecords(SESSION_ID, TOTAL_RECORDS);
    fetchSpy = vi.fn(async (input: string | URL) => {
      const url = new URL(typeof input === "string" ? `http://localhost${input}` : input);
      const records = serverRespond(serverData, url);
      return new Response(JSON.stringify(records), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    });
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("verifies the test scaffolding mirrors the server's pagination rule", async () => {
    // Sanity: limit=500 returns the most-recent 500 (seq 101..600).
    const first = await fetchSessionRecords(SESSION_ID, { limit: DEFAULT_PAGE_SIZE });
    expect(first).toHaveLength(DEFAULT_PAGE_SIZE);
    expect(first[0]!.seq).toBe(TOTAL_RECORDS - DEFAULT_PAGE_SIZE + 1);
    expect(first[first.length - 1]!.seq).toBe(TOTAL_RECORDS);

    // before_seq=101 returns the older 100 records (seq 1..100).
    const older = await fetchSessionRecords(SESSION_ID, {
      limit: DEFAULT_PAGE_SIZE,
      beforeSeq: first[0]!.seq,
    });
    expect(older).toHaveLength(TOTAL_RECORDS - DEFAULT_PAGE_SIZE);
    expect(older[0]!.seq).toBe(1);
    expect(older[older.length - 1]!.seq).toBe(TOTAL_RECORDS - DEFAULT_PAGE_SIZE);
  });

  /**
   * This is the failing spec.
   *
   * Reproduces what Timeline.tsx:321 does today: a single fetch with
   * limit=DEFAULT_PAGE_SIZE and no follow-up. Then asserts the user-visible
   * contract: every record for the session should end up loaded.
   *
   * Today this fails — only 500 of 600 records are loaded, the oldest 100
   * are silently dropped by the server, and the UI never asks for them.
   *
   * To make this pass, Timeline must either:
   *   (a) walk the cursor backward via `beforeSeq` until a short page returns, or
   *   (b) fetch with limit=2000 and accept the cap, plus add cursor walking
   *       for sessions above 2000 records.
   */
  it("should load every record for sessions larger than the default page size", async () => {
    const loaded = await fetchAllSessionRecords(SESSION_ID);

    // The contract: the user opened a session, every record should be visible.
    expect(loaded).toHaveLength(TOTAL_RECORDS);
    // Globally oldest-first.
    expect(loaded[0]!.seq).toBe(1);
    expect(loaded[loaded.length - 1]!.seq).toBe(TOTAL_RECORDS);
  });

  it("should issue a before_seq follow-up when the initial page hits the limit", async () => {
    await fetchAllSessionRecords(SESSION_ID);

    // If the first page came back full (limit hit), the loader keeps
    // walking older history via the cursor. Verify the follow-up
    // request was issued.
    const calls = fetchSpy.mock.calls.map((c) => String(c[0]));
    const followUp = calls.find((u) => u.includes("before_seq="));
    expect(followUp, "fetchAllSessionRecords should walk the before_seq cursor").toBeDefined();
  });

  it("should stop after a single fetch when the session fits in one page", async () => {
    serverData = makeRecords(SESSION_ID, 10);
    await fetchAllSessionRecords(SESSION_ID);
    // First page returned 10 records (< pageSize) → loop exits immediately.
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });
});

/**
 * Spec: streaming dispatch — records should reach the UI page-by-page so
 * users see something within one round-trip rather than waiting for the
 * full cursor walk to complete.
 *
 * Drives the design of `streamSessionRecords()`, an async generator that
 * yields each page as it arrives. Timeline iterates and dispatches each
 * page through the reducer; partial states are valid because
 * `mergeUniqueById` already dedups id-overlap with live `enriched` deltas.
 */
describe("streamSessionRecords — progressive dispatch", () => {
  const SESSION_ID = "stream-session";
  const TOTAL_RECORDS = 600;
  let serverData: WireRecord[];
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    serverData = makeRecords(SESSION_ID, TOTAL_RECORDS);
    fetchSpy = vi.fn(async (input: string | URL) => {
      const url = new URL(typeof input === "string" ? `http://localhost${input}` : input);
      const records = serverRespond(serverData, url);
      return new Response(JSON.stringify(records), { status: 200 });
    });
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("should yield each page as a separate iteration step", async () => {
    const pageSizes: number[] = [];
    for await (const page of streamSessionRecords(SESSION_ID)) {
      pageSizes.push(page.length);
    }
    // 600 records / 500 per page = 2 pages: [500 newest, 100 older]
    expect(pageSizes).toEqual([DEFAULT_PAGE_SIZE, TOTAL_RECORDS - DEFAULT_PAGE_SIZE]);
  });

  it("should yield pages newest-window-first so the recent view paints first", async () => {
    const seqsPerPage: Array<[number, number]> = [];
    for await (const page of streamSessionRecords(SESSION_ID)) {
      seqsPerPage.push([page[0]!.seq, page[page.length - 1]!.seq]);
    }
    // First yielded page covers the most-recent records (101..600);
    // second page covers the older history (1..100).
    expect(seqsPerPage[0]).toEqual([101, 600]);
    expect(seqsPerPage[1]).toEqual([1, 100]);
  });

  it("should yield exactly one page when the session fits", async () => {
    serverData = makeRecords(SESSION_ID, 200);
    let yields = 0;
    for await (const page of streamSessionRecords(SESSION_ID)) {
      expect(page).toHaveLength(200);
      yields++;
    }
    expect(yields).toBe(1);
  });

  it("should respect AbortSignal mid-stream", async () => {
    const ctrl = new AbortController();
    let yields = 0;
    const iterate = async () => {
      for await (const _page of streamSessionRecords(SESSION_ID, { signal: ctrl.signal })) {
        yields++;
        ctrl.abort(); // cancel after first page
      }
    };
    await expect(iterate()).rejects.toThrow();
    expect(yields).toBe(1);
  });

  it("should make the user-visible state correct when the consumer dispatches each page", () => {
    // Drive the reducer with the actions Timeline would dispatch as it
    // iterates the stream. Use a synthetic page sequence (newest first,
    // mirroring what streamSessionRecords yields).
    const newest = serverData.slice(100); // seq 101..600
    const older = serverData.slice(0, 100); // seq 1..100

    const actions: EnrichedAction[] = [
      { kind: "session_records_loaded", session_id: SESSION_ID, records: newest },
      { kind: "session_records_loaded", session_id: SESSION_ID, records: older },
    ];

    const final = actions.reduce(enrichedReducer, EMPTY_ENRICHED_STATE);

    // Reducer dedups by id; the union should equal all 600 records.
    expect(final.records).toHaveLength(TOTAL_RECORDS);
    expect(final.loadedSessions.has(SESSION_ID)).toBe(true);

    // Tree index includes every record that came through.
    expect(final.treeIndex.size).toBe(TOTAL_RECORDS);
  });
});
