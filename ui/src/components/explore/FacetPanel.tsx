/** Facet panel: file and tool indexes for filtering events. */

import type { FileFacet, ToolFacet, PlanFacet } from "@/lib/event-graph";
import { toolColor } from "@/lib/tool-colors";
import { buildHash } from "@/lib/hash-route";

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
          <div className="px-2 py-1 text-[10px] text-[color:var(--text-muted)] uppercase tracking-wider border-t border-[color:var(--bg-hover)]">
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
                      ? "bg-[color:var(--accent)]/8 text-[color:var(--accent)]"
                      : "text-[color:var(--text-bright)] hover:bg-[color:var(--bg-surface)]"
                  }`}
                  title={f.path}
                >
                  <span className="truncate font-mono text-[10px]">{basename}</span>
                  <span className="ml-auto flex items-center gap-1 shrink-0">
                    {f.reads > 0 && <span className="text-[8px] text-[color:var(--accent)]">{f.reads}R</span>}
                    {f.writes > 0 && <span className="text-[8px] text-[color:var(--orange)]">{f.writes}W</span>}
                  </span>
                </button>
              );
            })}
            {/* file→session: the selected file's impact ACROSS sessions —
                lands on the cross-session FTS search for the path. */}
            {selectedFile && (
              <a
                href={buildHash({ view: "explore", detailView: "search", searchQuery: selectedFile })}
                data-testid="file-impact-link"
                className="block px-2 py-1 text-[10px] text-[color:var(--accent)] hover:underline"
                title={`All sessions touching ${selectedFile}`}
              >
                ↺ Impact across sessions
              </a>
            )}
          </div>
        </div>
      )}

      {/* Tools */}
      {tools.length > 0 && (
        <div>
          <div className="px-2 py-1 text-[10px] text-[color:var(--text-muted)] uppercase tracking-wider border-t border-[color:var(--bg-hover)]">
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
                    ? "bg-[color:var(--accent)]/8"
                    : "hover:bg-[color:var(--bg-surface)]"
                }`}
              >
                <span
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ backgroundColor: color }}
                />
                <span style={{ color }} className="font-medium text-[10px]">{t.name}</span>
                <span className="text-[9px] text-[color:var(--text-muted)] ml-auto">
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
          <div className="px-2 py-1 text-[10px] text-[color:var(--text-muted)] uppercase tracking-wider border-t border-[color:var(--bg-hover)]">
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
                    ? "bg-[color:var(--orange)]/8 text-[color:var(--orange)]"
                    : "text-[color:var(--text-bright)] hover:bg-[color:var(--bg-surface)]"
                }`}
                title={p.title}
              >
                <span
                  className="w-2 h-2 rounded-sm shrink-0 bg-[color:var(--orange)]"
                />
                <span className="truncate text-[10px]">{p.title}</span>
                <span className="text-[9px] text-[color:var(--text-muted)] ml-auto shrink-0">
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
