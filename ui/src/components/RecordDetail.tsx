/**
 * RecordDetail — rich rendering for expanded timeline rows.
 *
 * Each record type gets a tailored view instead of raw JSON.
 */

import { memo, useState, useCallback } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { detectLanguage } from "@/lib/detect-language";
import { stripAnsi } from "@/lib/strip-ansi";
import { formatBytes, truncationLabel, contentApiUrl, copyToClipboard } from "@/lib/truncation";
import type {
  ViewRecord,
  ToolCall,
  ToolResult,
  UserMessage,
  AssistantMessage,
  Reasoning,
  SystemEvent,
  ErrorRecord,
  TurnEnd,
  ContentBlock,
  BashInput,
  EditInput,
} from "@/types/view-record";
import type { WireRecord } from "@/types/wire-record";

// ---------------------------------------------------------------------------
// Shared primitives
// ---------------------------------------------------------------------------

function Label({ children }: { children: React.ReactNode }) {
  return <span className="text-[#565f89] text-[10px] uppercase tracking-wider">{children}</span>;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(async () => {
    const ok = await copyToClipboard(text);
    if (ok) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  }, [text]);

  return (
    <button
      onClick={handleCopy}
      className="text-[10px] text-[#565f89] hover:text-[#c0caf5] transition-colors px-1.5"
      title="Copy to clipboard"
      data-testid="copy-button"
    >
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

function CodeBlock({
  children,
  lang,
  filePath,
  toolName,
  maxHeight,
}: {
  children: string;
  lang?: string;
  filePath?: string;
  toolName?: string;
  maxHeight?: string;
}) {
  const language = detectLanguage({ lang, filePath, toolName });
  const displayLabel = lang || (language !== "text" ? language : undefined);

  return (
    <div className="mt-1 rounded bg-[#1a1b26] border border-[#2f3348] overflow-auto" style={{ maxHeight: maxHeight ?? "200px" }}>
      <div className="flex items-center justify-between border-b border-[#2f3348]" style={{ minHeight: displayLabel ? undefined : 0 }}>
        {displayLabel && (
          <div className="px-2 py-0.5 text-[10px] text-[#565f89]">{displayLabel}</div>
        )}
        <CopyButton text={children} />
      </div>
      <SyntaxHighlighter
        language={language}
        style={vscDarkPlus}
        customStyle={{
          margin: 0,
          padding: "6px 8px",
          background: "transparent",
          fontSize: "12px",
        }}
        wrapLongLines={true}
      >
        {children}
      </SyntaxHighlighter>
    </div>
  );
}

function FilePath({ path }: { path: string }) {
  const parts = path.replace(/\\/g, "/").split("/");
  const file = parts.pop()!;
  const dir = parts.join("/");
  return (
    <span className="font-mono text-xs">
      {dir && <span className="text-[#565f89]">{dir}/</span>}
      <span className="text-[#7aa2f7]">{file}</span>
    </span>
  );
}

// ---------------------------------------------------------------------------
// Tool Call detail
// ---------------------------------------------------------------------------

function ToolCallDetail({ payload }: { payload: ToolCall }) {
  const ti = payload.typed_input;

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <span className="text-sm font-semibold text-[#2ac3de]">{payload.name}</span>
        <span className="text-[10px] text-[#565f89]">{payload.call_id.slice(0, 12)}</span>
      </div>

      {ti?.tool === "bash" && (
        <div>
          <Label>Command</Label>
          {(ti as BashInput).description && (
            <div className="text-xs text-[#565f89] italic mb-1">{(ti as BashInput).description}</div>
          )}
          <CodeBlock lang="bash">{(ti as BashInput).command}</CodeBlock>
        </div>
      )}

      {(ti?.tool === "read" || ti?.tool === "write") && (
        <div>
          <Label>File</Label>
          <div className="mt-0.5"><FilePath path={(ti as any).file_path} /></div>
        </div>
      )}

      {ti?.tool === "edit" && (
        <div className="space-y-1">
          <Label>File</Label>
          <div className="mt-0.5"><FilePath path={(ti as EditInput).file_path} /></div>
          {(ti as EditInput).old_string && (
            <>
              <Label>Replace</Label>
              <CodeBlock filePath={(ti as EditInput).file_path}>{(ti as EditInput).old_string}</CodeBlock>
              <Label>With</Label>
              <CodeBlock filePath={(ti as EditInput).file_path}>{(ti as EditInput).new_string}</CodeBlock>
            </>
          )}
        </div>
      )}

      {ti?.tool === "grep" && (
        <div>
          <Label>Pattern</Label>
          <CodeBlock>{(ti as any).pattern}</CodeBlock>
          {(ti as any).path && (
            <div className="mt-1"><Label>In</Label> <FilePath path={(ti as any).path} /></div>
          )}
        </div>
      )}

      {ti?.tool === "glob" && (
        <div>
          <Label>Pattern</Label>
          <CodeBlock>{(ti as any).pattern}</CodeBlock>
        </div>
      )}

      {ti?.tool === "agent" && (
        <div>
          {(ti as any).description && (
            <div className="text-xs text-[#c0caf5] mb-1">{(ti as any).description}</div>
          )}
          <Label>Prompt</Label>
          <div className="mt-0.5 text-xs text-[#a9b1d6] max-h-[150px] overflow-auto whitespace-pre-wrap">
            {(ti as any).prompt}
          </div>
        </div>
      )}

      {ti?.tool === "web_search" && (
        <div><Label>Query</Label><div className="text-xs text-[#c0caf5] mt-0.5">{(ti as any).query}</div></div>
      )}

      {ti?.tool === "web_fetch" && (
        <div><Label>URL</Label><div className="text-xs text-[#7aa2f7] mt-0.5">{(ti as any).url}</div></div>
      )}

      {/* Fallback for unknown tools */}
      {!ti && (
        <CodeBlock lang="json">{JSON.stringify(payload.input, null, 2)}</CodeBlock>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tool Result detail
// ---------------------------------------------------------------------------

const DISPLAY_LIMIT = 2000;

function ToolResultDetail({ payload, sessionId, eventId, isTruncated }: {
  payload: ToolResult;
  sessionId: string;
  eventId: string;
  isTruncated: boolean;
}) {
  const [fullContent, setFullContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isOutputLong = (payload.output?.length ?? 0) > DISPLAY_LIMIT;
  const showViewFull = isTruncated || isOutputLong;

  const handleViewFull = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(contentApiUrl(sessionId, eventId));
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      setFullContent(text);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, [sessionId, eventId]);

  const displayOutput = fullContent
    ? stripAnsi(fullContent)
    : payload.output
      ? stripAnsi(isOutputLong ? payload.output.slice(0, DISPLAY_LIMIT) + "\n... (truncated)" : payload.output)
      : null;

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        {payload.is_error && <span className="text-[10px] px-1.5 py-0.5 rounded bg-[#f7768e20] text-[#f7768e]">Error</span>}
        <span className="text-[10px] text-[#565f89]">{payload.call_id.slice(0, 12)}</span>
      </div>
      {displayOutput && (
        <CodeBlock maxHeight={fullContent ? "500px" : "200px"}>{displayOutput}</CodeBlock>
      )}
      {showViewFull && !fullContent && (
        <button
          onClick={handleViewFull}
          disabled={loading}
          className="text-[10px] text-[#7aa2f7] hover:text-[#89b4fa] transition-colors"
          data-testid="view-full-button"
        >
          {loading ? "Loading..." : "View full output"}
        </button>
      )}
      {error && (
        <div className="text-[10px] text-[#f7768e]">Failed to load full content: {error}</div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Message details
// ---------------------------------------------------------------------------

function renderContentBlocks(blocks: ContentBlock[], useMarkdown = false) {
  return blocks.map((block, i) => {
    if (block.type === "text" && block.text) {
      if (useMarkdown) {
        return (
          <div key={i} className="text-xs text-[#c0caf5] prose prose-invert prose-xs max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{block.text}</ReactMarkdown>
          </div>
        );
      }
      return <div key={i} className="text-xs text-[#c0caf5] whitespace-pre-wrap">{block.text}</div>;
    }
    if (block.type === "code_block" && block.text) {
      return <CodeBlock key={i} lang={block.language}>{block.text}</CodeBlock>;
    }
    return null;
  });
}

function UserMessageDetail({ payload }: { payload: UserMessage }) {
  if (typeof payload.content === "string") {
    return <div className="text-xs text-[#c0caf5] whitespace-pre-wrap">{payload.content}</div>;
  }
  return <div className="space-y-2">{renderContentBlocks(payload.content)}</div>;
}

function AssistantMessageDetail({ payload }: { payload: AssistantMessage }) {
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <span className="text-[10px] text-[#565f89]">{payload.model}</span>
        {payload.stop_reason && <span className="text-[10px] text-[#565f89]">stop: {payload.stop_reason}</span>}
      </div>
      {renderContentBlocks(payload.content, true)}
    </div>
  );
}

function ReasoningDetail({ payload }: { payload: Reasoning }) {
  return (
    <div className="space-y-1">
      {payload.encrypted && <span className="text-[10px] text-[#565f89]">encrypted</span>}
      {payload.summary.map((s, i) => (
        <div key={i} className="text-xs text-[#9ece6a] italic">{s}</div>
      ))}
      {payload.content && (
        <div className="text-xs text-[#a9b1d6] whitespace-pre-wrap mt-1">{payload.content}</div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// System / Error / TurnEnd
// ---------------------------------------------------------------------------

function SystemEventDetail({ payload }: { payload: SystemEvent }) {
  return (
    <div className="space-y-1">
      <span className="text-xs text-[#565f89] font-mono">{payload.subtype}</span>
      {payload.message && <div className="text-xs text-[#a9b1d6]">{payload.message}</div>}
      {payload.duration_ms != null && (
        <div className="text-xs text-[#565f89]">{(payload.duration_ms / 1000).toFixed(1)}s</div>
      )}
    </div>
  );
}

function ErrorDetail({ payload }: { payload: ErrorRecord }) {
  return (
    <div className="space-y-1">
      <div className="text-xs font-mono text-[#f7768e]">{payload.code}</div>
      <div className="text-xs text-[#c0caf5]">{payload.message}</div>
      {payload.details && <CodeBlock>{payload.details}</CodeBlock>}
    </div>
  );
}

function TurnEndDetail({ payload }: { payload: TurnEnd }) {
  return (
    <div className="text-xs text-[#565f89]">
      {payload.reason && <span>Reason: {payload.reason}</span>}
      {payload.duration_ms != null && <span className="ml-2">{(payload.duration_ms / 1000).toFixed(1)}s</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main dispatcher
// ---------------------------------------------------------------------------

interface RecordDetailProps {
  record: ViewRecord | WireRecord;
  onFocusSubtree?: (id: string) => void;
  isFocusRoot?: boolean;
}

export const RecordDetail = memo(function RecordDetail({ record, onFocusSubtree, isFocusRoot }: RecordDetailProps) {
  const p = record.payload;

  // WireRecord metadata (extracted early so it's available in sub-components)
  const isWire = "depth" in record;
  const depth = isWire ? (record as WireRecord).depth : 0;
  const truncated = isWire ? (record as WireRecord).truncated : false;
  const payloadBytes = isWire ? (record as WireRecord).payload_bytes : 0;

  let detail: React.ReactNode;
  switch (record.record_type) {
    case "tool_call":
      detail = <ToolCallDetail payload={p as ToolCall} />;
      break;
    case "tool_result":
      detail = (
        <ToolResultDetail
          payload={p as ToolResult}
          sessionId={record.session_id}
          eventId={record.id}
          isTruncated={truncated}
        />
      );
      break;
    case "user_message":
      detail = <UserMessageDetail payload={p as UserMessage} />;
      break;
    case "assistant_message":
      detail = <AssistantMessageDetail payload={p as AssistantMessage} />;
      break;
    case "reasoning":
      detail = <ReasoningDetail payload={p as Reasoning} />;
      break;
    case "system_event":
      detail = <SystemEventDetail payload={p as SystemEvent} />;
      break;
    case "error":
      detail = <ErrorDetail payload={p as ErrorRecord} />;
      break;
    case "turn_end":
      detail = <TurnEndDetail payload={p as TurnEnd} />;
      break;
    default:
      detail = (
        <pre className="text-xs text-[#a9b1d6] whitespace-pre-wrap overflow-auto max-h-[300px]">
          {JSON.stringify(p, null, 2)}
        </pre>
      );
  }

  return (
    <div>
      {detail}

      {/* Metadata bar: depth badge + truncation indicator + subtree focus */}
      {(onFocusSubtree || depth > 0 || truncated) && (
        <div className="mt-2 pt-2 border-t border-[#2f3348] flex items-center gap-2 flex-wrap">
          {depth > 0 && (
            <span
              className="text-[10px] px-1.5 py-0.5 rounded bg-[#2ac3de20] text-[#2ac3de]"
              data-testid="detail-depth"
              title={`Nesting level in agent delegation chain (0 = top-level, ${depth} = ${depth} level${depth !== 1 ? "s" : ""} deep)`}
            >
              depth {depth}
            </span>
          )}
          {truncated && (
            <span
              className="text-[10px] px-1.5 py-0.5 rounded bg-[#e0af6820] text-[#e0af68]"
              data-testid="detail-truncated"
              title={truncationLabel(payloadBytes, DISPLAY_LIMIT)}
            >
              {formatBytes(Math.max(0, payloadBytes - DISPLAY_LIMIT))} hidden
            </span>
          )}
          {onFocusSubtree && (
            <button
              data-testid="detail-focus-subtree"
              onClick={() => onFocusSubtree(record.id)}
              title={isFocusRoot
                ? "Click to exit focus and show all events"
                : "Show only this event and its descendants in the timeline"}
              className={`text-[10px] px-2 py-0.5 rounded transition-colors ${
                isFocusRoot
                  ? "bg-[#e0af6830] text-[#e0af68]"
                  : "text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b]"
              }`}
            >
              {isFocusRoot ? "Focused — click to exit" : "Focus subtree"}
            </button>
          )}
        </div>
      )}
    </div>
  );
});
