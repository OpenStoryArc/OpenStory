/**
 * Shared event card rendering — used by both Live Timeline and Explore SessionTimeline.
 *
 * CardBody renders the full content of an event: syntax-highlighted code for tool calls,
 * markdown for prompts/responses, file paths, error messages, etc.
 */

import type { TimelineRow } from "@/lib/timeline";
import type { ViewRecord, ToolCall } from "@/types/view-record";
import { detectLanguage } from "@/lib/detect-language";
import { compactTime } from "@/lib/time";
import { isCatNumbered, stripLineNumbers, extractStartLineNumber } from "@/lib/strip-line-numbers";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// ---------------------------------------------------------------------------
// Color palette (Tokyonight)
// ---------------------------------------------------------------------------

const CATEGORY_COLORS: Record<string, string> = {
  prompt: "#7aa2f7",
  response: "#bb9af7",
  thinking: "#9ece6a",
  tool: "#2ac3de",
  result: "#2ac3de",
  system: "#565f89",
  error: "#f7768e",
  turn: "#3b4261",
};

const CATEGORY_LABELS: Record<string, string> = {
  prompt: "Prompt",
  response: "Response",
  thinking: "Thinking",
  tool: "Tool",
  result: "Result",
  system: "System",
  error: "Error",
  turn: "Turn",
};

// ---------------------------------------------------------------------------
// Code — syntax-highlighted code block
// ---------------------------------------------------------------------------

const codeStyle = { margin: 0, padding: "6px 8px", background: "#1a1b26", fontSize: "12px", borderRadius: "6px" };

function Code({ children, language }: { children: string; language: string }) {
  return (
    <SyntaxHighlighter language={language} style={vscDarkPlus} customStyle={codeStyle} wrapLongLines>
      {children}
    </SyntaxHighlighter>
  );
}

// ---------------------------------------------------------------------------
// FilePath — dir in gray, filename in blue
// ---------------------------------------------------------------------------

function FilePath({ path }: { path: string }) {
  const parts = path.replace(/\\/g, "/").split("/");
  const file = parts.pop()!;
  const dir = parts.join("/");
  return (
    <span className="text-xs font-mono break-all">
      {dir && <span className="text-[#565f89]">{dir}/</span>}
      <span className="text-[#7aa2f7]">{file}</span>
    </span>
  );
}

// ---------------------------------------------------------------------------
// CardBody — the card IS the content
// ---------------------------------------------------------------------------

/** Get full text from a ViewRecord payload, bypassing truncated row.summary.
 *  Handles both flat `text` field and `content: ContentBlock[]` (user/assistant messages). */
function fullText(record: ViewRecord): string | null {
  const payload = record.payload as Record<string, unknown>;
  // Flat text field (thinking, system events)
  const text = payload.text as string | undefined;
  if (text) return text;
  // Content blocks (user_message, assistant_message)
  const content = payload.content as { type: string; text: string }[] | undefined;
  if (Array.isArray(content)) {
    for (const block of content) {
      if (block.type === "text" && block.text) return block.text;
    }
  }
  return null;
}

/** Get full output from a tool_result payload. */
function fullOutput(record: ViewRecord): string | null {
  const payload = record.payload as Record<string, unknown>;
  return (payload.output as string | undefined) ?? null;
}

export function CardBody({ row }: { row: TimelineRow }) {
  const vr = row.record as ViewRecord;

  // ── Tool calls ──
  if (row.category === "tool" && vr.record_type === "tool_call") {
    const tc = vr.payload as ToolCall;
    const ti = tc.typed_input;

    if (ti?.tool === "edit") {
      const edit = ti as import("@/types/view-record").EditInput;
      const lang = detectLanguage({ filePath: edit.file_path });
      return (
        <div className="space-y-1">
          <FilePath path={edit.file_path} />
          {edit.new_string && <Code language={lang}>{edit.new_string}</Code>}
        </div>
      );
    }

    if (ti?.tool === "bash") {
      const bash = ti as import("@/types/view-record").BashInput;
      return (
        <div className="space-y-1">
          {bash.description && <span className="text-xs text-[#565f89] italic">{bash.description}</span>}
          <Code language="bash">{bash.command}</Code>
        </div>
      );
    }

    if (ti?.tool === "write") {
      const fp = (ti as any).file_path as string;
      const lang = detectLanguage({ filePath: fp });
      const content = (ti as any).content as string | undefined;
      return (
        <div className="space-y-1">
          <FilePath path={fp} />
          {content && <Code language={lang}>{content}</Code>}
        </div>
      );
    }

    if (ti?.tool === "read") {
      return <FilePath path={(ti as any).file_path} />;
    }

    if (ti?.tool === "grep") {
      const pattern = (ti as any).pattern as string;
      const path = (ti as any).path as string | undefined;
      return (
        <div className="space-y-0.5">
          <Code language="regex">{pattern}</Code>
          {path && <span className="text-[10px] text-[#565f89] font-mono">in {path}</span>}
        </div>
      );
    }

    if (ti?.tool === "glob") {
      return <Code language="bash">{(ti as any).pattern}</Code>;
    }

    if (ti?.tool === "agent") {
      return (
        <div className="space-y-1">
          {(ti as any).description && <span className="text-xs text-[#bb9af7]">{(ti as any).description}</span>}
          <p className="text-xs text-[#a9b1d6] whitespace-pre-wrap break-words">{(ti as any).prompt}</p>
        </div>
      );
    }

    // Fallback
    return <pre className="text-xs text-[#a9b1d6] whitespace-pre-wrap break-words">{row.summary}</pre>;
  }

  // ── Tool results — detect file content and syntax highlight ──
  if (row.category === "result") {
    const text = fullOutput(vr) ?? row.summary;
    const isError = (vr.payload as Record<string, unknown>).is_error;

    // Detect cat -n formatted file content (from Read tool)
    if (!isError && isCatNumbered(text)) {
      const startLine = extractStartLineNumber(text);
      const cleaned = stripLineNumbers(text);
      const lineCount = cleaned.split("\n").length;
      const endLine = startLine + lineCount - 1;
      const lang = detectLanguage({ filePath: row.fileHint });
      return (
        <div className="space-y-1">
          <span className="text-[10px] text-[#565f89] font-mono">
            Lines {startLine}-{endLine}
          </span>
          <Code language={lang}>{cleaned}</Code>
        </div>
      );
    }

    return (
      <div className="flex items-start gap-1.5">
        <span className={isError ? "text-[#f7768e] shrink-0 mt-0.5" : "text-[#9ece6a] shrink-0 mt-0.5"}>
          {isError ? "\u2717" : "\u2713"}
        </span>
        <pre className="text-xs text-[#a9b1d6] whitespace-pre-wrap break-words min-w-0">{text}</pre>
      </div>
    );
  }

  // ── Errors — prefer full text from payload ──
  if (row.category === "error") {
    const text = fullText(vr) ?? fullOutput(vr) ?? row.summary;
    return (
      <div className="flex items-start gap-1.5">
        <span className="text-[#f7768e] shrink-0 mt-0.5">&#10007;</span>
        <pre className="text-xs text-[#f7768e] whitespace-pre-wrap break-words min-w-0">{text}</pre>
      </div>
    );
  }

  // ── Thinking — prefer full text from payload ──
  if (row.category === "thinking") {
    const text = fullText(vr) ?? row.summary;
    return <p className="text-xs text-[#9ece6a] italic opacity-70 whitespace-pre-wrap break-words">{text}</p>;
  }

  // ── System ──
  if (row.category === "system") {
    const text = fullText(vr) ?? row.summary;
    return <p className="text-xs text-[#565f89] font-mono whitespace-pre-wrap break-words">{text}</p>;
  }

  // ── Prompts + responses: render as markdown — prefer full text from payload ──
  const content = fullText(vr) ?? row.summary;
  const textColor = row.category === "prompt" ? "text-[#c0caf5]" : "text-[#a9b1d6]";
  return (
    <div className={`text-sm ${textColor} leading-relaxed prose prose-invert prose-sm max-w-none break-words [overflow-wrap:anywhere]`}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ className, children, ...props }) {
            const match = /language-(\w+)/.exec(className || "");
            const text = String(children).replace(/\n$/, "");
            if (match) {
              return (
                <SyntaxHighlighter language={match[1]} style={vscDarkPlus} customStyle={codeStyle} wrapLongLines>
                  {text}
                </SyntaxHighlighter>
              );
            }
            return <code className="bg-[#1a1b26] px-1 py-0.5 rounded text-xs break-all" {...props}>{children}</code>;
          },
          pre({ children }) {
            return <>{children}</>;
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

// ---------------------------------------------------------------------------
// EventCardRow — standalone card for Explore (no patterns, no focus, no session avatar)
// ---------------------------------------------------------------------------

interface EventCardRowProps {
  row: TimelineRow;
  /** Compact mode: one-line header only. Full mode: header + CardBody. Default: full. */
  compact?: boolean;
  /** Called when the row is clicked (for expand/collapse). */
  onClick?: () => void;
}

export function EventCardRow({ row, compact = false, onClick }: EventCardRowProps) {
  if (row.category === "turn") {
    return (
      <div className="flex items-center px-4 py-2">
        <div className="flex-1 h-px bg-[#3b4261]" />
        <span className="text-[10px] text-[#565f89] px-3 shrink-0 font-mono">{row.summary}</span>
        <div className="flex-1 h-px bg-[#3b4261]" />
      </div>
    );
  }

  const catColor = CATEGORY_COLORS[row.category] ?? "#565f89";

  return (
    <div
      className={`mx-3 my-1 rounded-xl border border-[#2f3348] overflow-hidden hover:border-[#414868] ${onClick ? "cursor-pointer" : ""}`}
      onClick={onClick}
    >
      <div className={compact ? "px-3 py-1.5" : "px-3 py-2"}>
        {/* Header */}
        <div className="flex items-center gap-1.5">
          <span
            className="text-[10px] px-1.5 py-0.5 rounded font-medium"
            style={{ color: catColor, backgroundColor: `${catColor}18` }}
          >
            {CATEGORY_LABELS[row.category] ?? row.category}
          </span>
          {row.toolName && (
            <span className="text-xs font-semibold text-[#2ac3de]">{row.toolName}</span>
          )}
          {compact && (
            <span className="text-xs text-[#a9b1d6] truncate min-w-0">
              {row.summary.length > 80 ? row.summary.slice(0, 80) + "..." : row.summary}
            </span>
          )}
          <span className="ml-auto text-[10px] text-[#565f89] font-mono shrink-0">
            {compactTime(row.timestamp)}
          </span>
        </div>

        {/* Body — full content (only when not compact) */}
        {!compact && (
          <div className="mt-1">
            <CardBody row={row} />
          </div>
        )}
      </div>
    </div>
  );
}
