import { useConnectionStatus } from "@/hooks/use-connection-status";

const STATUS_INDICATOR = {
  connected: { color: "bg-green-400", label: "Connected" },
  connecting: { color: "bg-yellow-400 animate-pulse", label: "Connecting" },
  disconnected: { color: "bg-red-400", label: "Disconnected" },
} as const;

export function Header() {
  const status = useConnectionStatus();
  const { color, label } = STATUS_INDICATOR[status];

  return (
    <header className="flex items-center justify-between px-4 py-2 bg-[#24283b] border-b border-[#2f3348]">
      <div className="flex items-center gap-3">
        <h1 className="text-lg font-semibold text-[#c0caf5]">Open Story</h1>
        <span className="text-xs text-[#565f89]">Event Dashboard</span>
      </div>
      <div className="flex items-center gap-2 text-xs text-[#565f89]">
        <span className={`w-2 h-2 rounded-full ${color}`} />
        {label}
      </div>
    </header>
  );
}
