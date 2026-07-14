/** PresentBanner — the agent's messaging panel.
 *
 *  When an agent posts a `present` intent, this floating panel (top-center,
 *  above the content, no layout shift) shows the message with the speaker's
 *  identity, a timestamp, and a session-lifetime HISTORY of earlier messages
 *  — narrator beats stack up instead of vanishing. Markdown renders in full;
 *  long messages clamp with click-to-expand. Dismissible — you can always
 *  wave the agent off. Part of the agent-in-UI seam; it only shows things,
 *  never mutates the observed sources.
 */

import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import type { HashRoute } from "@/lib/hash-route";
import { isRichMessage } from "@/lib/present-message";
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

/** Session-lifetime message log — survives dismiss/remount (module scope). */
const LOG: HistoryEntry[] = [];
const LOG_MAX = 20;

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
          customStyle={{ margin: "4px 0", padding: "8px 10px", background: "#1a1b26", borderRadius: 6, fontSize: 12 }}
        >
          {raw.replace(/\n$/, "")}
        </SyntaxHighlighter>
      );
    }
    return (
      <code className="rounded bg-[color:var(--bg)] px-1 py-0.5 font-mono text-[12px] text-[color:var(--cyan-bright)]" {...rest}>
        {children}
      </code>
    );
  },
};

function Body({ message, clampable }: { message: string; clampable: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const clamped = clampable && !expanded;
  return (
    <div
      className={cn(
        "prose prose-sm max-w-none break-words text-[13px] leading-snug text-[color:var(--text)] marker:text-[color:var(--text-muted)]",
        clamped && "relative max-h-14 cursor-pointer overflow-hidden",
      )}
      onClick={clamped ? () => setExpanded(true) : undefined}
      title={clamped ? "Click to expand" : undefined}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
        {message}
      </ReactMarkdown>
      {clamped && (
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-[color:var(--bg-surface)] to-transparent" />
      )}
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
  const { issuer, message, sessionIds, route } = present;
  const [showLog, setShowLog] = useState(false);

  // Append each new arrival to the session-lifetime log (dedupe re-renders).
  useEffect(() => {
    const last = LOG[LOG.length - 1];
    if (!last || last.message !== message || last.issuer !== issuer) {
      LOG.push({ ...present, at: new Date().toISOString() });
      if (LOG.length > LOG_MAX) LOG.splice(0, LOG.length - LOG_MAX);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [present]);

  const earlier = LOG.slice(0, -1).reverse();
  const latest = LOG[LOG.length - 1];

  return (
    <div
      className="panel-enter fixed left-1/2 top-14 z-40 w-[min(92vw,44rem)] -translate-x-1/2"
      data-testid="present-banner"
    >
      <div className="shadow-card overflow-hidden rounded-xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)]">
        {/* speaker row */}
        <div className="flex items-center gap-2 border-b border-[color:var(--divider)] bg-[color:var(--accent)]/8 px-3 py-1.5">
          <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[color:var(--accent)] text-[10px] font-bold text-[color:var(--bg-surface)]">
            {issuer.replace(/^Claude\s*/i, "C").slice(0, 1).toUpperCase()}
          </span>
          <span className="truncate text-[length:var(--fs-body)] font-semibold text-[color:var(--text)]">{issuer}</span>
          <span className="shrink-0 text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
            {latest ? compactTime(latest.at) : ""}
          </span>
          <div className="ml-auto flex shrink-0 items-center gap-2">
            {earlier.length > 0 && (
              <button
                onClick={() => setShowLog((v) => !v)}
                className="rounded border border-[color:var(--divider)] px-1.5 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
                aria-expanded={showLog}
              >
                {showLog ? "hide earlier" : `${earlier.length} earlier`}
              </button>
            )}
            {route && (
              <button
                onClick={() => onNavigate(route)}
                className="rounded bg-[color:var(--accent)] px-2 py-0.5 text-[11px] font-medium text-[color:var(--bg-surface)] hover:opacity-90"
              >
                Open →
              </button>
            )}
            <button
              onClick={onDismiss}
              className="rounded px-1.5 text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
              title="Dismiss"
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>
        </div>

        {/* latest message */}
        <div className="px-3 py-2">
          <Body message={message} clampable={isRichMessage(message)} />
          {sessionIds.length > 0 && (
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              {sessionIds.slice(0, 6).map((id) => (
                <button
                  key={id}
                  data-present-session={id}
                  onClick={() => onNavigate({ view: "explore", sessionId: id })}
                  className="rounded border border-[color:var(--divider)] px-1.5 py-0.5 font-mono text-[10px] text-[color:var(--accent)] hover:border-[color:var(--accent)]"
                  title={id}
                >
                  {id.slice(0, 8)}
                </button>
              ))}
              {sessionIds.length > 6 && (
                <span className="text-[10px] text-[color:var(--text-muted)]">+{sessionIds.length - 6}</span>
              )}
            </div>
          )}
        </div>

        {/* earlier messages — the narrator's trail */}
        {showLog && earlier.length > 0 && (
          <div className="max-h-56 space-y-2 overflow-y-auto border-t border-[color:var(--divider)] px-3 py-2">
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
