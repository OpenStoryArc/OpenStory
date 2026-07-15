/** Shared ReactMarkdown renderers for agent-facing surfaces (PresentBanner,
 *  EventSpotlight): fenced/`language-*` code → Prism highlight, inline code →
 *  chip. Extracted from PresentBanner so presentation surfaces share ONE
 *  markdown stack instead of inventing new ones. */

import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";

/** Build the `components` map for ReactMarkdown. `codeFontSize` lets the
 *  spotlight render code larger than the banner without a second stack. */
export function markdownComponents(codeFontSize = 12) {
  return {
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
            customStyle={{ margin: "4px 0", padding: "8px 10px", background: "#1a1b26", borderRadius: 8, fontSize: codeFontSize }}
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
}

/** The banner's renderers — the historical defaults. */
export const MD_COMPONENTS = markdownComponents();
