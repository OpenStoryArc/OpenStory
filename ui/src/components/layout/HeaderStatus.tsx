/**
 * HeaderStatus — the right side of the app header: theme, text size, the
 * jump-to button, the driven-by badge, the node's health dot (H-08), and
 * the WebSocket connection light. This is the header the app renders.
 */

import { ThemeToggle } from "@/components/layout/ThemeToggle";
import { TextSizeControl } from "@/components/layout/TextSizeControl";
import { HealthDot } from "@/components/layout/HealthDot";

export type ConnectionStatus = "connected" | "connecting" | "disconnected";

const STATUS_INDICATOR: Record<ConnectionStatus, { color: string; label: string }> = {
  connected: { color: "bg-green-400", label: "Connected" },
  connecting: { color: "bg-yellow-400 animate-pulse", label: "Connecting" },
  disconnected: { color: "bg-red-400", label: "Disconnected" },
};

interface Props {
  readonly status: ConnectionStatus;
  /** The agent driving the view, or null when the human has the wheel. */
  readonly drivenBy: string | null;
}

export function HeaderStatus({ status, drivenBy }: Props) {
  const { color, label } = STATUS_INDICATOR[status];
  return (
    <div className="flex shrink-0 items-center gap-3">
      <ThemeToggle />
      <TextSizeControl />
      <button
        onClick={() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }))}
        className="flex items-center gap-1.5 rounded border border-[color:var(--border)] px-2 py-1 text-[11px] text-[color:var(--text-muted)] hover:border-[color:var(--accent)] hover:text-[color:var(--text)] transition-colors"
        title="Command palette"
      >
        <span>Jump to…</span>
        <kbd className="rounded bg-[color:var(--bg)] px-1 text-[10px]">⌘K</kbd>
      </button>
      {drivenBy && (
        <div
          className="flex items-center gap-1.5 rounded border border-[color:var(--accent)]/50 bg-[color:var(--accent)]/10 px-2 py-1 text-[11px] text-[color:var(--accent)] animate-pulse"
          data-testid="driven-by"
          title="An agent is driving this view. Click anywhere or navigate to take back the wheel."
        >
          <span>▸</span> driven by {drivenBy}
        </div>
      )}
      <HealthDot />
      <div className="flex items-center gap-2 text-xs text-[color:var(--text-muted)]" data-testid="connection-status">
        <span className={`w-2 h-2 rounded-full ${color}`} />
        {label}
      </div>
    </div>
  );
}
