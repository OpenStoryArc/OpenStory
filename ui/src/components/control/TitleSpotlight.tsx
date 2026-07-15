/** TitleSpotlight — a full-screen title card: the MESSAGE fills the screen,
 *  everything else dims. The words-only sibling of EventSpotlight, for demo
 *  openers and closers, driven through the control seam
 *  (`present { message, spotlight: true }`). Dismissed by Esc, a backdrop
 *  click, `toggle {target:"spotlight", value:"off"}`, or any subsequent
 *  view-changing control action (App owns those seams). */

import { useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "./markdown";

const MD_LARGE = markdownComponents(16);

export function TitleSpotlight({ message, onClose }: { message: string; onClose: () => void }) {
  // Esc dismisses — the human can always take the wheel back.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="spotlight-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Title card"
      data-testid="title-spotlight"
    >
      {/* No card chrome — the words ARE the shot. Large, centered, balanced. */}
      <div
        className="spotlight-card mx-6 max-w-4xl text-center text-[color:var(--stream-text,#e6dac2)] [text-wrap:balance]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="prose prose-2xl max-w-none font-semibold leading-snug text-inherit prose-headings:text-inherit prose-strong:text-inherit prose-em:text-inherit">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_LARGE}>
            {message}
          </ReactMarkdown>
        </div>
      </div>
    </div>
  );
}
