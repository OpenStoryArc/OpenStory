/** Class-name merge helper — the shadcn/ui convention.
 *  clsx composes conditional class lists; tailwind-merge resolves conflicting
 *  Tailwind utilities so the last one wins (e.g. `px-2 px-4` → `px-4`). */
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
