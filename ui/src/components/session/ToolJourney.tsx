/** Horizontal scrollable row of color-coded tool chips showing the agent's strategy. */

import { toolColor } from "@/lib/tool-colors";
import {
  groupConsecutiveTools,
  buildJourneySequence,
  truncateFilePath,
  type ToolStep,
} from "@/lib/tool-journey";

interface ToolJourneyProps {
  steps: readonly ToolStep[];
}

export function ToolJourney({ steps }: ToolJourneyProps) {
  const groups = groupConsecutiveTools(steps);
  const elements = buildJourneySequence(groups);

  if (elements.length === 0) {
    return (
      <div className="text-xs text-[#565f89] py-2" data-testid="tool-journey-empty">
        No tool usage
      </div>
    );
  }

  return (
    <div data-testid="tool-journey">
      <div className="text-[10px] text-[#565f89] mb-1.5">
        Tool Journey ({steps.length} steps)
      </div>
      <div className="flex items-center gap-1 overflow-x-auto pb-1 scrollbar-thin">
        {elements.map((el, i) => {
          if (el.kind === "gap") {
            return (
              <span
                key={`gap-${i}`}
                className="text-[9px] text-[#565f89] px-1 shrink-0"
                title={`${el.gap.label} pause`}
              >
                ⋯ {el.gap.label}
              </span>
            );
          }

          const { group } = el;
          const color = toolColor(group.tool);
          const fileHint = group.files.length > 0
            ? group.files.map((f) => truncateFilePath(f)).join(", ")
            : undefined;

          return (
            <span
              key={`group-${i}`}
              className="text-[11px] px-2 py-0.5 rounded shrink-0 font-medium whitespace-nowrap"
              style={{ color, backgroundColor: `${color}18` }}
              title={fileHint ? `${group.tool} (${group.count}x): ${fileHint}` : `${group.tool} (${group.count}x)`}
            >
              {group.tool}
              {group.count > 1 && (
                <span className="text-[9px] opacity-60 ml-0.5">×{group.count}</span>
              )}
            </span>
          );
        })}
      </div>
    </div>
  );
}
