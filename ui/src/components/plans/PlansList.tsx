import { useState, useEffect } from "react";
import type { PlanSummary } from "@/types/session";
import { relativeTime } from "@/lib/time";

interface PlansListProps {
  sessionId?: string;
  onSelect: (planId: string) => void;
  selectedId: string | null;
  onPlansLoaded?: (count: number) => void;
}

export function PlansList({ sessionId, onSelect, selectedId, onPlansLoaded }: PlansListProps) {
  const [plans, setPlans] = useState<PlanSummary[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    const url = sessionId
      ? `/api/sessions/${sessionId}/plans`
      : "/api/plans";
    let cancelled = false;
    fetch(url)
      .then((r) => r.json())
      .then((data: PlanSummary[]) => {
        if (!cancelled) {
          setPlans(data);
          setLoading(false);
          onPlansLoaded?.(data.length);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, onPlansLoaded]);

  if (loading) {
    return (
      <div className="p-4 text-sm text-[#565f89] animate-pulse">Loading plans...</div>
    );
  }

  if (plans.length === 0) {
    return (
      <div className="p-4 text-sm text-[#565f89]">No plans available</div>
    );
  }

  return (
    <div className="border-r border-[#2f3348] w-64 overflow-y-auto">
      {plans.map((p) => (
        <button
          key={p.id}
          onClick={() => onSelect(p.id)}
          className={`w-full text-left p-3 border-b border-[#2f3348] transition-colors ${
            selectedId === p.id ? "bg-[#2f3348]" : "hover:bg-[#24283b]"
          }`}
        >
          <div className="text-sm text-[#c0caf5] mb-1 truncate">{p.title}</div>
          <div className="text-xs text-[#565f89]">
            {relativeTime(p.timestamp)}
          </div>
        </button>
      ))}
    </div>
  );
}
