/** EventSpotlight — presentation mode: ONE event fills the screen, everything
 *  else dims. For narrated demos, driven through the agent-in-UI control seam
 *  (`focus_event { spotlight: true }`). Deliberately SIMPLE: dim backdrop,
 *  one centered card, the event's full text rendered large. Dismissed by Esc,
 *  a backdrop click, `toggle {target:"spotlight", value:"off"}`, or any
 *  subsequent view-changing control action (App owns those seams). */

import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "./markdown";
import { fetchSpotlightEvent, type SpotlightEvent as SpotlightData } from "@/lib/event-spotlight";
import { fullTimestamp } from "@/lib/time";

/** Code renders larger than the banner's 12px — this is a projector surface. */
const MD_LARGE = markdownComponents(14);

const ROLE_LABEL: Record<SpotlightData["role"], string> = {
  user: "user",
  assistant: "assistant",
  system: "system",
};

export function EventSpotlight({
  sessionId,
  eventId,
  onClose,
}: {
  sessionId: string;
  eventId: string;
  onClose: () => void;
}) {
  const [event, setEvent] = useState<SpotlightData | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "missing">("loading");

  // Fetch the event the same way the conversation view loads entries
  // (paged /conversation walk) — see lib/event-spotlight.ts.
  useEffect(() => {
    const ctrl = new AbortController();
    setState("loading");
    setEvent(null);
    fetchSpotlightEvent(sessionId, eventId, { signal: ctrl.signal })
      .then((ev) => {
        if (ctrl.signal.aborted) return;
        setEvent(ev);
        setState(ev ? "ready" : "missing");
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setState("missing");
      });
    return () => ctrl.abort();
  }, [sessionId, eventId]);

  // Esc dismisses — the human can always take the wheel back.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="spotlight-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Event spotlight"
      data-testid="event-spotlight"
    >
      {state === "loading" ? (
        // Minimal loading state: just the dimmed backdrop + a faint pulse.
        <div className="h-2 w-24 animate-pulse rounded-full bg-[color:var(--text-muted)]/50" />
      ) : (
        <div
          className="spotlight-card mx-4 w-full max-w-3xl rounded-xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)] p-8 text-[color:var(--text)] shadow-card"
          onClick={(e) => e.stopPropagation()}
        >
          {/* header: role · timestamp · session chip — quiet, muted */}
          <div className="mb-4 flex items-baseline gap-3">
            <span className="text-[length:var(--fs-label)] font-semibold uppercase tracking-wider text-[color:var(--text-muted)]">
              {event ? ROLE_LABEL[event.role] : "event"}
            </span>
            {event?.timestamp && (
              <span className="text-[length:var(--fs-label)] tabular-nums text-[color:var(--text-muted)]">
                {fullTimestamp(event.timestamp)}
              </span>
            )}
            <span className="ml-auto rounded-full border border-[color:var(--divider)] px-2 py-0.5 font-mono text-[10px] text-[color:var(--text-muted)]" title={sessionId}>
              {sessionId.slice(0, 8)}
            </span>
          </div>

          {/* body: the FULL event text, large enough to read on a video.
              Long events scroll — never truncate (project principle). */}
          {event ? (
            <div className="prose prose-lg max-w-none max-h-[70vh] overflow-y-auto leading-relaxed text-[color:var(--text)] marker:text-[color:var(--text-muted)] prose-headings:text-[color:var(--text)] prose-strong:text-[color:var(--text-bright)] prose-a:text-[color:var(--accent)] prose-blockquote:text-[color:var(--text-bright)]">
              <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_LARGE}>
                {event.text}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-[1.05rem] text-[color:var(--text-muted)]">
              Event {eventId.slice(0, 8)}… wasn’t found in this session’s recent history.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
