/** The Event Storming board — OpenStory explaining its own architecture, live.
 *
 *  Interactive: pan (drag) + zoom (wheel) an infinite canvas; click a sticky to
 *  light its causes (upstream) and effects (downstream); pick a user journey to
 *  trace its path through the grammar. Everything is derived from the pure model
 *  in lib/event-storming.ts — the board IS the data. */

import { useMemo, useRef, useState, useCallback } from "react";
import {
  STICKIES, FLOWS, JOURNEYS, neighborsOf, journeyEdges, stickyById,
  type StickyKind, type StormContext,
} from "@/lib/event-storming";

// Event Storming's sticky grammar, harmonized to the app's palette.
const KIND: Record<StickyKind, { color: string; label: string }> = {
  event: { color: "#e0af68", label: "Domain event" },
  command: { color: "#7aa2f7", label: "Command" },
  aggregate: { color: "#d7c56a", label: "Aggregate" },
  actor: { color: "#9aa5ce", label: "Actor" },
  policy: { color: "#bb9af7", label: "Policy" },
  readmodel: { color: "#9ece6a", label: "Read model" },
  external: { color: "#f7768e", label: "External" },
};
const KIND_COL: Record<StickyKind, number> = { actor: 0, external: 0, command: 1, aggregate: 2, event: 3, policy: 4, readmodel: 5 };
const COL_X = [24, 224, 430, 628, 838, 1052];
const W = 168, H = 54, ROW = 68;
const BAND_Y: Record<StormContext, number> = { observed: 70, authored: 470 };

interface Pos { x: number; y: number }

export function EventStormBoard() {
  const [sel, setSel] = useState<string | null>(null);
  const [journey, setJourney] = useState<string | null>(null);
  const [view, setView] = useState({ tx: 0, ty: 0, s: 1 });
  const drag = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);

  // ── layout: kind → column, context → band, stack within a cell ──
  const pos = useMemo(() => {
    const m = new Map<string, Pos>();
    const counter = new Map<string, number>();
    for (const s of STICKIES) {
      const col = KIND_COL[s.kind];
      const key = `${col}:${s.context}`;
      const i = counter.get(key) ?? 0;
      counter.set(key, i + 1);
      m.set(s.id, { x: COL_X[col]!, y: BAND_Y[s.context] + i * ROW });
    }
    return m;
  }, []);
  const center = (id: string): Pos => { const p = pos.get(id)!; return { x: p.x + W / 2, y: p.y + H / 2 }; };

  // ── what's lit ──
  const lit = useMemo(() => {
    if (journey) {
      const j = JOURNEYS.find((x) => x.id === journey)!;
      return { nodes: new Set(j.path), edges: journeyEdges(j).map((e) => `${e.from}->${e.to}`) };
    }
    if (sel) {
      const n = neighborsOf(FLOWS, sel);
      const nodes = new Set([sel, ...n.upstream, ...n.downstream]);
      const edges = FLOWS.filter((f) => f.from === sel || f.to === sel).map((f) => `${f.from}->${f.to}`);
      return { nodes, edges: new Set(edges) as unknown as string[] };
    }
    return null;
  }, [sel, journey]);
  const nodeLit = (id: string) => !lit || lit.nodes.has(id);
  const edgeLit = (f: { from: string; to: string }) => {
    if (!lit) return false;
    const key = `${f.from}->${f.to}`;
    return Array.isArray(lit.edges) ? lit.edges.includes(key) : (lit.edges as Set<string>).has(key);
  };

  // ── pan + zoom (the canvas "physics": one transform over the scene) ──
  const onWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    setView((v) => {
      const ds = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const s = Math.min(2.2, Math.max(0.4, v.s * ds));
      // zoom toward the cursor: keep the point under the pointer fixed
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const px = e.clientX - rect.left, py = e.clientY - rect.top;
      return { s, tx: px - (px - v.tx) * (s / v.s), ty: py - (py - v.ty) * (s / v.s) };
    });
  }, []);
  const onDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("[data-sticky]")) return; // let sticky clicks through
    drag.current = { x: e.clientX, y: e.clientY, tx: view.tx, ty: view.ty };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    setView((v) => ({ ...v, tx: drag.current!.tx + (e.clientX - drag.current!.x), ty: drag.current!.ty + (e.clientY - drag.current!.y) }));
  };
  const onUp = () => { drag.current = null; };
  const reset = () => { setView({ tx: 0, ty: 0, s: 1 }); setSel(null); setJourney(null); };

  const selSticky = sel ? stickyById(sel) : null;
  const n = sel ? neighborsOf(FLOWS, sel) : null;

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-[#16161e]">
      {/* toolbar */}
      <div className="flex items-center gap-3 border-b border-[#2a2c3d] px-4 py-2 text-[12px]">
        <span className="font-semibold text-[#c0caf5]">Event Storming</span>
        <span className="text-[#565f89]">OpenStory, explaining itself — drag to pan, scroll to zoom, click a sticky.</span>
        <div className="ml-auto flex items-center gap-3">
          {Object.entries(KIND).map(([k, v]) => (
            <span key={k} className="flex items-center gap-1.5 text-[#a2acce]">
              <span className="inline-block h-2.5 w-2.5 rounded-sm" style={{ background: v.color }} />{v.label}
            </span>
          ))}
          <button onClick={reset} className="rounded border border-[#3b4261] px-2 py-0.5 text-[#7aa2f7] hover:bg-[#7aa2f710]">reset</button>
        </div>
      </div>

      <div className="flex flex-1 min-h-0">
        {/* canvas */}
        <div className="relative flex-1 overflow-hidden cursor-grab active:cursor-grabbing"
          onWheel={onWheel} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} data-testid="storm-canvas">
          <div className="absolute left-0 top-0" style={{ transform: `translate(${view.tx}px,${view.ty}px) scale(${view.s})`, transformOrigin: "0 0" }}>
            <svg width="1240" height="900" className="absolute left-0 top-0 pointer-events-none">
              {/* context band separator = the anti-corruption layer */}
              <line x1="0" y1="440" x2="1240" y2="440" stroke="#2a2c3d" strokeWidth="1.5" strokeDasharray="6 6" />
              <text x="12" y="58" className="fill-[#4a5178]" style={{ font: "600 11px ui-monospace", letterSpacing: ".14em" }}>OBSERVED · events.*  (read-only mirror)</text>
              <text x="12" y="462" className="fill-[#4a5178]" style={{ font: "600 11px ui-monospace", letterSpacing: ".14em" }}>AUTHORED · ui.*  (commands live here)</text>
              {FLOWS.map((f, i) => {
                const a = center(f.from), b = center(f.to);
                const on = edgeLit(f);
                const mx = (a.x + b.x) / 2;
                return (
                  <path key={i} d={`M ${a.x} ${a.y} C ${mx} ${a.y}, ${mx} ${b.y}, ${b.x} ${b.y}`}
                    fill="none" stroke={on ? "#7dcfff" : "#2f3348"}
                    strokeWidth={on ? 2.2 : 1.2} opacity={lit && !on ? 0.15 : on ? 0.95 : 0.5} />
                );
              })}
            </svg>
            {STICKIES.map((s) => {
              const p = pos.get(s.id)!, c = KIND[s.kind].color, on = nodeLit(s.id);
              const isSel = sel === s.id;
              return (
                <button key={s.id} data-sticky data-testid={`sticky-${s.id}`}
                  onClick={() => { setJourney(null); setSel(isSel ? null : s.id); }}
                  className="absolute rounded-md border text-left transition-opacity"
                  style={{
                    left: p.x, top: p.y, width: W, height: H, padding: "7px 9px",
                    background: isSel ? `${c}26` : "#1b1c28",
                    borderColor: c, boxShadow: isSel ? `0 0 0 2px ${c}, 0 6px 18px -8px ${c}` : "none",
                    opacity: on ? 1 : 0.22,
                  }}>
                  <div className="text-[9px] font-mono uppercase tracking-wide" style={{ color: c }}>{KIND[s.kind].label}</div>
                  <div className="truncate text-[13px] font-semibold text-[#c8d1f5]" title={s.label}>{s.label}</div>
                </button>
              );
            })}
          </div>
          <div className="absolute bottom-2 right-3 font-mono text-[10px] text-[#565f89]">{Math.round(view.s * 100)}%</div>
        </div>

        {/* side panel: journeys + selected detail */}
        <aside className="w-72 shrink-0 overflow-y-auto border-l border-[#2a2c3d] bg-[#1a1b26] p-3">
          <div className="mb-1 text-[10px] font-mono uppercase tracking-wide text-[#565f89]">User journeys · E2E requirements</div>
          <div className="mb-4 flex flex-col gap-1.5">
            {JOURNEYS.map((j) => (
              <button key={j.id} data-testid={`journey-${j.id}`}
                onClick={() => { setSel(null); setJourney(journey === j.id ? null : j.id); }}
                className={`rounded border px-2.5 py-2 text-left text-[12px] transition-colors ${
                  journey === j.id ? "border-[#7dcfff] bg-[#7dcfff12] text-[#c8d1f5]" : "border-[#2a2c3d] text-[#a2acce] hover:border-[#3b4261]"
                }`}>
                <div className="font-medium">{j.name}</div>
                {j.note && <div className="mt-0.5 text-[11px] text-[#565f89]">{j.note}</div>}
              </button>
            ))}
          </div>

          {selSticky ? (
            <div className="rounded border border-[#2a2c3d] p-3">
              <div className="text-[9px] font-mono uppercase tracking-wide" style={{ color: KIND[selSticky.kind].color }}>{KIND[selSticky.kind].label} · {selSticky.context}</div>
              <div className="mt-0.5 text-[15px] font-semibold text-[#c8d1f5]">{selSticky.label}</div>
              {selSticky.note && <p className="mt-2 text-[12.5px] leading-relaxed text-[#a2acce]">{selSticky.note}</p>}
              {n && (n.upstream.length > 0 || n.downstream.length > 0) && (
                <div className="mt-3 space-y-2 text-[11.5px]">
                  {n.upstream.length > 0 && <div><span className="text-[#565f89]">caused by → </span><span className="text-[#a2acce]">{n.upstream.map((id) => stickyById(id)?.label).join(", ")}</span></div>}
                  {n.downstream.length > 0 && <div><span className="text-[#565f89]">then → </span><span className="text-[#a2acce]">{n.downstream.map((id) => stickyById(id)?.label).join(", ")}</span></div>}
                </div>
              )}
            </div>
          ) : (
            <p className="text-[12px] leading-relaxed text-[#565f89]">Click a sticky to trace its causes and effects, or pick a journey to light its path. The dashed line is the <span className="text-[#a2acce]">anti-corruption layer</span> — observed data can't be commanded, only watched.</p>
          )}
        </aside>
      </div>
    </div>
  );
}
