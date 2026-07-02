import { useCallback, useEffect, useRef, useState } from "react";
import { clampWidth } from "@/lib/resizable";

export interface ResizablePanel {
  /** Current width in px, always within [min, max]. */
  width: number;
  /** Whether a drag is in progress (for styling the handle). */
  dragging: boolean;
  /** Attach to a left-edge grab handle's onPointerDown. */
  onHandlePointerDown: (e: React.PointerEvent) => void;
}

/** A reusable resizable-panel hook: drag a left-edge handle to set the panel's
 *  width, clamped to [min, max] and persisted to localStorage under `storageKey`
 *  so it survives reloads. The panel grows as the handle moves LEFT (its left
 *  edge), so width = startWidth + (startX - currentX). Pure bounds logic lives in
 *  `clampWidth`; this hook owns only the DOM/side-effect edges. */
export function useResizablePanel(
  storageKey: string,
  defaultPx: number,
  min: number,
  max: number,
): ResizablePanel {
  const [width, setWidth] = useState<number>(() => {
    if (typeof window === "undefined") return defaultPx;
    const saved = Number(window.localStorage.getItem(storageKey));
    return clampWidth(Number.isFinite(saved) && saved > 0 ? saved : defaultPx, min, max);
  });
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);

  // Persist whenever the width settles.
  useEffect(() => {
    if (typeof window !== "undefined") window.localStorage.setItem(storageKey, String(width));
  }, [storageKey, width]);

  const onHandlePointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    drag.current = { startX: e.clientX, startWidth: width };
    setDragging(true);
  }, [width]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      // Left-edge handle: dragging left (smaller clientX) widens the panel.
      setWidth(clampWidth(d.startWidth + (d.startX - e.clientX), min, max));
    };
    const onUp = () => {
      drag.current = null;
      setDragging(false);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [dragging, min, max]);

  return { width, dragging, onHandlePointerDown };
}
