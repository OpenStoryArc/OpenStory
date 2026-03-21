/** Metric grid displaying synopsis: events, tools, errors, duration, top 5 tools. */

import type { SessionSynopsis } from "@/lib/session-detail";
import { deriveSynopsisMetrics } from "@/lib/session-detail";
import { toolColor } from "@/lib/tool-colors";

interface SynopsisCardProps {
  synopsis: SessionSynopsis;
}

export function SynopsisCard({ synopsis }: SynopsisCardProps) {
  const m = deriveSynopsisMetrics(synopsis);

  return (
    <div className="space-y-3" data-testid="synopsis-card">
      {/* Metric grid */}
      <div className="grid grid-cols-4 gap-2">
        <Metric label="Events" value={m.events} />
        <Metric label="Tools" value={m.tools} />
        <Metric label="Errors" value={m.errors} color={m.errors > 0 ? "#f7768e" : undefined} />
        <Metric label="Duration" value={m.duration} />
      </div>

      {/* Top tools */}
      {synopsis.top_tools.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {synopsis.top_tools.slice(0, 5).map((t) => (
            <span
              key={t.tool}
              className="text-[11px] px-2 py-0.5 rounded font-medium"
              style={{ color: toolColor(t.tool), backgroundColor: `${toolColor(t.tool)}18` }}
            >
              {t.tool} ({t.count})
            </span>
          ))}
        </div>
      )}

      {/* Project / label context */}
      {(synopsis.label || synopsis.project_name) && (
        <div className="flex items-center gap-2 text-[11px] text-[#565f89]">
          {synopsis.project_name && <span>{synopsis.project_name}</span>}
          {synopsis.label && synopsis.project_name && <span>·</span>}
          {synopsis.label && <span className="text-[#c0caf5]">{synopsis.label}</span>}
        </div>
      )}
    </div>
  );
}

function Metric({ label, value, color }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="bg-[#1a1b26] rounded px-2 py-1.5">
      <div className="text-[10px] text-[#565f89]">{label}</div>
      <div className="text-sm font-semibold" style={{ color: color ?? "#c0caf5" }}>{value}</div>
    </div>
  );
}
