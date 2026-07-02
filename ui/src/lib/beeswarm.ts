/** Pure beeswarm packing for the Session Duration Beeswarm (Lab candidate
 *  `duration-beeswarm`). Given each dot's x pixel-position and a radius, assign
 *  a y-offset (centered on 0) so dots at similar x don't overlap — the classic
 *  greedy swarm: process left→right, place each dot at the y closest to 0 that
 *  clears all already-placed neighbors within a diameter. Side-effect-free →
 *  unit-tested; the component maps duration→x and colors by agent. */

export function beeswarmOffsets(xs: readonly number[], radius: number): number[] {
  const n = xs.length;
  const ys = new Array<number>(n).fill(0);
  const order = Array.from({ length: n }, (_, i) => i).sort((a, b) => xs[a]! - xs[b]!);
  const placed: { x: number; y: number }[] = [];
  const diam = radius * 2;
  const diam2 = diam * diam;

  for (const i of order) {
    const x = xs[i]!;
    const near = placed.filter((p) => Math.abs(p.x - x) < diam);
    // candidate ys: 0, plus the two y that just touch each nearby placed dot
    const cands = [0];
    for (const p of near) {
      const dx = x - p.x;
      const d = diam2 - dx * dx;
      if (d > 0) { const dy = Math.sqrt(d); cands.push(p.y + dy, p.y - dy); }
    }
    cands.sort((a, b) => Math.abs(a) - Math.abs(b));
    let chosen = 0;
    for (const c of cands) {
      const ok = near.every((p) => { const dx = x - p.x, dy = c - p.y; return dx * dx + dy * dy >= diam2 - 1e-6; });
      if (ok) { chosen = c; break; }
    }
    ys[i] = chosen;
    placed.push({ x, y: chosen });
  }
  return ys;
}
