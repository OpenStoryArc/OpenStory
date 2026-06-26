//! Spec: agent-directed watch focus reception.
//!
//! When the server pushes a `focus` message (from POST /api/watch/{id}),
//! the UI reacts context-aware: if it's already on the Live tab it switches
//! focus instantly; on any other tab it shows a dismissible "Follow" banner
//! so the user is never yanked off what they're doing.

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  decideWatchAction,
  isFocusMessage,
} from "@/lib/watch-focus";
import type { FocusMessage } from "@/types/websocket";
import type { WsMessage } from "@/types/websocket";

const focusMsg: FocusMessage = {
  kind: "focus",
  session_id: "sess-a1",
  label: "Where are we at?",
  project_name: "agent-harness",
  host: "a1",
  user: "max",
};

describe("decideWatchAction", () => {
  it("navigates instantly when already on the Live tab", () => {
    scenario(
      () => ({ view: "live" as const, msg: focusMsg }),
      ({ view, msg }) => decideWatchAction(view, msg),
      (action) =>
        expect(action).toEqual({ type: "navigate", sessionId: "sess-a1" }),
    );
  });

  it("shows a banner when on the Explore tab", () => {
    scenario(
      () => ({ view: "explore" as const, msg: focusMsg }),
      ({ view, msg }) => decideWatchAction(view, msg),
      (action) => expect(action).toEqual({ type: "banner", message: focusMsg }),
    );
  });

  it("shows a banner on every non-Live tab", () => {
    for (const view of ["explore", "story", "users", "admin"] as const) {
      scenario(
        () => ({ view, msg: focusMsg }),
        ({ view, msg }) => decideWatchAction(view, msg),
        (action) => expect(action.type).toBe("banner"),
      );
    }
  });
});

describe("isFocusMessage", () => {
  it("narrows a focus message", () => {
    scenario(
      () => focusMsg as WsMessage,
      (msg) => isFocusMessage(msg),
      (ok) => expect(ok).toBe(true),
    );
  });

  it("rejects a non-focus message", () => {
    scenario(
      () => ({ kind: "plan_saved", session_id: "x" }) as WsMessage,
      (msg) => isFocusMessage(msg),
      (ok) => expect(ok).toBe(false),
    );
  });
});
