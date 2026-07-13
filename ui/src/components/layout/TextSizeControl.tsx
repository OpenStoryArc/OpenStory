/** Global text-size control.
 *
 *  The app sizes text with hardcoded pixel values scattered across many
 *  components, so a root `font-size` change would do nothing. Instead we scale
 *  the whole document via CSS `zoom` — which scales text, icons, and spacing
 *  uniformly regardless of unit — and persist the choice. Applied to
 *  `document.documentElement` (page-zoom semantics) so full-height (`h-screen`)
 *  layouts continue to fill the viewport.
 */

import { useEffect, useState } from "react";

const KEY = "os.textScale";

const LEVELS: { label: string; scale: number; title: string }[] = [
  { label: "S", scale: 0.9, title: "Smaller" },
  { label: "M", scale: 1.0, title: "Default text size" },
  { label: "L", scale: 1.15, title: "Larger" },
  { label: "XL", scale: 1.3, title: "Largest" },
];

function readScale(): number {
  try {
    const s = Number(localStorage.getItem(KEY));
    return Number.isFinite(s) && s > 0 ? s : 1.0;
  } catch {
    return 1.0;
  }
}

function applyScale(scale: number) {
  // Non-standard but supported in Chromium/Safari (the target browsers here);
  // scales the whole page uniformly, which is what we want given px-based sizes.
  (document.documentElement.style as CSSStyleDeclaration & { zoom: string }).zoom =
    String(scale);
}

export function TextSizeControl() {
  const [scale, setScale] = useState<number>(readScale);

  // Apply on mount and whenever the choice changes.
  useEffect(() => {
    applyScale(scale);
  }, [scale]);

  const pick = (s: number) => {
    setScale(s);
    try {
      localStorage.setItem(KEY, String(s));
    } catch {
      /* ignore quota/private-mode */
    }
  };

  return (
    <div
      className="flex items-center overflow-hidden rounded border border-[#2f3348]"
      role="group"
      aria-label="Text size"
      title="Text size"
    >
      {LEVELS.map(({ label, scale: s, title }) => {
        const active = Math.abs(scale - s) < 0.001;
        return (
          <button
            key={label}
            type="button"
            onClick={() => pick(s)}
            title={title}
            aria-pressed={active}
            className={`px-1.5 py-0.5 text-[10px] leading-none transition-colors ${
              active
                ? "bg-[#7aa2f7] font-medium text-[#1a1b26]"
                : "text-[#565f89] hover:bg-[#24283b] hover:text-[#c0caf5]"
            }`}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}
