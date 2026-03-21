import { useState, useEffect, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { PlanDetail } from "@/types/session";
import { PlansList } from "./PlansList";

interface PlanViewerProps {
  sessionId?: string;
}

export function PlanViewer({ sessionId }: PlanViewerProps) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [plan, setPlan] = useState<PlanDetail | null>(null);
  const [hasPlans, setHasPlans] = useState(false);

  const onPlansLoaded = useCallback((count: number) => {
    setHasPlans(count > 0);
  }, []);

  useEffect(() => {
    if (!selectedId) {
      setPlan(null);
      return;
    }
    let cancelled = false;
    fetch(`/api/plans/${selectedId}`)
      .then((r) => r.json())
      .then((data: PlanDetail) => {
        if (!cancelled) setPlan(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  return (
    <div className="flex h-full">
      <PlansList
        sessionId={sessionId}
        onSelect={setSelectedId}
        selectedId={selectedId}
        onPlansLoaded={onPlansLoaded}
      />
      <div className="flex-1 overflow-y-auto p-6">
        {plan ? (
          <div className="prose prose-invert max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {plan.content}
            </ReactMarkdown>
          </div>
        ) : hasPlans ? (
          <div className="flex items-center justify-center h-full text-[#565f89] text-sm">
            Select a plan to view
          </div>
        ) : null}
      </div>
    </div>
  );
}
