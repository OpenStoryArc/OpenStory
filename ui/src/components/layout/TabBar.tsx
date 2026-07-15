/** Tab switcher: Live / Explore. */

import type { ViewMode } from "@/lib/navigation";

interface TabBarProps {
  active: ViewMode;
  onSwitch: (mode: ViewMode) => void;
}

const TABS: { mode: ViewMode; label: string }[] = [
  { mode: "live", label: "Live" },
  { mode: "explore", label: "Explore" },
  { mode: "story", label: "Story" },
  { mode: "canvas", label: "Canvas" },
  { mode: "ask", label: "Ask" },
  { mode: "users", label: "Users" },
  { mode: "admin", label: "Admin" },
];

export function TabBar({ active, onSwitch }: TabBarProps) {
  return (
    <div
      className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto whitespace-nowrap [scrollbar-width:thin]"
      data-testid="tab-bar"
      role="tablist"
    >
      {TABS.map(({ mode, label }) => (
        <button
          key={mode}
          onClick={() => onSwitch(mode)}
          data-testid={`tab-${mode}`}
          role="tab"
          aria-selected={active === mode}
          className={`shrink-0 px-3 py-1 rounded text-sm transition-colors ${
            active === mode
              ? "bg-[color:var(--accent)] text-[color:var(--bg)] font-medium"
              : "text-[color:var(--text-muted)] hover:text-[color:var(--text)] hover:bg-[color:var(--bg-surface)]"
          }`}
        >
          {active === mode && mode === "live" && (
            <span className="inline-block w-1.5 h-1.5 rounded-full bg-[color:var(--bg)] mr-1.5 animate-pulse" />
          )}
          {label}
        </button>
      ))}
    </div>
  );
}
