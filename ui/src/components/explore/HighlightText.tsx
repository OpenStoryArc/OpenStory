/** Renders text with highlighted search matches. */

import { highlightMatch } from "@/lib/explore-search";

interface HighlightTextProps {
  text: string;
  query: string;
}

export function HighlightText({ text, query }: HighlightTextProps) {
  const segments = highlightMatch(text, query);

  return (
    <>
      {segments.map((seg, i) =>
        seg.isMatch ? (
          <mark
            key={i}
            className="bg-[color:var(--orange)]/19 text-[color:var(--orange)] rounded-sm px-0.5"
          >
            {seg.text}
          </mark>
        ) : (
          <span key={i}>{seg.text}</span>
        ),
      )}
    </>
  );
}
