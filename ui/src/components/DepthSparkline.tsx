/** Tiny SVG depth sparkline — shows session "shape" at a glance. */

interface DepthSparklineProps {
  profile: readonly number[];
  color?: string;
  height?: number;
}

export function DepthSparkline({
  profile,
  color = "#7aa2f7",
  height = 20,
}: DepthSparklineProps) {
  if (profile.length === 0) return null;

  const max = Math.max(...profile);
  if (max === 0) return null;

  const w = profile.length;
  const points = profile
    .map((d, i) => `${i},${height - (d / max) * height}`)
    .join(" ");
  // Closed polygon for filled area
  const areaPoints = `0,${height} ${points} ${w - 1},${height}`;

  return (
    <svg
      width="100%"
      height={height}
      viewBox={`0 0 ${w} ${height}`}
      preserveAspectRatio="none"
      className="block"
      data-testid="depth-sparkline"
    >
      <polygon
        points={areaPoints}
        fill={`${color}20`}
        stroke="none"
      />
      <polyline
        points={points}
        fill="none"
        stroke={color}
        strokeWidth="1"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
