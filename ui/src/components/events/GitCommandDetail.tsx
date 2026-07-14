import type { ViewRecord, ToolCall } from "@/types/view-record";
import { gitCommandRisk, gitRiskLabel, gitCommandSummary, GIT_RISK_COLORS, parseGitSubcommand } from "@/lib/git-commands";

interface GitCommandDetailProps {
  record: ViewRecord;
  onClose: () => void;
}

/** Warning callouts for specific git patterns */
function getWarning(command: string): string | null {
  if (/push\s/.test(command) && (/--force\b/.test(command) || /\s-\w*f/.test(command))) {
    return "Force push overwrites remote history";
  }
  if (/reset\s/.test(command) && /--hard\b/.test(command)) {
    return "Discards all uncommitted changes";
  }
  if (/clean\s/.test(command) && /\s-\w*f/.test(command)) {
    return "Permanently deletes untracked files";
  }
  if (/branch\s/.test(command) && /\s-D\b/.test(command)) {
    return "Deletes branch even if not fully merged";
  }
  if (/checkout\s/.test(command) && /--\s+\./.test(command)) {
    return "Discards all unstaged changes";
  }
  if (/restore\s/.test(command) && /\s\.\s*$/.test(command)) {
    return "Discards all working tree changes";
  }
  if (/commit\s/.test(command) && /--amend\b/.test(command)) {
    return "Rewrites the previous commit";
  }
  if (parseGitSubcommand(command) === "rebase") {
    return "Rewrites commit history";
  }
  return null;
}

export function GitCommandDetail({ record, onClose }: GitCommandDetailProps) {
  const tc = record.payload as ToolCall;
  const command = tc.typed_input?.tool === "bash"
    ? (tc.typed_input as { command?: string }).command ?? ""
    : "";
  const risk = gitCommandRisk(command);
  const riskColor = GIT_RISK_COLORS[risk];
  const label = gitRiskLabel(risk);
  const summary = gitCommandSummary(command);
  const warning = getWarning(command);

  return (
    <div className="h-full bg-[color:var(--bg-surface)] p-4 overflow-y-auto">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-[color:var(--text)]">
          Git Command
        </h3>
        <button
          onClick={onClose}
          className="text-xs text-[color:var(--text-muted)] hover:text-[color:var(--text)] px-2 py-1"
        >
          Close
        </button>
      </div>

      {/* Risk badge */}
      <div className="flex items-center gap-2 mb-3">
        <span
          className="inline-block w-2.5 h-2.5 rounded-full"
          style={{ backgroundColor: riskColor }}
        />
        <span className="text-xs font-medium" style={{ color: riskColor }}>
          {label}
        </span>
        <span className="text-xs text-[color:var(--text-muted)] ml-2">
          {summary}
        </span>
      </div>

      {/* Full command */}
      <pre className="bg-[color:var(--bg)] rounded p-3 text-xs font-mono text-[color:var(--text)] whitespace-pre-wrap break-words mb-3">
        {command}
      </pre>

      {/* Warning callout */}
      {warning && (
        <div
          data-testid="git-warning"
          className="rounded p-3 mb-3 text-xs border"
          style={{
            backgroundColor: `${riskColor}15`,
            borderColor: `${riskColor}40`,
            color: riskColor,
          }}
        >
          {warning}
        </div>
      )}

      {/* Record data */}
      <details className="text-xs">
        <summary className="text-[color:var(--text-muted)] cursor-pointer mb-2">
          Record Data
        </summary>
        <pre className="bg-[color:var(--bg)] rounded p-3 overflow-x-auto text-[color:var(--text)] whitespace-pre-wrap break-words">
          {JSON.stringify(record.payload, null, 2)}
        </pre>
      </details>
    </div>
  );
}
