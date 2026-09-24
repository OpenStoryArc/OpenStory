import { useConnectionStatus } from "@/hooks/use-connection-status";
import { HealthDot } from "@/components/layout/HealthDot";

const STATUS_INDICATOR = {
  connected: { color: "bg-green-400", label: "Connected" },
  connecting: { color: "bg-yellow-400 animate-pulse", label: "Connecting" },
  disconnected: { color: "bg-red-400", label: "Disconnected" },
} as const;

export function Header() {
  const status = useConnectionStatus();
  const { color, label } = STATUS_INDICATOR[status];

  return (
    <header className="flex items-center justify-between px-4 py-2 bg-[color:var(--bg-surface)] border-b border-[color:var(--divider)]">
      <div className="flex items-center gap-3">
        <h1 className="text-lg font-semibold text-[color:var(--text)]">Open Story</h1>
        <span className="text-xs text-[color:var(--text-muted)]">Event Dashboard</span>
      </div>
      <div className="flex items-center gap-3 text-xs text-[color:var(--text-muted)]">
        <HealthDot />
        <span className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${color}`} />
          {label}
        </span>
      </div>
    </header>
  );
}
