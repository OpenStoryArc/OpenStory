/** Inline image attachments for a message: thumbnails that expand to a lightbox. */

import { useState } from "react";
import type { ResolvedImage } from "@/lib/message-images";
import { Lightbox } from "@/components/ui/Lightbox";

export function MessageImages({ images }: { images: readonly ResolvedImage[] }) {
  const [zoom, setZoom] = useState<ResolvedImage | null>(null);
  if (images.length === 0) return null;

  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {images.map((img, i) => (
        <button
          key={i}
          type="button"
          onClick={() => setZoom(img)}
          className="block overflow-hidden rounded border border-[#2f3348] transition-colors hover:border-[#7aa2f7]"
          title="Click to expand"
        >
          <img
            src={img.src}
            alt="attachment"
            loading="lazy"
            className="max-h-64 max-w-full object-contain"
          />
        </button>
      ))}
      {zoom && <Lightbox src={zoom.src} onClose={() => setZoom(null)} />}
    </div>
  );
}
