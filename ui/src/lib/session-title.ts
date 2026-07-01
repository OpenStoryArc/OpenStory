/** The one place a session's human title is derived. Prefer the server label,
 *  fall back to the first prompt, humanize harness-wrapper text (e.g. a
 *  `<command-message>loop…` becomes "/loop"), and finally fall back to a short
 *  session id. Every surface (Overview, Explore, Story, Users, ⌘K, Canvas)
 *  routes through this so the same session reads the same way everywhere. */

import { cleanHarnessPreview } from "@/lib/harness-message";

/** Minimal shape needed to title a session — satisfied by both StorySession
 *  (which carries `label`) and SessionSummary (which doesn't). */
export interface TitleableSession {
  readonly session_id: string;
  readonly label?: string | null;
  readonly first_prompt?: string | null;
}

export function sessionTitle(s: TitleableSession): string {
  const raw = (s.label || s.first_prompt || "").trim();
  if (!raw) return s.session_id.slice(0, 8);
  return cleanHarnessPreview(raw).trim() || s.session_id.slice(0, 8);
}
