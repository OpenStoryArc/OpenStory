/**
 * Deterministic project color assignment.
 *
 * Sibling of `personColor()` and `sessionColor()`, with a deliberately
 * different palette so a project chip and a person chip rendered next
 * to each other don't visually clash. Uses a teal/green/sand range —
 * earthy backgrounds against the sidebar's blue/purple foregrounds.
 */

const PROJECT_COLORS = [
  "#73daca", // teal
  "#9ece6a", // green
  "#e0af68", // sand
  "#7dcfff", // pale cyan
  "#bb9af7", // muted purple (kept narrow to avoid clashing)
  "#ff9e64", // amber
  "#a3be8c", // moss
  "#ebcb8b", // wheat
] as const;

/** Stable color for a project identifier (project_name or project_id). */
export function projectColor(project: string): string {
  let hash = 0;
  for (let i = 0; i < project.length; i++) {
    hash = (hash << 5) - hash + project.charCodeAt(i);
    hash |= 0;
  }
  const idx = Math.abs(hash) % PROJECT_COLORS.length;
  return PROJECT_COLORS[idx]!;
}
