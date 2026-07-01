/** Pure "fit content to viewport" transform for the Canvas board's pan/zoom.
 *  Given content bounds and the viewport size, returns the d3-zoom transform
 *  {k, x, y} that centers the (padded) content and scales it to fit, clamped so
 *  it never over-zooms past maxScale. Extracted so the scale/centering math is
 *  unit-tested independently of d3. */

export interface Bounds {
  readonly minX: number;
  readonly maxX: number;
  readonly minY: number;
  readonly maxY: number;
}

export interface Size {
  readonly w: number;
  readonly h: number;
}

export interface FitTransform {
  readonly k: number;
  readonly x: number;
  readonly y: number;
}

export function fitTransform(
  b: Bounds,
  size: Size,
  { pad = 110, maxScale = 1.4 }: { pad?: number; maxScale?: number } = {},
): FitTransform {
  const bw = Math.max(b.maxX - b.minX + pad * 2, 1);
  const bh = Math.max(b.maxY - b.minY + pad * 2, 1);
  const k = Math.min(size.w / bw, size.h / bh, maxScale);
  const cx = (b.minX + b.maxX) / 2;
  const cy = (b.minY + b.maxY) / 2;
  // d3 transform composes as: translate(w/2,h/2) · scale(k) · translate(-cx,-cy)
  // → effective translation is (w/2 - k·cx, h/2 - k·cy).
  return { k, x: size.w / 2 - k * cx, y: size.h / 2 - k * cy };
}
