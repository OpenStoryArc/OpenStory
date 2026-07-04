/** plan→turn: which event authored a plan.
 *
 *  A stored plan carries (title, timestamp); its authoring moment is an
 *  ExitPlanMode tool call in the session. Title match (via the same
 *  extractor the facets use) is authoritative; when titles drift, the
 *  closest-in-time ExitPlanMode call is the honest fallback. Null when the
 *  session has none — no wrong guesses.
 */

import type { WireRecord } from "@/types/wire-record";
import type { ToolCall } from "@/types/view-record";
import { extractPlanTitle } from "@/lib/event-graph";

interface PlanRef {
  readonly title: string;
  readonly timestamp: string;
}

export function planSourceEventId(
  records: readonly WireRecord[],
  plan: PlanRef,
): string | null {
  const exits = records.filter(
    (r) => r.record_type === "tool_call" && (r.payload as ToolCall)?.name === "ExitPlanMode",
  );
  if (exits.length === 0) return null;

  const byTitle = exits.find((r) => extractPlanTitle(r) === plan.title);
  if (byTitle) return byTitle.id;

  const planMs = Date.parse(plan.timestamp);
  if (!Number.isFinite(planMs)) return null;
  let best: WireRecord | null = null;
  let bestDelta = Infinity;
  for (const r of exits) {
    const t = Date.parse(r.timestamp);
    if (!Number.isFinite(t)) continue;
    const delta = Math.abs(t - planMs);
    if (delta < bestDelta) {
      bestDelta = delta;
      best = r;
    }
  }
  return best?.id ?? null;
}
