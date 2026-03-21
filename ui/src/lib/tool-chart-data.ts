/** Data point for the tool usage bar chart. */
export interface ToolChartPoint {
  readonly name: string;
  readonly count: number;
}

/** Prepare tool breakdown data for a bar chart.
 *  Sorts descending by count (ties broken alphabetically by name),
 *  keeps top (maxBars - 1) tools, and buckets the rest into "Other". */
export function prepareToolChartData(
  breakdown: Readonly<Record<string, number>>,
  maxBars = 8,
): ToolChartPoint[] {
  const entries = Object.entries(breakdown);
  if (entries.length === 0) return [];

  // Sort: descending by count, then alphabetical by name for ties
  entries.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));

  if (entries.length <= maxBars) {
    return entries.map(([name, count]) => ({ name, count }));
  }

  const top = entries.slice(0, maxBars - 1);
  const rest = entries.slice(maxBars - 1);
  const otherCount = rest.reduce((sum, [, count]) => sum + count, 0);

  return [
    ...top.map(([name, count]) => ({ name, count })),
    { name: "Other", count: otherCount },
  ];
}
