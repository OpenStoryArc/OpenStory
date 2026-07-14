/** PresentBanner — the agent's narration bar.
 *
 *  IN-FLOW under the header (never floats, never covers the record it
 *  narrates — occlusion was the floating panel's design flaw): a slim,
 *  content-width line of avatar · name · latest message · time · controls.
 *  Click the message to expand it; "n earlier" unfolds the session-lifetime
 *  thread DOWNWARD in flow. Markdown in full; session chips on expand;
 *  dismiss hides until the next message. Agent-in-UI seam: shows things,
 *  never mutates sources.
 */

import { useEffect, useState } from "react";
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
  const [expanded, setExpanded] = useState(false);
  const [showLog, setShowLog] = useState(false);

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

  // New message arrives collapsed — the bar stays one line tall.
  useEffect(() => setExpanded(false), [message]);

  const earlier = LOG.slice(0, -1).reverse();
  const latest = LOG[LOG.length - 1];

  return (
    <div className="flex justify-center px-4 pt-2" data-testid="present-banner">
      <div className="w-full max-w-[56rem] overflow-hidden rounded-xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)]">
        {/* resting state: one quiet line — avatar · name · message · time · controls */}
        <div className="flex items-center gap-2.5 px-3 py-1.5">
          <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[color:var(--accent)] text-[10px] font-bold text-[color:var(--bg-surface)]">
            {issuer.replace(/^Claude\s*/i, "C").slice(0, 1).toUpperCase()}
          </span>
          <span className="shrink-0 text-[length:var(--fs-body)] font-semibold text-[color:var(--text)]">
            {issuer}
          </span>
          <div
            className={cn(
              "prose prose-sm min-w-0 flex-1 text-[length:var(--fs-body)] leading-snug text-[color:var(--text)] marker:text-[color:var(--text-muted)] [&_p]:my-0",
              !expanded && "truncate [&_p]:inline [&_*]:inline",
            )}
            onClick={() => setExpanded((v) => !v)}
            title={expanded ? undefined : "Click to expand"}
            role="button"
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
              {message}
            </ReactMarkdown>
          </div>
          <span className="shrink-0 text-[length:var(--fs-label)] tabular-nums text-[color:var(--text-muted)]">
            {latest ? compactTime(latest.at) : ""}
          </span>
          {latest?.route && (
            <button
              onClick={() => onNavigate(latest.route!)}
              className="shrink-0 rounded-full bg-[color:var(--accent)] px-2 py-0.5 text-[11px] font-medium text-[color:var(--bg-surface)] hover:opacity-90"
            >
              Open →
            </button>
          )}
          {earlier.length > 0 && (
            <button
              onClick={() => setShowLog((v) => !v)}
              className="shrink-0 rounded-full border border-[color:var(--divider)] px-2 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
              aria-expanded={showLog}
            >
              {showLog ? "hide" : `${earlier.length} earlier`}
            </button>
          )}
          <button
            onClick={onDismiss}
            className="shrink-0 rounded px-1 text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
            title="Dismiss"
            aria-label="Dismiss"
          >
            ×
          </button>
        </div>

        {/* expanded extras of the latest message: session chips */}
        {expanded && latest && latest.sessionIds.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 px-3 pb-2">
            {latest.sessionIds.slice(0, 6).map((id) => (
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

        {/* the thread — expands DOWNWARD in flow, never covers content */}
        {showLog && earlier.length > 0 && (
          <div className="max-h-52 space-y-1.5 overflow-y-auto border-t border-[color:var(--divider)] px-3 py-2">
            {earlier.map((e, i) => (
              <div key={`${e.at}-${i}`} className="flex items-baseline gap-2">
                <span className="shrink-0 text-[length:var(--fs-label)] tabular-nums text-[color:var(--text-muted)]">
                  {compactTime(e.at)}
                </span>
                <span className="min-w-0 flex-1 text-[length:var(--fs-body)] leading-snug text-[color:var(--text-bright)]">
                  {e.message}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
