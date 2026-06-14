import type { ReactNode } from "react";

/**
 * HowItWorks — a collapsible "under the hood" disclosure for an admin
 * subsection. Keeps the deep technical explanation out of the way by default
 * but one click from anyone who wants the mechanism.
 */
export function HowItWorks({
  summary = "How it works under the hood",
  children,
}: {
  summary?: string;
  children: ReactNode;
}) {
  return (
    <details className="mt-3 rounded border border-[#24283b] bg-[#16161e]/60 text-xs text-[#9aa5ce]">
      <summary className="cursor-pointer select-none px-3 py-2 font-medium text-[#7aa2f7]">
        {summary}
      </summary>
      <div className="space-y-2 px-3 pb-3 pt-1 leading-relaxed">{children}</div>
    </details>
  );
}
