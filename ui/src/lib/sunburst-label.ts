/** Pure label-layout decision for sunburst wedges. A wedge only gets an inline
 *  label when it's big enough to hold readable text; the label reads radially
 *  (outward), flipped on the left half so it stays upright. Extracted so the
 *  fit/orientation logic is unit-tested independently of SVG. */

export interface Arc {
  /** start/end angle in radians (d3 partition: 0 at 12 o'clock, clockwise). */
  readonly x0: number;
  readonly x1: number;
  /** inner/outer radius in px. */
  readonly y0: number;
  readonly y1: number;
}

export interface SunburstLabel {
  /** whether the wedge is large enough to label at all. */
  readonly show: boolean;
  /** rotation (deg) to align text along the wedge's mid-angle radius. */
  readonly angleDeg: number;
  /** true when the wedge is on the left half → text is flipped 180° upright. */
  readonly flip: boolean;
  /** inner radius to translate the text out to. */
  readonly innerR: number;
  /** how many chars fit in the radial thickness (rough, at ~6px/char). */
  readonly maxChars: number;
}

export interface CenterText {
  readonly primary: string;
  readonly secondary: string;
}

/** Text for the sunburst's center hole: the hovered wedge's name + metric value
 *  when hovering, else the current focus (the drill root). Pure so the readout
 *  is unit-tested independently of the SVG hover wiring. */
export function sunburstCenterText(
  hovered: { name: string; value: number } | null,
  focus: { name: string; depth: number },
  metric: string,
): CenterText {
  if (hovered) {
    return {
      primary: hovered.name.slice(0, 18),
      secondary: `${Math.round(hovered.value).toLocaleString()} ${metric}`,
    };
  }
  return {
    primary: focus.depth === 0 ? "all" : focus.name.slice(0, 18),
    secondary: "hover a wedge",
  };
}

export function sunburstLabelLayout(a: Arc): SunburstLabel {
  const angular = a.x1 - a.x0;
  const thickness = a.y1 - a.y0;
  const midR = (a.y0 + a.y1) / 2;
  const mid = (a.x0 + a.x1) / 2;
  // Tangential room for the ~10px glyph height is angular·midR; radial room for
  // the string is the ring thickness. Need both to clear a minimum.
  const show = angular * midR > 14 && thickness > 24;
  const flip = mid > Math.PI;
  return {
    show,
    angleDeg: (mid * 180) / Math.PI - 90,
    flip,
    innerR: a.y0 + 4,
    maxChars: Math.max(1, Math.floor((thickness - 8) / 6)),
  };
}
