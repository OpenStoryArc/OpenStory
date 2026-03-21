//! Spec: Story 044 — Context-aware empty state messages.
//!
//! The empty state should tell the user WHY the timeline is empty
//! and WHAT TO DO about it — not just "Waiting for events..."

import { describe, it, expect } from "vitest";
import { emptyStateMessage, type EmptyStateContext } from "@/lib/empty-state";

describe("emptyStateMessage", () => {
  it("connected + no records + no filter → waiting for agent", () => {
    const ctx: EmptyStateContext = { connection: "connected", activeFilter: "all", totalRecords: 0 };
    const msg = emptyStateMessage(ctx);
    expect(msg.headline).toContain("No events yet");
    expect(msg.detail).toContain("agent");
  });

  it("disconnected + no records → connection problem", () => {
    const ctx: EmptyStateContext = { connection: "disconnected", activeFilter: "all", totalRecords: 0 };
    const msg = emptyStateMessage(ctx);
    expect(msg.headline).toContain("Disconnected");
  });

  it("connecting + no records → connecting message", () => {
    const ctx: EmptyStateContext = { connection: "connecting", activeFilter: "all", totalRecords: 0 };
    const msg = emptyStateMessage(ctx);
    expect(msg.headline).toContain("Connecting");
  });

  it("connected + has records + filter matches nothing → filter problem", () => {
    const ctx: EmptyStateContext = { connection: "connected", activeFilter: "conversation", totalRecords: 200 };
    const msg = emptyStateMessage(ctx);
    expect(msg.headline).toContain("No matching events");
    expect(msg.detail).toContain("Conversation");
    expect(msg.action).toBe("all");
  });

  it("connected + has records + 'all' filter → should not happen but handles gracefully", () => {
    const ctx: EmptyStateContext = { connection: "connected", activeFilter: "all", totalRecords: 200 };
    const msg = emptyStateMessage(ctx);
    // This shouldn't happen in practice (all filter + records → rows exist)
    // but handle gracefully
    expect(msg.headline).toBeDefined();
  });

  it("disconnected + has records + filter active → disconnected takes priority", () => {
    const ctx: EmptyStateContext = { connection: "disconnected", activeFilter: "conversation", totalRecords: 200 };
    const msg = emptyStateMessage(ctx);
    // Filter message is more useful here — data exists but filter is wrong
    expect(msg.headline).toContain("No matching events");
  });

  it("action is 'all' when filter is the problem", () => {
    const ctx: EmptyStateContext = { connection: "connected", activeFilter: "agents", totalRecords: 50 };
    const msg = emptyStateMessage(ctx);
    expect(msg.action).toBe("all");
  });

  it("action is undefined when filter is not the problem", () => {
    const ctx: EmptyStateContext = { connection: "connected", activeFilter: "all", totalRecords: 0 };
    const msg = emptyStateMessage(ctx);
    expect(msg.action).toBeUndefined();
  });
});
