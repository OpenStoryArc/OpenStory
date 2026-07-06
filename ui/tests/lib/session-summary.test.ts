import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildSessionSummary } from "@/lib/session-summary";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, ts: string, seq: number, payload: unknown = {}): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: ts, record_type,
    payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null,
    truncated: false, payload_bytes: 50,
  };
}
const T = (s: number) => `2026-06-30T10:00:${String(s).padStart(2, "0")}.000Z`;

describe("buildSessionSummary", () => {
  describe("when empty", () => {
    it("returns zeros and null timing", () => {
      scenario(
        () => buildSessionSummary([]),
        (s) => s,
        (s) => {
          expect(s.eventCount).toBe(0);
          expect(s.durationMs).toBe(0);
          expect(s.startMs).toBeNull();
          expect(s.topFiles).toEqual([]);
        },
      );
    });
  });

  it("computes duration from first to last record timestamp", () => {
    scenario(
      () => [rec("user_message", T(0), 1), rec("assistant_message", T(30), 2, { model: "claude-opus-4-8", content: [] })],
      (records) => buildSessionSummary(records),
      (s) => {
        expect(s.durationMs).toBe(30_000);
        expect(s.startMs).toBe(Date.parse(T(0)));
        expect(s.model).toBe("claude-opus-4-8");
      },
    );
  });

  it("counts tools, errors, and turns", () => {
    scenario(
      () => [
        rec("user_message", T(0), 1),
        rec("tool_call", T(1), 2, { call_id: "c1", name: "Bash", typed_input: { tool: "bash", command: "x" } }),
        rec("tool_result", T(2), 3, { call_id: "c1", is_error: true }),
        rec("tool_call", T(3), 4, { call_id: "c2", name: "Read", typed_input: { tool: "read", file_path: "/a.ts" } }),
        rec("tool_result", T(4), 5, { call_id: "c2", is_error: false }),
        rec("error", T(5), 6, { code: "x", message: "boom" }),
        rec("turn_end", T(6), 7, {}),
        rec("turn_end", T(7), 8, {}),
      ],
      (records) => buildSessionSummary(records),
      (s) => {
        expect(s.toolCount).toBe(2);
        expect(s.errorCount).toBe(2); // one errored tool_result + one error record
        expect(s.turnCount).toBe(2);
      },
    );
  });

  it("sums turn-scoped token usage incl. cache, ignoring session_total snapshots", () => {
    scenario(
      () => [
        rec("token_usage", T(1), 1, { input_tokens: 1000, output_tokens: 200, cache_creation_input_tokens: 300, cache_read_input_tokens: 50000, scope: "turn" }),
        rec("token_usage", T(2), 2, { input_tokens: 9999, output_tokens: 9999, cache_read_input_tokens: 9999, scope: "session_total" }),
        rec("token_usage", T(3), 3, { input_tokens: 500, output_tokens: 50, cache_read_input_tokens: 20000, scope: "turn" }),
      ],
      (records) => buildSessionSummary(records),
      (s) => {
        expect(s.inputTokens).toBe(1500);
        expect(s.outputTokens).toBe(250);
        expect(s.cacheCreationTokens).toBe(300);
        expect(s.cacheReadTokens).toBe(70000);
        // total now includes cache — the whole point (was 1750, undercounting ~40×)
        expect(s.totalTokens).toBe(1500 + 250 + 300 + 70000);
      },
    );
  });

  it("ranks the most-touched files from file tool inputs", () => {
    scenario(
      () => [
        rec("tool_call", T(1), 1, { call_id: "1", name: "Read", typed_input: { tool: "read", file_path: "/a.ts" } }),
        rec("tool_call", T(2), 2, { call_id: "2", name: "Edit", typed_input: { tool: "edit", file_path: "/a.ts" } }),
        rec("tool_call", T(3), 3, { call_id: "3", name: "Write", typed_input: { tool: "write", file_path: "/b.ts" } }),
        rec("tool_call", T(4), 4, { call_id: "4", name: "Bash", typed_input: { tool: "bash", command: "ls" } }),
      ],
      (records) => buildSessionSummary(records),
      (s) => {
        expect(s.topFiles[0]).toEqual({ path: "/a.ts", count: 2 });
        expect(s.topFiles.map((f) => f.path)).toEqual(["/a.ts", "/b.ts"]);
      },
    );
  });
});
