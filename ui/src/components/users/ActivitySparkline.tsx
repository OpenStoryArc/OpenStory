/**
 * ActivitySparkline — 24 hourly bars colored with the user's hue.
 *
 * Inline SVG, no chart library. Each bar's height is proportional to
 * its bucket value relative to the row's max. An empty row (max=0)
 * renders a flat baseline so the card height stays stable.
 *
 * The right edge is "now" (current hour, in progress); the left edge
 * is 23h ago. A subtle grid line marks the midpoint (12h ago) so the
 * eye can place "this morning" vs "last night" without a label.
 */

import { memo } from "react";

interface ActivitySparklineProps {
  /** 24 hourly counts; index 0 = oldest, index 23 = current hour. */
  buckets: readonly number[];
  /** Hue for the bars — typically the user's `personColor`. */
  color: string;
  /** Pixel height of the rendered area. Default 28px. */
  height?: number;
  /** ARIA label for screen readers. */
  ariaLabel?: string;
}

export const ActivitySparkline = memo(function ActivitySparkline({
  buckets,
  color,
  height = 28,
  ariaLabel = "Activity over the last 24 hours",
}: ActivitySparklineProps) {
  // Always render exactly 24 bars even if the array is short — defends
  // the layout against an upstream contract change without throwing.
  const bars = Array.from({ length: 24 }, (_, i) => buckets[i] ?? 0);
  const max = Math.max(1, ...bars); // avoid division by zero
  const barWidth = 100 / 24; // % of viewBox

  return (
    <svg
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      width="100%"
      height={height}
      role="img"
      aria-label={ariaLabel}
      data-testid="activity-sparkline"
      style={{ display: "block" }}
    >
      {/* Faint baseline + midpoint divider for time orientation. */}
      <line
        x1="0"
        y1="100"
        x2="100"
        y2="100"
        stroke="#2f3348"
        strokeWidth="1"
        vectorEffect="non-scaling-stroke"
      />
      <line
        x1="50"
        y1="0"
        x2="50"
        y2="100"
        stroke="#2f3348"
        strokeWidth="1"
        strokeDasharray="2 2"
        vectorEffect="non-scaling-stroke"
        opacity="0.5"
      />
      {bars.map((v, i) => {
        // Per-bar height. Reserve 5% baseline so even a 0 bucket renders
        // a faint dot — keeps the row visually anchored.
        const h = v === 0 ? 2 : Math.max(4, (v / max) * 100);
        const y = 100 - h;
        const x = i * barWidth;
        // Current hour (rightmost bucket) gets full opacity; older
        // buckets fade to ~70% so the eye reads "now" first.
        const ageOpacity = 0.7 + (i / 23) * 0.3;
        return (
          <rect
            key={i}
            x={x + barWidth * 0.1}
            y={y}
            width={barWidth * 0.8}
            height={h}
            fill={color}
            opacity={v === 0 ? 0.15 : ageOpacity}
            data-bucket={i}
            data-value={v}
          />
        );
      })}
    </svg>
  );
});
