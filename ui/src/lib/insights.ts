/** Pure "ask the data" engine — answers a curated set of questions about the
 *  session fleet from the read-only sessions list. No writes, no LLM: the
 *  sovereignty-safe first step (Pattern 3). Each answer is structured data the
 *  Ask panel renders with drill-through; true free-text NL would need an LLM
 *  proxy (a later, still-read-only, step). Side-effect-free → unit-tested. */

import type { StorySession } from "@/lib/story-api";
import {
  computeFacets, projectKey, sessionDurationMs, sessionTokens, sortSessions, sessionDayKey, dayKey,
} from "@/lib/sessions-overview";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { formatDuration, relativeTimeFrom } from "@/lib/time";

export interface AnswerItem {
  readonly label: string;
  readonly sub?: string;
  readonly value: string;
  readonly sessionId?: string;
}
export interface Answer {
  readonly title: string;
  readonly note?: string;
  readonly items: AnswerItem[];
}
export interface Question { readonly id: string; readonly q: string }

export const QUESTIONS: Question[] = [
  { id: "latest", q: "What are my latest sessions?" },
  { id: "today", q: "What did I work on today?" },
  { id: "ongoing", q: "What's running right now?" },
  { id: "tokens", q: "Which sessions burned the most tokens?" },
  { id: "longest", q: "What are my longest sessions?" },
  { id: "projects", q: "Which projects am I most active in?" },
  { id: "agents", q: "Which agents do I use, and how efficiently?" },
];

function kfmt(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
function title(s: StorySession): string {
  return cleanHarnessPreview(s.label || s.first_prompt || s.session_id.slice(0, 8)).slice(0, 60) || s.session_id.slice(0, 8);
}
function ctx(s: StorySession): string {
  return [projectKey(s), s.origin_agent, s.user].filter(Boolean).join(" · ");
}

export function answer(sessions: readonly StorySession[], id: string, nowMs: number): Answer {
  const top = <T,>(arr: T[], n = 8) => arr.slice(0, n);

  switch (id) {
    case "latest": {
      const items = top(sortSessions(sessions, "recent")).map((s): AnswerItem => ({
        label: title(s), sub: ctx(s), value: s.last_event ? relativeTimeFrom(s.last_event, nowMs) : "—", sessionId: s.session_id,
      }));
      return { title: "Latest sessions", items };
    }
    case "today": {
      const today = dayKey(new Date(nowMs));
      const todays = sortSessions(sessions.filter((s) => sessionDayKey(s) === today), "recent");
      return {
        title: `Today (${today})`,
        note: `${todays.length} session${todays.length === 1 ? "" : "s"}`,
        items: top(todays).map((s): AnswerItem => ({ label: title(s), sub: ctx(s), value: s.last_event ? relativeTimeFrom(s.last_event, nowMs) : "—", sessionId: s.session_id })),
      };
    }
    case "ongoing": {
      const live = sortSessions(sessions.filter((s) => s.status === "ongoing"), "recent");
      return {
        title: "Running now",
        note: live.length ? `${live.length} ongoing` : "Nothing is running right now.",
        items: top(live).map((s): AnswerItem => ({ label: title(s), sub: ctx(s), value: s.start_time ? `started ${relativeTimeFrom(s.start_time, nowMs)}` : "—", sessionId: s.session_id })),
      };
    }
    case "tokens": {
      const items = top(sortSessions(sessions, "tokens")).map((s): AnswerItem => ({
        label: title(s), sub: ctx(s), value: `${kfmt(sessionTokens(s))} tok`, sessionId: s.session_id,
      }));
      return { title: "Biggest token burners", note: "input+output (cache excluded — see the token report)", items };
    }
    case "longest": {
      const items = top(sortSessions(sessions, "duration")).map((s): AnswerItem => ({
        label: title(s), sub: ctx(s), value: formatDuration(sessionDurationMs(s)), sessionId: s.session_id,
      }));
      return { title: "Longest sessions", items };
    }
    case "projects": {
      const items = top(computeFacets(sessions).projects).map((f): AnswerItem => ({
        label: f.key, value: `${f.count} session${f.count === 1 ? "" : "s"}`,
      }));
      return { title: "Most active projects", items };
    }
    case "agents": {
      const by = new Map<string, { n: number; ev: number; out: number }>();
      for (const s of sessions) {
        const a = s.origin_agent || "unknown";
        const cur = by.get(a) ?? { n: 0, ev: 0, out: 0 };
        cur.n += 1; cur.ev += s.event_count ?? 0; cur.out += s.total_output_tokens ?? 0;
        by.set(a, cur);
      }
      const items = [...by.entries()].sort((x, y) => y[1].n - x[1].n).map(([a, v]): AnswerItem => ({
        label: a,
        sub: `${v.n} sessions`,
        value: v.out > 0 && v.ev > 0 ? `${(v.out / v.ev).toFixed(0)} out-tok/event` : "no token telemetry",
      }));
      return { title: "Agents & efficiency", note: "output tokens produced per event", items };
    }
    default:
      return { title: "?", items: [] };
  }
}
