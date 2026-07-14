/** Heatmap3D — the isometric 3D stacks (lazy-loaded so three.js only ships when
 *  the mode opens). Each day is a column at (week, day); each BOX is a session
 *  (largest/warm at the base → smallest/cool on top; stackBoxes is the tested
 *  fold). Hover a box to see its session; click to open it. Tall days get an
 *  overflow cap — clicking the cap filters Explore to that day instead.
 *  Stacks rise on entrance, NEWEST week first, cascading back in time. */

import { useMemo, useRef, useState } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import type { Mesh } from "three";
import { stackBoxes, type HeatGrid, type StackBox } from "@/lib/heatmap";

const SP = 1.0;      // grid spacing
const BOX = 0.82;    // box footprint
const BOXH = 0.4;    // per-session height
const DUR = 0.55;    // rise duration (s)
const MAXH = 12;     // cap visible boxes; taller days get an overflow cap on top
// warm (largest, base) → cool (smallest, top)
const RAMP = ["#ff9e64", "#f7a86a", "#e0af68", "#f7c56a", "#d7d06a", "#9ece6a", "#73daca", "#5fc7d4", "#7dcfff", "#7aa2f7", "#9d86f0", "#bb9af7"];
const OVERFLOW = "#c0caf5";

interface Hover {
  box: StackBox;
  x: number;
  y: number;
}

function easeOut(x: number) { return 1 - Math.pow(1 - x, 3); }

function Stacks({ grid, onBox, onHover }: {
  grid: HeatGrid;
  onBox: (b: StackBox) => void;
  onHover: (h: Hover | null) => void;
}) {
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

  const boxes = useMemo(() => {
    const cx = (grid.weeks - 1) / 2;
    const maxWeek = grid.weeks - 1;
    return stackBoxes(grid, MAXH).map((b) => ({
      b,
      x: (b.week - cx) * SP,
      z: (b.day - 3) * SP,
      baseY: b.level * BOXH,
      color: b.overflow ? OVERFLOW : RAMP[Math.min(b.level, RAMP.length - 1)]!,
      delay: (maxWeek - b.week) * 0.04 + b.level * 0.03, // newest week first
    }));
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
      {boxes.map(({ b, x, z, baseY, color }, i) => (
        <mesh
          key={i}
          ref={(el) => { refs.current[i] = el; }}
          position={[x, baseY, z]}
          onClick={(e) => { e.stopPropagation(); onBox(b); }}
          onPointerOver={(e) => {
            e.stopPropagation();
            document.body.style.cursor = "pointer";
            onHover({ box: b, x: e.nativeEvent.clientX, y: e.nativeEvent.clientY });
          }}
          onPointerMove={(e) => {
            e.stopPropagation();
            onHover({ box: b, x: e.nativeEvent.clientX, y: e.nativeEvent.clientY });
          }}
          onPointerOut={() => { document.body.style.cursor = ""; onHover(null); }}
        >
          <boxGeometry args={[BOX, BOXH, BOX]} />
          <meshStandardMaterial color={color} roughness={0.55} metalness={0.1} />
        </mesh>
      ))}
    </group>
  );
}

export default function Heatmap3D({ grid, onOpenSession, onDayFilter }: {
  grid: HeatGrid;
  /** Box click → open that session. */
  onOpenSession: (id: string) => void;
  /** Overflow-cap click (no single session) → filter to the day. */
  onDayFilter: (date: string) => void;
}) {
  const dist = grid.weeks * 0.62 + 12;
  const [hover, setHover] = useState<Hover | null>(null);

  return (
    <div className="relative h-full w-full">
      <Canvas
        orthographic
        camera={{ position: [dist, dist * 1.05, dist], zoom: Math.max(7, 460 / grid.weeks), near: 0.1, far: 3000 }}
        style={{ width: "100%", height: "100%" }}
      >
        <color attach="background" args={["#16171f"]} />
        <ambientLight intensity={0.62} />
        <directionalLight position={[12, 22, 8]} intensity={0.85} />
        <directionalLight position={[-10, 8, -12]} intensity={0.25} />
        <Stacks
          grid={grid}
          onBox={(b) => (b.session ? onOpenSession(b.session.id) : onDayFilter(b.date))}
          onHover={setHover}
        />
        <OrbitControls enablePan={false} target={[0, 2, 0]} minZoom={3} maxZoom={40} />
      </Canvas>

      {/* session tooltip — DOM overlay so it stays crisp at any zoom */}
      {hover && (
        <div
          data-testid="heatmap-box-tooltip"
          className="pointer-events-none fixed z-50 max-w-[280px] rounded border border-[color:var(--border)] bg-[color:var(--bg)] px-2.5 py-1.5 text-[11px] shadow-xl"
          style={{ left: hover.x + 14, top: hover.y + 12 }}
        >
          {hover.box.session ? (
            <>
              <div className="truncate text-[color:var(--text)]">{hover.box.session.title}</div>
              <div className="mt-0.5 text-[10px] text-[color:var(--text-muted)]">
                {hover.box.date} · {hover.box.session.events.toLocaleString()} ev
                {hover.box.session.agent ? ` · ${hover.box.session.agent}` : ""} · click to open
              </div>
            </>
          ) : (
            <div className="text-[10px] text-[color:var(--text-muted)]">
              {hover.box.date} · +{hover.box.hidden} more sessions · click to filter the day
            </div>
          )}
        </div>
      )}
    </div>
  );
}
