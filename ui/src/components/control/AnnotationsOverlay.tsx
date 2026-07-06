/** AnnotationsOverlay — renders the durable overlay notes an agent/person has
 *  pinned to sessions. A collapsible corner panel; each note shows its author,
 *  target session, and body, and clicks through to that session in Explore.
 *  Pure sink of the annotations list held in App (fetched on load + appended
 *  live from `annotation_added`). Overlay namespace — shows notes, never touches
 *  the observed sources. */

import { useState } from "react";
import type { HashRoute } from "@/lib/hash-route";
import type { Annotation } from "@/lib/annotations";

export function AnnotationsOverlay({
  annotations,
  onNavigate,
  onRemove,
}: {
  annotations: readonly Annotation[];
  onNavigate: (route: HashRoute) => void;
  onRemove?: (id: string) => void;
}) {
  const [open, setOpen] = useState(true);
  if (annotations.length === 0) return null;

  return (
    <div className="fixed bottom-3 right-3 z-40 w-72 rounded-lg border border-[#e0af68]/40 bg-[#1a1b26]/95 shadow-xl" data-testid="annotations-overlay">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded-t-lg px-3 py-2 text-left text-[12px] font-semibold text-[#e0af68]"
      >
        <span>📌</span> Notes
        <span className="rounded bg-[#e0af68]/20 px-1.5 text-[10px]">{annotations.length}</span>
        <span className="ml-auto text-[#565f89]">{open ? "▾" : "▸"}</span>
      </button>
      {open && (
        <div className="max-h-[40vh] overflow-y-auto border-t border-[#2f3348] px-2 py-1.5">
          {annotations.slice(0, 30).map((a) => (
            <div key={a.id} data-annotation={a.id} className="group relative mb-1 rounded hover:bg-[#24283b]">
              <button
                onClick={() => onNavigate({ view: "explore", sessionId: a.session_id })}
                className="block w-full rounded px-2 py-1.5 pr-6 text-left"
              >
                <div className="text-[12px] leading-snug text-[#c0caf5]">{a.body}</div>
                <div className="mt-0.5 flex items-center gap-1.5 text-[10px] text-[#565f89]">
                  <span className="text-[#e0af68]">{a.issuer}</span>
                  <span>·</span>
                  <span className="font-mono">{a.session_id.slice(0, 8)}</span>
                </div>
              </button>
              {onRemove && (
                <button
                  data-remove-annotation={a.id}
                  onClick={(e) => { e.stopPropagation(); onRemove(a.id); }}
                  title="Remove note"
                  aria-label="Remove note"
                  className="absolute right-1 top-1 rounded px-1 text-[13px] leading-none text-[#565f89] opacity-0 transition-opacity hover:text-[#f7768e] group-hover:opacity-100"
                >
                  ×
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
