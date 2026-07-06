/** Heatmap3D — the isometric 3D stacks (lazy-loaded so three.js only ships when
 *  you toggle into 3D). Each day is a column at (week, day); each session is a
 *  box in the stack, largest (warm) at the base → smallest (cool) on top. Stacks
 *  rise on entrance, NEWEST week first, cascading back in time. Hover/click land
 *  in the next increment. */

import { useMemo, useRef } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import type { Mesh } from "three";
import type { HashRoute } from "@/lib/hash-route";
import type { HeatGrid } from "@/lib/heatmap";

const SP = 1.0;      // grid spacing
const BOX = 0.82;    // box footprint
const BOXH = 0.4;    // per-session height
const DUR = 0.55;    // rise duration (s)
const MAXH = 12;     // cap visible boxes; taller days get an overflow cap on top
// warm (largest, base) → cool (smallest, top)
const RAMP = ["#ff9e64", "#f7a86a", "#e0af68", "#f7c56a", "#d7d06a", "#9ece6a", "#73daca", "#5fc7d4", "#7dcfff", "#7aa2f7", "#9d86f0", "#bb9af7"];
const OVERFLOW = "#c0caf5";

interface Box {
  x: number; z: number; baseY: number; color: string; delay: number;
  date: string; overflow: boolean;
}

function easeOut(x: number) { return 1 - Math.pow(1 - x, 3); }

function Stacks({ grid, onDay }: { grid: HeatGrid; onDay: (date: string) => void }) {
  const refs = useRef<(Mesh | null)[]>([]);
  const start = useRef<number | null>(null);

  // faint calendar floor — every present day, so the grid structure reads even
  // where there's no activity (keeps a lot of data legible).
  const tiles = useMemo(() => {
    const cx = (grid.weeks - 1) / 2;
    return grid.cells.filter((c) => c.present).map((c) => ({
      x: (c.week - cx) * SP, z: (c.day - 3) * SP, lit: c.count > 0,
    }));
  }, [grid]);

  const boxes = useMemo<Box[]>(() => {
    const out: Box[] = [];
    const maxWeek = grid.weeks - 1;
    const cx = (grid.weeks - 1) / 2;
    for (const c of grid.cells) {
      if (!c.count) continue;
      const x = (c.week - cx) * SP;
      const z = (c.day - 3) * SP;
      const weekDelay = (maxWeek - c.week) * 0.04; // newest week first
      const shown = Math.min(c.count, MAXH);
      for (let L = 0; L < shown; L++) {
        const isOverflow = c.count > MAXH && L === shown - 1;
        out.push({
          x, z, baseY: L * BOXH,
          color: isOverflow ? OVERFLOW : RAMP[Math.min(L, RAMP.length - 1)]!,
          delay: weekDelay + L * 0.03, date: c.date, overflow: isOverflow,
        });
      }
    }
    return out;
  }, [grid]);

  useFrame(({ clock }) => {
    if (start.current === null) start.current = clock.getElapsedTime();
    const t = clock.getElapsedTime() - start.current;
    for (let i = 0; i < boxes.length; i++) {
      const m = refs.current[i];
      if (!m) continue;
      const b = boxes[i]!;
      let s = (t - b.delay) / DUR;
      s = s <= 0 ? 0.0001 : s >= 1 ? 1 : easeOut(s);
      m.scale.y = s;
      m.position.y = b.baseY + (BOXH * s) / 2;
    }
  });

  return (
    <group>
      {/* calendar floor */}
      {tiles.map((t, i) => (
        <mesh key={`t${i}`} position={[t.x, 0.03, t.z]}>
          <boxGeometry args={[0.9, 0.06, 0.9]} />
          <meshStandardMaterial color={t.lit ? "#273049" : "#1b1f2b"} roughness={0.9} />
        </mesh>
      ))}
      {boxes.map((b, i) => (
        <mesh
          key={i}
          ref={(el) => { refs.current[i] = el; }}
          position={[b.x, b.baseY, b.z]}
          onClick={(e) => { e.stopPropagation(); onDay(b.date); }}
          onPointerOver={(e) => { e.stopPropagation(); document.body.style.cursor = "pointer"; }}
          onPointerOut={() => { document.body.style.cursor = ""; }}
        >
          <boxGeometry args={[BOX, BOXH, BOX]} />
          <meshStandardMaterial color={b.color} roughness={0.55} metalness={0.1} />
        </mesh>
      ))}
    </group>
  );
}

export default function Heatmap3D({ grid, onNavigate }: { grid: HeatGrid; onNavigate: (r: HashRoute) => void }) {
  const dist = grid.weeks * 0.62 + 12;
  return (
    <Canvas
      orthographic
      camera={{ position: [dist, dist * 1.05, dist], zoom: Math.max(7, 460 / grid.weeks), near: 0.1, far: 3000 }}
      style={{ width: "100%", height: "100%" }}
    >
      <color attach="background" args={["#16171f"]} />
      <ambientLight intensity={0.62} />
      <directionalLight position={[12, 22, 8]} intensity={0.85} />
      <directionalLight position={[-10, 8, -12]} intensity={0.25} />
      <Stacks grid={grid} onDay={(date) => onNavigate({ view: "explore", explore: { filters: { day: date } } })} />
      <OrbitControls enablePan={false} target={[0, 2, 0]} minZoom={3} maxZoom={40} />
    </Canvas>
  );
}
