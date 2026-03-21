/** Session detail view for the Explore tab — fetches and displays full session data from REST. */

import { useState, useEffect } from "react";
import type { SessionSynopsis, FileImpact, SessionError } from "@/lib/session-detail";
import type { ToolStep } from "@/lib/tool-journey";
import { SynopsisCard } from "@/components/session/SynopsisCard";
import { FileImpactTable } from "@/components/session/FileImpactTable";
import { ErrorList } from "@/components/session/ErrorList";
import { ToolJourney } from "@/components/session/ToolJourney";

interface ExploreDetailProps {
  sessionId: string;
}

interface DetailData {
  synopsis: SessionSynopsis | null;
  files: readonly FileImpact[];
  errors: readonly SessionError[];
  toolSteps: readonly ToolStep[];
}

export function ExploreDetail({ sessionId }: ExploreDetailProps) {
  const [data, setData] = useState<DetailData | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setData(null);

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

  if (loading) {
    return <div className="p-4 text-xs text-[#565f89]">Loading session detail...</div>;
  }

  if (!data) {
    return <div className="p-4 text-xs text-[#565f89]">No data available</div>;
  }

  return (
    <div className="space-y-4 p-4" data-testid="explore-detail">
      {data.synopsis && <SynopsisCard synopsis={data.synopsis} />}
      <ToolJourney steps={data.toolSteps} />
      <div className="grid grid-cols-2 gap-4">
        <FileImpactTable files={data.files} />
        <ErrorList errors={data.errors} />
      </div>
    </div>
  );
}
