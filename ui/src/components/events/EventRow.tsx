import { memo } from "react";
import type { ViewRecord, ToolCall } from "@/types/view-record";
import { viewRecordLabel, viewRecordSummary } from "@/lib/view-record-transforms";
import { viewRecordColor, isGitBashRecord } from "@/lib/view-record-transforms";
import { compactTime, fullTimestamp } from "@/lib/time";
import { gitCommandRisk, GIT_RISK_COLORS } from "@/lib/git-commands";
import { originAgentColor, originAgentLabel } from "@/lib/origin-agent";

interface EventRowProps {
  record: ViewRecord;
  selected: boolean;
  onClick: (id: string) => void;
  isNew?: boolean;
}

export const EventRow = memo(
  function EventRow({ record, selected, onClick, isNew }: EventRowProps) {
    const color = viewRecordColor(record);
    const label = viewRecordLabel(record.record_type);
    const agentLabel = originAgentLabel(record.origin_agent);
    const agentColor = originAgentColor(record.origin_agent);

    // Tool name as subtype for tool_call records
    const subtype = record.record_type === "tool_call"
      ? (record.payload as ToolCall).name
      : undefined;

    // Git command risk styling
    const gitBash = isGitBashRecord(record);
    const gitRisk = gitBash
      ? gitCommandRisk(extractBashCommand(record))
      : null;
    const gitBorderStyle = gitRisk
      ? { borderLeftWidth: "3px", borderLeftColor: GIT_RISK_COLORS[gitRisk] }
      : {};
    const destructiveBg = gitRisk === "destructive" ? " bg-[color:var(--red)]/10" : "";

    return (
      <button
        onClick={() => onClick(record.id)}
        className={`w-full text-left flex items-center gap-2 px-3 py-1.5 text-xs font-mono border-b border-[color:var(--divider)] transition-colors ${
          selected ? "bg-[color:var(--bg-hover)]" : "hover:bg-[color:var(--bg-surface)]"
        } ${isNew ? "event-new" : ""}${destructiveBg}`}
        style={gitBorderStyle}
      >
        <span className="w-16 text-[color:var(--text-muted)] flex-shrink-0">
          <span title={fullTimestamp(record.timestamp)}>{compactTime(record.timestamp)}</span>
        </span>
        {agentLabel && (
          <span
            className="w-14 flex-shrink-0 px-1.5 py-0.5 rounded text-center whitespace-nowrap"
            style={{ color: agentColor, backgroundColor: `${agentColor}20` }}
            title={`Agent: ${agentLabel}`}
            data-testid="event-agent-badge"
          >
            {agentLabel}
          </span>
        )}
        <span
          className="w-20 flex-shrink-0 px-1.5 py-0.5 rounded text-center whitespace-nowrap"
          style={{ color, backgroundColor: `${color}20` }}
        >
          {label}
        </span>
        {subtype && (
          <span className="w-24 flex-shrink-0 text-[color:var(--cyan)] truncate">
            {subtype}
          </span>
        )}
        <span className="flex-1 text-[color:var(--text)] truncate">
          {viewRecordSummary(record)}
        </span>
      </button>
    );
  },
  (prev, next) => prev.record.id === next.record.id && prev.selected === next.selected,
);

/** Extract bash command from a ViewRecord (for git risk detection) */
function extractBashCommand(record: ViewRecord): string {
  if (record.record_type !== "tool_call") return "";
  const tc = record.payload as ToolCall;
  if (tc.typed_input?.tool === "bash") {
    return (tc.typed_input as { command?: string }).command ?? "";
  }
  return "";
}
