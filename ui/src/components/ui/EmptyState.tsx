/** Shared empty state: says what WOULD be here and (optionally) what fills it.
 *  Replaces bare one-line "No X" strings so empty views still feel designed. */

export function EmptyState({ title, hint }: { title: string; hint?: string }) {
  return (
    <div className="flex grow flex-col items-center justify-center gap-1 px-6 py-8 text-center">
      <span className="text-[length:var(--fs-emph)] text-[color:var(--text-bright)]">{title}</span>
      {hint && <span className="max-w-[42ch] text-[length:var(--fs-body)] text-[color:var(--text-muted)]">{hint}</span>}
    </div>
  );
}
