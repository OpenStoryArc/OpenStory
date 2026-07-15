import { useState, useEffect, useCallback, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { PlanDetail } from "@/types/session";
import { PlansList } from "./PlansList";
import { useSessionRecords } from "@/hooks/use-session-records";
import { planSourceEventId } from "@/lib/plan-source";

interface PlanViewerProps {
  sessionId?: string;
  /** Pre-select a plan (deep links / tests). */
  initialPlanId?: string;
}

export function PlanViewer({ sessionId, initialPlanId }: PlanViewerProps) {
  const [selectedId, setSelectedId] = useState<string | null>(initialPlanId ?? null);
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

  // plan→turn: find the ExitPlanMode event that authored the selected plan
  // (records come from the shared cache — free if any surface loaded them).
  const { records } = useSessionRecords(plan?.session_id ?? null);
  const sourceEventId = useMemo(
    () => (plan ? planSourceEventId(records, plan) : null),
    [plan, records],
  );

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
          <div>
            {sourceEventId && (
              <a
                href={`#/story/${plan.session_id}/event/${sourceEventId}`}
                data-testid="plan-turn-link"
                className="mb-3 inline-block text-[11px] text-[color:var(--accent)] hover:underline"
                title="Open the turn that authored this plan in Story"
              >
                ↑ Turn that authored this plan
              </a>
            )}
            <div className="prose prose-invert max-w-none">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {plan.content}
              </ReactMarkdown>
            </div>
          </div>
        ) : hasPlans ? (
          <div className="flex items-center justify-center h-full text-[color:var(--text-muted)] text-sm">
            Select a plan to view
          </div>
        ) : null}
      </div>
    </div>
  );
}
