/** Turn outline for faceted navigation — collapsible list of turns with tool summaries. */

import type { Turn } from "@/lib/event-graph";
import { toolColor } from "@/lib/tool-colors";

interface TurnOutlineProps {
  turns: readonly Turn[];
  selectedTurn: number | null;
  onSelectTurn: (index: number | null) => void;
}

export function TurnOutline({ turns, selectedTurn, onSelectTurn }: TurnOutlineProps) {
  if (turns.length === 0) return null;

  return (
    <div data-testid="turn-outline">
      <div className="px-2 py-1 text-[10px] text-[#565f89] uppercase tracking-wider">
        Turns
      </div>
      <div className="space-y-px">
        {turns.map((t) => {
          const isSelected = selectedTurn === t.index;
          const toolEntries = Object.entries(t.toolCounts).sort((a, b) => b[1] - a[1]);
          const prompt = t.promptText
            ? t.promptText.length > 40 ? t.promptText.slice(0, 40) + "..." : t.promptText
            : null;

          return (
            <button
              key={t.index}
              onClick={() => onSelectTurn(isSelected ? null : t.index)}
              className={`w-full text-left px-2 py-1.5 text-xs transition-colors ${
                isSelected
                  ? "bg-[#7aa2f715] border-l-2 border-[#7aa2f7]"
                  : "hover:bg-[#24283b] border-l-2 border-transparent"
              }`}
              data-testid={`turn-${t.index}`}
            >
              <div className="flex items-center gap-1.5">
                <span className="text-[10px] text-[#565f89] font-mono shrink-0">
                  {t.index + 1}
                </span>
                {prompt && (
                  <span className="text-[10px] text-[#c0caf5] truncate">{prompt}</span>
                )}
                {!prompt && (
                  <span className="text-[10px] text-[#565f89] italic">no prompt</span>
                )}
                {t.hasError && (
                  <span className="text-[9px] text-[#f7768e] shrink-0">!</span>
                )}
                <span className="text-[9px] text-[#565f89] ml-auto shrink-0">
                  {t.eventIds.length}
                </span>
              </div>
              {toolEntries.length > 0 && (
                <div className="flex items-center gap-1 mt-0.5 flex-wrap">
                  {toolEntries.slice(0, 4).map(([name, count]) => (
                    <span
                      key={name}
                      className="text-[8px] px-1 rounded"
                      style={{ color: toolColor(name), backgroundColor: `${toolColor(name)}15` }}
                    >
                      {name}({count})
                    </span>
                  ))}
                  {t.files.length > 0 && (
                    <span className="text-[8px] text-[#565f89] ml-auto">
                      {t.files.length}f
                    </span>
                  )}
                </div>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
