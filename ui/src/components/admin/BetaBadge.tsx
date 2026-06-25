/**
 * Beta marker for features that work in testing but aren't yet guaranteed —
 * the sharing/permissions surfaces are new and still being hardened. The pill
 * is the eye-catch; the `title` carries the full disclaimer for hover/AT.
 */
const DEFAULT_NOTE =
  "Beta — sharing & permissions are new and not guaranteed to work yet. Keep testing before relying on this.";

export function BetaBadge({ note }: { note?: string }) {
  return (
    <span
      data-testid="beta-badge"
      title={note ?? DEFAULT_NOTE}
      className="ml-2 inline-flex items-center rounded-full border border-amber-500/40 bg-amber-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-amber-300"
    >
      Beta
    </span>
  );
}
