import { useState, useEffect } from "react";
import type { ActivitySummary as ActivityData } from "@/types/session";
import { formatDuration } from "@/lib/time";
import { truncate, OP_COLORS } from "@/lib/event-transforms";
import { ToolChart } from "@/components/analytics/ToolChart";

interface ActivitySummaryProps {
  sessionId: string;
}

export function ActivitySummary({ sessionId }: ActivitySummaryProps) {
  const [data, setData] = useState<ActivityData | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetch(`/api/sessions/${sessionId}/activity`)
      .then((r) => r.json())
      .then((d: ActivityData) => {
        if (!cancelled) setData(d);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (!data) {
    return (
      <div className="p-4 text-sm text-[#565f89]">Loading activity...</div>
    );
  }

  return (
    <div className="p-4 space-y-4">
      {/* First prompt */}
      <div>
        <h3 className="text-xs text-[#565f89] mb-1">First Prompt</h3>
        <p className="text-sm text-[#c0caf5]">
          {truncate(data.first_prompt, 200)}
        </p>
      </div>

      {/* Key metrics */}
      <div className="grid grid-cols-3 gap-3">
        <MetricCard label="Turns" value={data.conversation_turns} />
        <MetricCard label="Plans" value={data.plan_count} />
        <MetricCard
          label="Duration"
          value={data.duration_ms != null ? formatDuration(data.duration_ms) : "—"}
        />
      </div>

      {/* Files touched */}
      {data.files_touched.length > 0 && (
        <div>
          <h3 className="text-xs text-[#565f89] mb-2">
            Files Touched ({data.files_touched.length})
          </h3>
          <div className="flex flex-wrap gap-1">
            {data.files_touched.map((f, i) => {
              const color =
                OP_COLORS[f.operation as keyof typeof OP_COLORS] ?? "#565f89";
              const basename = f.path.split(/[\\/]/).pop() ?? f.path;
              return (
                <span
                  key={i}
                  className="text-xs px-2 py-0.5 rounded"
                  style={{
                    color,
                    backgroundColor: `${color}20`,
                  }}
                  title={f.path}
                >
                  {basename}
                </span>
              );
            })}
          </div>
        </div>
      )}

      {/* Tool distribution chart */}
      {data.tool_breakdown && Object.keys(data.tool_breakdown).length > 0 && (
        <ToolChart breakdown={data.tool_breakdown} />
      )}

      {/* Errors */}
      {data.error_messages.length > 0 && (
        <div>
          <h3 className="text-xs text-[#f7768e] mb-2">
            Errors ({data.error_messages.length})
          </h3>
          <div className="space-y-1">
            {data.error_messages.map((msg, i) => (
              <div
                key={i}
                className="text-xs text-[#f7768e] bg-[#f7768e10] rounded p-2 font-mono"
              >
                {truncate(msg, 200)}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Last response */}
      {data.last_response && (
        <div>
          <h3 className="text-xs text-[#565f89] mb-1">Last Response</h3>
          <p className="text-xs text-[#c0caf5]">
            {truncate(data.last_response, 300)}
          </p>
        </div>
      )}
    </div>
  );
}

function MetricCard({
  label,
  value,
}: {
  label: string;
  value: string | number;
}) {
  return (
    <div className="bg-[#24283b] rounded p-3">
      <div className="text-xs text-[#565f89]">{label}</div>
      <div className="text-lg font-semibold text-[#c0caf5]">{value}</div>
    </div>
  );
}
