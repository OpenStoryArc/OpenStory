/** Tab switcher: Live / Explore. */

import type { ViewMode } from "@/lib/navigation";

interface TabBarProps {
  active: ViewMode;
  onSwitch: (mode: ViewMode) => void;
}

const TABS: { mode: ViewMode; label: string }[] = [
  { mode: "live", label: "Live" },
  { mode: "overview", label: "Overview" },
  { mode: "canvas", label: "Canvas" },
  { mode: "heatmap", label: "Heatmap" },
  { mode: "lab", label: "Lab" },
  { mode: "storm", label: "Storm" },
  { mode: "ask", label: "Ask" },
  { mode: "explore", label: "Explore" },
  { mode: "story", label: "Story" },
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
              ? "bg-[#7aa2f7] text-[#1a1b26] font-medium"
              : "text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b]"
          }`}
        >
          {active === mode && mode === "live" && (
            <span className="inline-block w-1.5 h-1.5 rounded-full bg-[#1a1b26] mr-1.5 animate-pulse" />
          )}
          {label}
        </button>
      ))}
    </div>
  );
}
