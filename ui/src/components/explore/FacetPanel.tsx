/** Facet panel: file and tool indexes for filtering events. */

import type { FileFacet, ToolFacet, PlanFacet } from "@/lib/event-graph";
import { toolColor } from "@/lib/tool-colors";

interface FacetPanelProps {
  files: readonly FileFacet[];
  tools: readonly ToolFacet[];
  plans: readonly PlanFacet[];
  selectedFile: string | null;
  selectedTool: string | null;
  selectedPlan: string | null;
  onSelectFile: (path: string | null) => void;
  onSelectTool: (name: string | null) => void;
  onSelectPlan: (title: string | null) => void;
}

export function FacetPanel({ files, tools, plans, selectedFile, selectedTool, selectedPlan, onSelectFile, onSelectTool, onSelectPlan }: FacetPanelProps) {
  return (
    <div data-testid="facet-panel">
      {/* Files */}
      {files.length > 0 && (
        <div>
          <div className="px-2 py-1 text-[10px] text-[#565f89] uppercase tracking-wider border-t border-[#2f3348]">
            Files ({files.length})
          </div>
          <div className="max-h-[180px] overflow-y-auto">
            {files.slice(0, 20).map((f) => {
              const isSelected = selectedFile === f.path;
              const basename = f.path.replace(/\\/g, "/").split("/").pop() ?? f.path;
              return (
                <button
                  key={f.path}
                  onClick={() => onSelectFile(isSelected ? null : f.path)}
                  className={`w-full text-left px-2 py-1 text-xs transition-colors flex items-center gap-1.5 ${
                    isSelected
                      ? "bg-[#7aa2f715] text-[#7aa2f7]"
                      : "text-[#a9b1d6] hover:bg-[#24283b]"
                  }`}
                  title={f.path}
                >
                  <span className="truncate font-mono text-[10px]">{basename}</span>
                  <span className="ml-auto flex items-center gap-1 shrink-0">
                    {f.reads > 0 && <span className="text-[8px] text-[#7aa2f7]">{f.reads}R</span>}
                    {f.writes > 0 && <span className="text-[8px] text-[#e0af68]">{f.writes}W</span>}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* Tools */}
      {tools.length > 0 && (
        <div>
          <div className="px-2 py-1 text-[10px] text-[#565f89] uppercase tracking-wider border-t border-[#2f3348]">
            Tools ({tools.length})
          </div>
          {tools.map((t) => {
            const isSelected = selectedTool === t.name;
            const color = toolColor(t.name);
            return (
              <button
                key={t.name}
                onClick={() => onSelectTool(isSelected ? null : t.name)}
                className={`w-full text-left px-2 py-1 text-xs transition-colors flex items-center gap-1.5 ${
                  isSelected
                    ? "bg-[#7aa2f715]"
                    : "hover:bg-[#24283b]"
                }`}
              >
                <span
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ backgroundColor: color }}
                />
                <span style={{ color }} className="font-medium text-[10px]">{t.name}</span>
                <span className="text-[9px] text-[#565f89] ml-auto">
                  {t.count} in {t.turnCount}t
                </span>
              </button>
            );
          })}
        </div>
      )}

      {/* Plans */}
      {plans.length > 0 && (
        <div>
          <div className="px-2 py-1 text-[10px] text-[#565f89] uppercase tracking-wider border-t border-[#2f3348]">
            Plans ({plans.length})
          </div>
          {plans.map((p) => {
            const isSelected = selectedPlan === p.title;
            return (
              <button
                key={p.title}
                onClick={() => onSelectPlan(isSelected ? null : p.title)}
                className={`w-full text-left px-2 py-1 text-xs transition-colors flex items-center gap-1.5 ${
                  isSelected
                    ? "bg-[#e0af6815] text-[#e0af68]"
                    : "text-[#a9b1d6] hover:bg-[#24283b]"
                }`}
                title={p.title}
              >
                <span
                  className="w-2 h-2 rounded-sm shrink-0 bg-[#e0af68]"
                />
                <span className="truncate text-[10px]">{p.title}</span>
                <span className="text-[9px] text-[#565f89] ml-auto shrink-0">
                  {p.count}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
