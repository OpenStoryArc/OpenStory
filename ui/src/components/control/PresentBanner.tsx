/** PresentBanner — the "present" control class made visible: when an agent
 *  posts a `present`/`highlight`/`announce` intent, this strip shows its message
 *  and spotlights the sessions it pointed at, each click-through to Explore, with
 *  an optional jump. Dismissible — you can always wave the agent off. Part of the
 *  agent-in-UI seam; it only shows things, never mutates the observed sources.
 *
 *  The message renders as full markdown (with syntax-highlighted fenced code) and
 *  NEVER hard-truncates: a long/multi-line/code message collapses to a few lines
 *  with a fade, and the body is click-to-expand — you can always reach all of it. */

import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import type { HashRoute } from "@/lib/hash-route";
import { isRichMessage } from "@/lib/present-message";
import { cn } from "@/lib/cn";

export interface Presentation {
  readonly issuer: string;
  readonly message: string;
  readonly sessionIds: readonly string[];
  readonly route: HashRoute | null;
}

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
  const rich = isRichMessage(message);
  const [expanded, setExpanded] = useState(false);
  const clamped = rich && !expanded;

  return (
    <div
      className="flex flex-col gap-1 border-b border-[color:var(--accent)]/40 bg-[color:var(--accent)]/10 px-4 py-2 text-[13px] text-[color:var(--text)]"
      data-testid="present-banner"
    >
      {/* header row: who + spotlight chips + jump + expand + dismiss */}
      <div className="flex items-center gap-3">
        <span className="shrink-0 rounded bg-[color:var(--accent)] px-1.5 py-0.5 text-[10px] font-semibold text-[color:var(--bg)]">
          ▸ {issuer}
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {sessionIds.length > 0 && (
            <div className="flex items-center gap-1.5">
              {sessionIds.slice(0, 6).map((id) => (
                <button
                  key={id}
                  data-present-session={id}
                  onClick={() => onNavigate({ view: "explore", sessionId: id })}
                  className="rounded border border-[color:var(--border)] px-1.5 py-0.5 font-mono text-[10px] text-[color:var(--accent)] hover:border-[color:var(--accent)] hover:bg-[color:var(--bg-surface)]"
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
          {route && (
            <button
              onClick={() => onNavigate(route)}
              className="rounded bg-[color:var(--accent)] px-2 py-0.5 text-[11px] font-medium text-[color:var(--bg)] hover:bg-[#9db8fa]"
            >
              Open →
            </button>
          )}
          {rich && (
            <button
              data-testid="present-expand"
              onClick={() => setExpanded((v) => !v)}
              className="rounded border border-[color:var(--border)] px-1.5 py-0.5 text-[10px] text-[color:var(--text-bright)] hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
              aria-expanded={expanded}
            >
              {expanded ? "▲ less" : "▼ more"}
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

      {/* message body: full markdown, wraps, and click-to-expand when clamped
          (never a dead-end clip — the whole thing is always reachable). */}
      {message && (
        <div
          className={cn(
            "prose prose-invert prose-sm max-w-none break-words text-[13px] leading-snug marker:text-[color:var(--text-muted)]",
            clamped && "relative max-h-12 cursor-pointer overflow-hidden",
          )}
          onClick={clamped ? () => setExpanded(true) : undefined}
          title={clamped ? "Click to expand" : undefined}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
            {message}
          </ReactMarkdown>
          {clamped && (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-[#191c2b] to-transparent" />
          )}
        </div>
      )}
    </div>
  );
}
