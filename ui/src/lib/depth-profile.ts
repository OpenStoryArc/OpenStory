/** Downsample a depth array into fixed-width buckets using max-in-bucket.
 *  Returns one value per bucket — the maximum depth in that bucket's range.
 *  Used for sparkline visualization in the sidebar. */
export function sampleDepthProfile(
  depths: readonly number[],
  buckets = 40,
): number[] {
  const n = depths.length;
  if (n === 0) return [];
  if (n <= buckets) return [...depths];

  const result: number[] = [];
  for (let i = 0; i < buckets; i++) {
    const start = Math.floor((i * n) / buckets);
    const end = Math.floor(((i + 1) * n) / buckets);
    let max = 0;
    for (let j = start; j < end; j++) {
      if (depths[j]! > max) max = depths[j]!;
    }
    result.push(max);
  }
  return result;
}
