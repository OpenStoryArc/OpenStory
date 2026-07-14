import type { StatusFilter } from "@/lib/filters";

const FILTERS: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "ongoing", label: "Ongoing" },
  { value: "completed", label: "Completed" },
  { value: "errored", label: "Errored" },
  { value: "stale", label: "Stale" },
];

interface SessionFilterBarProps {
  value: StatusFilter;
  onChange: (filter: StatusFilter) => void;
}

export function SessionFilterBar({ value, onChange }: SessionFilterBarProps) {
  return (
    <div className="flex gap-1">
      {FILTERS.map((f) => (
        <button
          key={f.value}
          onClick={() => onChange(f.value)}
          className={`text-xs px-2 py-1 rounded transition-colors ${
            value === f.value
              ? "bg-[color:var(--accent)] text-[color:var(--bg)]"
              : "text-[color:var(--text-muted)] hover:text-[color:var(--text)] hover:bg-[color:var(--bg-hover)]"
          }`}
        >
          {f.label}
        </button>
      ))}
    </div>
  );
}
