import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TokenReport, cacheHitRate } from "@/components/viz/TokenReport";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function tu(seq: number, p: Record<string, unknown>): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: `2026-06-30T10:00:0${seq}.000Z`,
    record_type: "token_usage" as RecordType, payload: { scope: "turn", ...p } as WireRecord["payload"],
    origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0,
    parent_uuid: null, truncated: false, payload_bytes: 50,
  };
}

describe("cacheHitRate", () => {
  it("is the share of input tokens served from cache", () => {
    // 90 cache-read out of (90 read + 10 write + 0 fresh input) = 90%
    expect(cacheHitRate({ inputTokens: 0, cacheCreationTokens: 10, cacheReadTokens: 90 })).toBeCloseTo(0.9);
  });
  it("is 0 when there is no input at all", () => {
    expect(cacheHitRate({ inputTokens: 0, cacheCreationTokens: 0, cacheReadTokens: 0 })).toBe(0);
  });
});

describe("TokenReport", () => {
  const RECORDS = [
    tu(1, { input_tokens: 1000, output_tokens: 2000, cache_creation_input_tokens: 5000, cache_read_input_tokens: 900000 }),
  ];

  it("reports every token category incl. cache read, with exact counts", () => {
    render(<TokenReport records={RECORDS} />);
    // the cache-read total the old UI hid entirely
    expect(screen.getByText("Cache read")).toBeInTheDocument();
    expect(screen.getByText("900,000")).toBeInTheDocument();
    expect(screen.getByText("Output")).toBeInTheDocument();
    expect(screen.getByText("2,000")).toBeInTheDocument();
    // one bar segment per non-zero category
    expect(document.querySelectorAll("[data-token-seg]").length).toBe(4);
  });

  it("shows a cache-hit-rate stat", () => {
    render(<TokenReport records={RECORDS} />);
    // 900000 / (900000 + 5000 + 1000) ~= 99%  (shown as the hit-rate + legend %)
    expect(screen.getByText(/from cache/i)).toBeInTheDocument();
    expect(screen.getAllByText(/99%/).length).toBeGreaterThanOrEqual(1);
  });

  it("renders an empty state when there is no token data", () => {
    render(<TokenReport records={[]} />);
    expect(screen.getByText(/no token data/i)).toBeInTheDocument();
  });
});
