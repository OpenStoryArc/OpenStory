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
}: {
  annotations: readonly Annotation[];
  onNavigate: (route: HashRoute) => void;
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
            <button
              key={a.id}
              data-annotation={a.id}
              onClick={() => onNavigate({ view: "explore", sessionId: a.session_id })}
              className="mb-1 block w-full rounded px-2 py-1.5 text-left hover:bg-[#24283b]"
            >
              <div className="text-[12px] leading-snug text-[#c0caf5]">{a.body}</div>
              <div className="mt-0.5 flex items-center gap-1.5 text-[10px] text-[#565f89]">
                <span className="text-[#e0af68]">{a.issuer}</span>
                <span>·</span>
                <span className="font-mono">{a.session_id.slice(0, 8)}</span>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
