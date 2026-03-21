/** GitFlowCard — horizontal flow visualization for git.workflow patterns.
 *  Shows each git command as a colored badge connected by arrows.
 *  Destructive steps get a red border. */

import { parseGitFlowSteps } from "@/lib/git-flow-data";

interface GitFlowCardProps {
  metadata: Readonly<Record<string, unknown>>;
}

export function GitFlowCard({ metadata }: GitFlowCardProps) {
  const steps = parseGitFlowSteps(metadata);
  if (steps.length === 0) return null;

  return (
    <div
      className="flex items-center gap-1 flex-wrap py-1"
      data-testid="git-flow-card"
    >
      {steps.map((step, i) => (
        <span key={i} className="flex items-center gap-1">
          {i > 0 && (
            <span className="text-[#565f89] text-[10px]">&rarr;</span>
          )}
          <span
            className="text-[10px] px-1.5 py-0.5 rounded font-mono"
            style={{
              color: step.color,
              backgroundColor: `${step.color}15`,
              border: step.risk === "destructive"
                ? `1px solid ${step.color}`
                : `1px solid ${step.color}30`,
            }}
            title={step.command}
          >
            {step.verb}
          </span>
        </span>
      ))}
    </div>
  );
}
