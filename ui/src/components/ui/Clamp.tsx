/** Clamp — clipped text that is ALWAYS reachable. The primitive the
 *  no-dead-end-truncation sweep converges on: renders the full text (never
 *  drops it), clamps it to N lines by default, exposes the whole thing as a
 *  hover `title`, and expands on click / Enter. Replaces bare `truncate` /
 *  `line-clamp` and `.slice(0,N)` display clips so a user can always read the
 *  rest. Keep it small — it's used in dozens of places. */

import { useState, type KeyboardEvent } from "react";
import { cn } from "@/lib/cn";

/** Static clamp classes (literal so Tailwind's scanner emits them). */
const CLAMP: Record<number, string> = {
  1: "truncate",
  2: "line-clamp-2",
  3: "line-clamp-3",
  4: "line-clamp-4",
  5: "line-clamp-5",
};

export function Clamp({
  text,
  lines = 1,
  className,
  expandedClassName,
}: {
  text: string;
  /** lines to show while collapsed (1 = single-line truncate). */
  lines?: number;
  className?: string;
  /** extra classes applied only when expanded (e.g. a scroll cap). */
  expandedClassName?: string;
}) {
  const [open, setOpen] = useState(false);
  const toggle = () => setOpen((v) => !v);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle();
    }
  };

  return (
    <span
      data-testid="clamp"
      data-open={open}
      role="button"
      tabIndex={0}
      title={open ? undefined : text || undefined}
      onClick={(e) => {
        e.stopPropagation();
        toggle();
      }}
      onKeyDown={onKeyDown}
      className={cn(
        "cursor-pointer",
        className,
        open ? cn("whitespace-pre-wrap break-words", expandedClassName) : (CLAMP[lines] ?? "line-clamp-3"),
      )}
    >
      {text}
    </span>
  );
}
