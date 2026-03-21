/** Context-aware empty state messages for the timeline.
 *
 * Instead of a single "Waiting for events..." message, we tell the user
 * WHY the timeline is empty and WHAT TO DO about it. */

import { FILTER_LABELS } from "@/lib/ui-labels";
import type { ConnectionStatus } from "@/streams/connection";

export interface EmptyStateContext {
  readonly connection: ConnectionStatus;
  readonly activeFilter: string;
  readonly totalRecords: number;
}

export interface EmptyStateMessage {
  readonly headline: string;
  readonly detail: string;
  /** If set, clicking should switch to this filter (e.g., "all"). */
  readonly action?: string;
}

export function emptyStateMessage(ctx: EmptyStateContext): EmptyStateMessage {
  // Case 1: Filter is active and there are records — filter is the problem
  if (ctx.activeFilter !== "all" && ctx.totalRecords > 0) {
    const filterLabel = FILTER_LABELS[ctx.activeFilter] ?? ctx.activeFilter;
    return {
      headline: "No matching events",
      detail: `No events match the "${filterLabel}" filter. Try "All" to see everything.`,
      action: "all",
    };
  }

  // Case 2: Disconnected with no data
  if (ctx.connection === "disconnected" && ctx.totalRecords === 0) {
    return {
      headline: "Disconnected from server",
      detail: "Cannot reach the Open Story server. Check that it's running and refresh the page.",
    };
  }

  // Case 3: Connecting
  if (ctx.connection === "connecting") {
    return {
      headline: "Connecting to server...",
      detail: "Establishing WebSocket connection.",
    };
  }

  // Case 4: Connected, no records, no filter — genuinely empty
  return {
    headline: "No events yet",
    detail: "Start a Claude agent to see its work here in real time.",
  };
}
