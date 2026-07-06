/** Shared rich-markdown renderer — the "Live experience": GFM markdown with
 *  Prism-highlighted fenced code blocks and inline-code chips. One renderer so
 *  the conversation (User/Assistant), the present banner, and any prose surface
 *  format code the same way. */

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";

/** Fenced/`language-*` code → Prism highlight; inline → chip. Block detected by
 *  a language class OR a newline (react-markdown v9 drops the `inline` flag). */
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
      <code className="rounded bg-[#1a1b26] px-1 py-0.5 font-mono text-[12px] text-[#7dcfff]" {...rest}>
        {children}
      </code>
    );
  },
};

/** Render markdown text with syntax-highlighted code. `className` styles the
 *  prose wrapper (spacing/typography via Tailwind `prose`-like utilities). */
export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={className}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
