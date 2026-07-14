/**
 * Shared event card rendering — used by both Live Timeline and Explore SessionTimeline.
 *
 * CardBody renders the full content of an event: syntax-highlighted code for tool calls,
 * markdown for prompts/responses, file paths, error messages, etc.
 */

import type { TimelineRow } from "@/lib/timeline";
import type { ViewRecord, ToolCall } from "@/types/view-record";
import { detectLanguage } from "@/lib/detect-language";
import { compactTime, fullTimestamp } from "@/lib/time";
import { buildHash } from "@/lib/hash-route";
import { isCatNumbered, stripLineNumbers, extractStartLineNumber } from "@/lib/strip-line-numbers";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { codeTheme, lineNumberStyle } from "@/lib/code-theme";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { isHarnessMessage } from "@/lib/harness-message";
import { HarnessMessageBlock } from "./HarnessMessageBlock";
import { originAgentColor, originAgentLabel } from "@/lib/origin-agent";

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

function Code({
  children,
  language,
  showLineNumbers = false,
  startingLineNumber = 1,
}: {
  children: string;
  language: string;
  showLineNumbers?: boolean;
  startingLineNumber?: number;
}) {
  return (
    <SyntaxHighlighter
      language={language}
      style={codeTheme}
      customStyle={codeStyle}
      wrapLongLines
      showLineNumbers={showLineNumbers}
      startingLineNumber={startingLineNumber}
      lineNumberStyle={lineNumberStyle}
    >
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
      {dir && <span className="text-[color:var(--text-muted)]">{dir}/</span>}
      <span className="text-[color:var(--accent)]">{file}</span>
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

/** Try to extract text from a JSON content-block array (e.g. [{"text":"..."}]).
 *  Returns the extracted text if it matches, or null if it's not that shape. */
function extractContentBlockText(text: string): string | null {
  if (!text.startsWith("[{")) return null;
  try {
    const blocks = JSON.parse(text) as unknown;
    if (!Array.isArray(blocks)) return null;
    const texts: string[] = [];
    for (const block of blocks) {
      if (typeof block === "object" && block !== null && "text" in block && typeof (block as { text: unknown }).text === "string") {
        texts.push((block as { text: string }).text);
      }
    }
    return texts.length > 0 ? texts.join("\n\n") : null;
  } catch {
    return null;
  }
}

/** MCP and other JSON-shaped tool results render unreadably as a single
 *  escaped string. This helper unwraps two common shapes and produces a
 *  pretty-printed JSON string when the input is structured.
 *
 *    1. MCP envelope: `{"result": "<json-or-text>"}` — unwrap to the inner
 *       value, then re-parse as JSON if possible.
 *    2. Bare JSON object/array — pretty-print directly.
 *
 *  Returns `{ pretty, isJson }` where `pretty` is the formatted text and
 *  `isJson` indicates whether to syntax-highlight as JSON. Returns null
 *  when the input isn't structured (caller falls back to plain text). */
function tryFormatStructuredResult(text: string): { pretty: string; isJson: boolean } | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return null;
  }

  // MCP envelope: { "result": "..." } — single `result` key, string value.
  if (
    parsed !== null &&
    typeof parsed === "object" &&
    !Array.isArray(parsed)
  ) {
    const obj = parsed as Record<string, unknown>;
    const keys = Object.keys(obj);
    if (keys.length === 1 && keys[0] === "result" && typeof obj.result === "string") {
      const inner = obj.result;
      try {
        const innerJson = JSON.parse(inner);
        return { pretty: JSON.stringify(innerJson, null, 2), isJson: true };
      } catch {
        // Inner wasn't JSON — return the unwrapped string as plain text.
        return { pretty: inner, isJson: false };
      }
    }
  }

  // Bare JSON object/array — pretty-print.
  return { pretty: JSON.stringify(parsed, null, 2), isJson: true };
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
          {edit.new_string && (
            <Code language={lang} showLineNumbers>
              {edit.new_string}
            </Code>
          )}
        </div>
      );
    }

    if (ti?.tool === "bash") {
      const bash = ti as import("@/types/view-record").BashInput;
      return (
        <div className="space-y-1">
          {bash.description && <span className="text-xs text-[color:var(--text-muted)] italic">{bash.description}</span>}
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
          {content && (
            <Code language={lang} showLineNumbers>
              {content}
            </Code>
          )}
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
          {path && <span className="text-[10px] text-[color:var(--text-muted)] font-mono">in {path}</span>}
        </div>
      );
    }

    if (ti?.tool === "glob") {
      return <Code language="bash">{(ti as any).pattern}</Code>;
    }

    if (ti?.tool === "agent") {
      return (
        <div className="space-y-1">
          {(ti as any).description && <span className="text-xs text-[color:var(--purple)]">{(ti as any).description}</span>}
          <p className="text-xs text-[color:var(--text-bright)] whitespace-pre-wrap break-words">{(ti as any).prompt}</p>
        </div>
      );
    }

    // Fallback
    return <pre className="text-xs text-[color:var(--text-bright)] whitespace-pre-wrap break-words">{row.summary}</pre>;
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
          <span className="text-[10px] text-[color:var(--text-muted)] font-mono">
            Lines {startLine}-{endLine}
          </span>
          <Code language={lang} showLineNumbers startingLineNumber={startLine}>
            {cleaned}
          </Code>
        </div>
      );
    }

    // Detect JSON content-block arrays and render as markdown
    const extracted = extractContentBlockText(text);
    if (extracted && !isError) {
      return (
        <div className="flex items-start gap-1.5">
          <span className="text-[color:var(--green)] shrink-0 mt-0.5">{"\u2713"}</span>
          <div className="text-sm text-[color:var(--text-bright)] leading-relaxed prose prose-invert prose-sm max-w-none break-words [overflow-wrap:anywhere] min-w-0">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || "");
                  const codeText = String(children).replace(/\n$/, "");
                  if (match) {
                    return (
                      <SyntaxHighlighter language={match[1]} style={codeTheme} customStyle={codeStyle} wrapLongLines>
                        {codeText}
                      </SyntaxHighlighter>
                    );
                  }
                  return <code className="bg-[color:var(--bg)] px-1 py-0.5 rounded text-xs break-all" {...props}>{children}</code>;
                },
                pre({ children }) {
                  return <>{children}</>;
                },
              }}
            >
              {extracted}
            </ReactMarkdown>
          </div>
        </div>
      );
    }

    // Detect MCP-style and bare JSON results \u2014 pretty-print and (where
    // appropriate) syntax-highlight. Without this, MCP wraps everything
    // in `{"result":"<escaped JSON>"}`, which renders as one unreadable
    // line of \n and \" escapes.
    const structured = tryFormatStructuredResult(text);
    if (structured && !isError) {
      return (
        <div className="flex items-start gap-1.5">
          <span className="text-[color:var(--green)] shrink-0 mt-0.5">{"\u2713"}</span>
          {structured.isJson ? (
            <div className="min-w-0 flex-1 overflow-x-auto">
              <Code language="json">{structured.pretty}</Code>
            </div>
          ) : (
            <pre className="text-xs text-[color:var(--text-bright)] whitespace-pre-wrap break-words min-w-0">
              {structured.pretty}
            </pre>
          )}
        </div>
      );
    }

    return (
      <div className="flex items-start gap-1.5">
        <span className={isError ? "text-[color:var(--red)] shrink-0 mt-0.5" : "text-[color:var(--green)] shrink-0 mt-0.5"}>
          {isError ? "\u2717" : "\u2713"}
        </span>
        <pre className="text-xs text-[color:var(--text-bright)] whitespace-pre-wrap break-words min-w-0">{text}</pre>
      </div>
    );
  }

  // ── Errors — prefer full text from payload ──
  if (row.category === "error") {
    const text = fullText(vr) ?? fullOutput(vr) ?? row.summary;
    return (
      <div className="flex items-start gap-1.5">
        <span className="text-[color:var(--red)] shrink-0 mt-0.5">&#10007;</span>
        <pre className="text-xs text-[color:var(--red)] whitespace-pre-wrap break-words min-w-0">{text}</pre>
      </div>
    );
  }

  // ── Thinking — prefer full text from payload ──
  if (row.category === "thinking") {
    const text = fullText(vr) ?? row.summary;
    return <p className="text-xs text-[color:var(--green)] italic opacity-70 whitespace-pre-wrap break-words">{text}</p>;
  }

  // ── System ──
  if (row.category === "system") {
    const text = fullText(vr) ?? row.summary;
    return <p className="text-xs text-[color:var(--text-muted)] font-mono whitespace-pre-wrap break-words">{text}</p>;
  }

  // ── Prompts + responses: render as markdown — prefer full text from payload ──
  const content = fullText(vr) ?? row.summary;

  // Harness-wrapper prompts (slash commands, task notifications, system
  // reminders) render as a clean structured block, full content preserved.
  if (row.category === "prompt" && isHarnessMessage(content)) {
    return <HarnessMessageBlock text={content} />;
  }

  const textColor = row.category === "prompt" ? "text-[color:var(--text)]" : "text-[color:var(--text-bright)]";
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
                <SyntaxHighlighter language={match[1]} style={codeTheme} customStyle={codeStyle} wrapLongLines>
                  {text}
                </SyntaxHighlighter>
              );
            }
            return <code className="bg-[color:var(--bg)] px-1 py-0.5 rounded text-xs break-all" {...props}>{children}</code>;
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
  /** Keyboard navigation selection indicator. */
  selected?: boolean;
  /** Called when the row is clicked (for expand/collapse). */
  onClick?: () => void;
  /** Event id of this record's tool round-trip partner (call↔result). */
  pairedEventId?: string;
}

export function EventCardRow({ row, compact = false, selected = false, onClick, pairedEventId }: EventCardRowProps) {
  if (row.category === "turn") {
    return (
      <div className="flex items-center px-4 py-2">
        <div className="flex-1 h-px bg-[color:var(--border)]" />
        <span className="text-[10px] text-[color:var(--text-muted)] px-3 shrink-0 font-mono">{row.summary}</span>
        <div className="flex-1 h-px bg-[color:var(--border)]" />
      </div>
    );
  }

  const catColor = CATEGORY_COLORS[row.category] ?? "#565f89";
  const agent = (row.record as ViewRecord).origin_agent;
  const agentLabel = originAgentLabel(agent);
  const agentColor = originAgentColor(agent);

  return (
    <div
      className={`mx-3 my-1 rounded-xl border border-[color:var(--bg-hover)] overflow-hidden hover:border-[color:var(--border)] ${onClick ? "cursor-pointer" : ""}${selected ? " ring-1 ring-[color:var(--accent)]" : ""}`}
      onClick={onClick}
    >
      <div className={compact ? "px-3 py-1.5" : "px-3 py-2"}>
        {/* Header */}
        <div className="flex items-center gap-1.5">
          {agentLabel && (
            <span
              className="text-[10px] px-1.5 py-0.5 rounded font-medium"
              style={{ color: agentColor, backgroundColor: `${agentColor}18` }}
              title={`Agent: ${agentLabel}`}
              data-testid="event-card-agent-badge"
            >
              {agentLabel}
            </span>
          )}
          <span
            className="text-[10px] px-1.5 py-0.5 rounded font-medium"
            style={{ color: catColor, backgroundColor: `${catColor}18` }}
          >
            {CATEGORY_LABELS[row.category] ?? row.category}
          </span>
          {row.toolName && (
            <span className="text-xs font-semibold text-[color:var(--cyan)]">{row.toolName}</span>
          )}
          {compact && (
            <span className="text-xs text-[color:var(--text-bright)] truncate min-w-0">
              {row.summary.length > 80 ? row.summary.slice(0, 80) + "..." : row.summary}
            </span>
          )}
          <span
            className="ml-auto text-[10px] text-[color:var(--text-muted)] font-mono shrink-0"
            data-testid="event-time"
            title={fullTimestamp(row.timestamp)}
          >
            {compactTime(row.timestamp)}
          </span>
        </div>

        {/* Body — full content (only when not compact) */}
        {!compact && (
          <div className="mt-1">
            <CardBody row={row} />
            {/* The event→turn edge: climb from this event to its turn in
                Story. Detail-on-click — only the expanded card offers it. */}
            <a
              href={buildHash({
                view: "story",
                sessionId: (row.record as ViewRecord).session_id,
                eventId: (row.record as ViewRecord).id,
              })}
              data-testid="event-story-turn-link"
              onClick={(e) => e.stopPropagation()}
              className="mt-1 inline-block text-[10px] text-[color:var(--purple)] hover:underline"
              title="Open this event's turn in Story"
            >
              ↑ Turn in Story
            </a>
            {/* toolcall↔result: jump across the round trip. */}
            {pairedEventId && (
              <a
                href={buildHash({
                  view: "explore",
                  sessionId: (row.record as ViewRecord).session_id,
                  eventId: pairedEventId,
                })}
                data-testid="event-pair-link"
                onClick={(e) => e.stopPropagation()}
                className="ml-3 mt-1 inline-block text-[10px] text-[color:var(--cyan-bright)] hover:underline"
                title="Jump across the tool round trip"
              >
                ⇄ {row.record.record_type === "tool_call" ? "result" : "call"}
              </a>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
