/** Skeleton — shadcn-style loading placeholder.
 *
 *  A calm, layout-matched shimmer used in place of "Loading…" text so nothing
 *  shifts when real content arrives (Apple's no-jank bar). Compose bespoke
 *  skeletons from this primitive to mirror the exact shape they replace. */

import { cn } from "@/lib/cn";

export function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="skeleton"
      className={cn("skeleton-shimmer rounded-md bg-[color:var(--bg-hover)]", className)}
      {...props}
    />
  );
}
