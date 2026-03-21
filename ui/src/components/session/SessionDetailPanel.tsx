/** Collapsible session detail panel — fetches synopsis, file impact, errors, tool journey. */

import { useState, useEffect, useCallback } from "react";
import type { SessionSynopsis, FileImpact, SessionError } from "@/lib/session-detail";
import type { ToolStep } from "@/lib/tool-journey";
import { SynopsisCard } from "./SynopsisCard";
import { FileImpactTable } from "./FileImpactTable";
import { ErrorList } from "./ErrorList";
import { ToolJourney } from "./ToolJourney";

interface SessionDetailPanelProps {
  sessionId: string;
}

interface DetailData {
  synopsis: SessionSynopsis | null;
  files: readonly FileImpact[];
  errors: readonly SessionError[];
  toolSteps: readonly ToolStep[];
}

export function SessionDetailPanel({ sessionId }: SessionDetailPanelProps) {
  const [data, setData] = useState<DetailData | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setData(null);

    // Fetch all 4 endpoints in parallel
    Promise.all([
      fetch(`/api/sessions/${sessionId}/synopsis`).then((r) => r.json()).catch(() => null),
      fetch(`/api/sessions/${sessionId}/file-impact`).then((r) => r.json()).catch(() => []),
      fetch(`/api/sessions/${sessionId}/errors`).then((r) => r.json()).catch(() => []),
      fetch(`/api/sessions/${sessionId}/tool-journey`).then((r) => r.json()).catch(() => []),
    ]).then(([synopsis, files, errors, toolSteps]) => {
      if (!cancelled) {
        setData({ synopsis, files, errors, toolSteps });
        setLoading(false);
      }
    });

    return () => { cancelled = true; };
  }, [sessionId]);

  const toggleCollapse = useCallback(() => setCollapsed((c) => !c), []);

  return (
    <div
      className="border-b border-[#2f3348] bg-[#24283b]"
      data-testid="session-detail-panel"
    >
      {/* Header bar — always visible */}
      <button
        onClick={toggleCollapse}
        className="w-full flex items-center justify-between px-3 py-1.5 text-xs text-[#565f89] hover:text-[#c0caf5] transition-colors"
      >
        <span className="flex items-center gap-2">
          <span className="text-[10px]">{collapsed ? "▸" : "▾"}</span>
          <span>Session Detail</span>
          {data?.synopsis?.label && (
            <span className="text-[#c0caf5] font-medium">{data.synopsis.label}</span>
          )}
        </span>
        <span className="font-mono text-[10px]">{sessionId.slice(0, 8)}</span>
      </button>

      {/* Content — collapsible */}
      {!collapsed && (
        <div className="px-3 pb-3 space-y-3">
          {loading ? (
            <div className="text-xs text-[#565f89] py-2">Loading session detail...</div>
          ) : data ? (
            <>
              {data.synopsis && <SynopsisCard synopsis={data.synopsis} />}
              <ToolJourney steps={data.toolSteps} />
              <div className="grid grid-cols-2 gap-3">
                <FileImpactTable files={data.files} />
                <ErrorList errors={data.errors} />
              </div>
            </>
          ) : (
            <div className="text-xs text-[#565f89] py-2">No data available</div>
          )}
        </div>
      )}
    </div>
  );
}
