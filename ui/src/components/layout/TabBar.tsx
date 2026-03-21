/** Tab switcher: Live / Explore. */

import type { ViewMode } from "@/lib/navigation";

interface TabBarProps {
  active: ViewMode;
  onSwitch: (mode: ViewMode) => void;
}

const TABS: { mode: ViewMode; label: string }[] = [
  { mode: "live", label: "Live" },
  { mode: "explore", label: "Explore" },
];

export function TabBar({ active, onSwitch }: TabBarProps) {
  return (
    <div className="flex items-center gap-1" data-testid="tab-bar">
      {TABS.map(({ mode, label }) => (
        <button
          key={mode}
          onClick={() => onSwitch(mode)}
          data-testid={`tab-${mode}`}
          className={`px-3 py-1 rounded text-sm transition-colors ${
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
