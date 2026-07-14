/** PresentBanner — the agent's chat panel.
 *
 *  Agent `present` messages arrive as a narrow chat thread (iMessage idiom):
 *  incoming bubbles from the agent, newest at the bottom, anchored top-right
 *  so it reads as a companion to the page rather than a banner across it.
 *  The thread is session-lifetime — narrator beats accumulate. Markdown in
 *  full; session chips ride inside the bubble; dismiss hides the panel until
 *  the next message. Agent-in-UI seam: shows things, never mutates sources.
 */

import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import type { HashRoute } from "@/lib/hash-route";
import { compactTime } from "@/lib/time";
import { cn } from "@/lib/cn";

export interface Presentation {
  readonly issuer: string;
  readonly message: string;
  readonly sessionIds: readonly string[];
  readonly route: HashRoute | null;
}

interface HistoryEntry extends Presentation {
  readonly at: string; // ISO timestamp, stamped on arrival
}

/** Session-lifetime chat log — survives dismiss/remount (module scope). */
const LOG: HistoryEntry[] = [];
const LOG_MAX = 30;

/** Markdown renderers: fenced/`language-*` code → Prism highlight, inline → chip. */
const MD_COMPONENTS = {
  code({ className, children, ...rest }: { className?: string; children?: React.ReactNode }) {
    const match = /language-(\w+)/.exec(className || "");
    const raw = String(children ?? "");
    const isBlock = Boolean(match) || raw.includes("\n");
    if (isBlock) {
      return (
        <SyntaxHighlighter
          language={match?.[1] ?? "text"}
          style={oneDark}
          PreTag="div"
          customStyle={{ margin: "4px 0", padding: "8px 10px", background: "#1a1b26", borderRadius: 8, fontSize: 12 }}
        >
          {raw.replace(/\n$/, "")}
        </SyntaxHighlighter>
      );
    }
    return (
      <code className="rounded bg-[color:var(--bg)]/60 px-1 py-0.5 font-mono text-[0.9em] text-[color:var(--cyan-bright)]" {...rest}>
        {children}
      </code>
    );
  },
};

function Bubble({
  entry,
  latest,
  onNavigate,
}: {
  entry: HistoryEntry;
  latest: boolean;
  onNavigate: (route: HashRoute) => void;
}) {
  return (
    <div className={cn("chat-enter flex flex-col items-start gap-0.5", !latest && "opacity-80")}>
      <div
        className={cn(
          "max-w-full rounded-2xl rounded-tl-md px-3 py-2",
          latest
            ? "bg-[color:var(--accent)]/10 border border-[color:var(--accent)]/25"
            : "bg-[color:var(--bg-hover)]/40 border border-[color:var(--divider)]",
        )}
      >
        <div className="prose prose-sm max-w-none break-words text-[length:var(--fs-emph)] leading-snug text-[color:var(--text)] marker:text-[color:var(--text-muted)] [&_p]:my-0.5">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
            {entry.message}
          </ReactMarkdown>
        </div>
        {entry.sessionIds.length > 0 && (
          <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
            {entry.sessionIds.slice(0, 6).map((id) => (
              <button
                key={id}
                data-present-session={id}
                onClick={() => onNavigate({ view: "explore", sessionId: id })}
                className="rounded-full border border-[color:var(--divider)] px-2 py-0.5 font-mono text-[10px] text-[color:var(--accent)] hover:border-[color:var(--accent)]"
                title={id}
              >
                {id.slice(0, 8)}
              </button>
            ))}
          </div>
        )}
        {latest && entry.route && (
          <button
            onClick={() => onNavigate(entry.route!)}
            className="mt-1.5 rounded-full bg-[color:var(--accent)] px-2.5 py-0.5 text-[11px] font-medium text-[color:var(--bg-surface)] hover:opacity-90"
          >
            Open →
          </button>
        )}
      </div>
      <span className="pl-1 text-[10px] tabular-nums text-[color:var(--text-muted)]">
        {compactTime(entry.at)}
      </span>
    </div>
  );
}

export function PresentBanner({
  present,
  onNavigate,
  onDismiss,
}: {
  present: Presentation;
  onNavigate: (route: HashRoute) => void;
  onDismiss: () => void;
}) {
  const { issuer, message } = present;
  const [, bump] = useState(0);
  const threadRef = useRef<HTMLDivElement>(null);

  // Append each new arrival to the session-lifetime log (dedupe re-renders).
  useEffect(() => {
    const last = LOG[LOG.length - 1];
    if (!last || last.message !== message || last.issuer !== issuer) {
      LOG.push({ ...present, at: new Date().toISOString() });
      if (LOG.length > LOG_MAX) LOG.splice(0, LOG.length - LOG_MAX);
      bump((n) => n + 1);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [present]);

  // Newest bubble stays in view.
  useEffect(() => {
    const el = threadRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [LOG.length]);

  return (
    <div
      className="fixed right-4 top-14 z-40 w-[min(92vw,22rem)]"
      data-testid="present-banner"
    >
      <div className="shadow-card overflow-hidden rounded-2xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)]/95 backdrop-blur-sm">
        {/* header — as quiet as a contact card */}
        <div className="flex items-center gap-2 px-3 py-2">
          <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-[color:var(--accent)] text-[11px] font-bold text-[color:var(--bg-surface)]">
            {issuer.replace(/^Claude\s*/i, "C").slice(0, 1).toUpperCase()}
          </span>
          <span className="truncate text-[length:var(--fs-body)] font-semibold text-[color:var(--text)]">
            {issuer}
          </span>
          <button
            onClick={onDismiss}
            className="ml-auto rounded px-1.5 text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
            title="Dismiss"
            aria-label="Dismiss"
          >
            ×
          </button>
        </div>

        {/* the thread — oldest to newest, newest pinned into view */}
        <div ref={threadRef} className="flex max-h-[46vh] flex-col gap-2.5 overflow-y-auto px-3 pb-3">
          {LOG.map((e, i) => (
            <Bubble
              key={`${e.at}-${i}`}
              entry={e}
              latest={i === LOG.length - 1}
              onNavigate={onNavigate}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
