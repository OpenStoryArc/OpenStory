/** Extract renderable images from message content blocks.
 *
 *  Transcript images arrive as ContentBlocks of `type: "image"` with a
 *  `source` of `{ type: "base64", media_type, data }` (Claude Code) or a URL.
 *  The conversation view historically filtered these out; these helpers turn
 *  them into `<img>`-ready sources instead.
 */

import type { ContentBlock } from "@/types/view-record";

export interface ResolvedImage {
  /** data: URI or remote URL, ready for an <img src>. */
  readonly src: string;
  readonly mediaType?: string;
}

interface ImageSource {
  type?: string;
  media_type?: string;
  data?: string;
  url?: string;
}

/** Resolve one content block to an image src, or null if it isn't a usable image. */
export function resolveImage(block: ContentBlock): ResolvedImage | null {
  if (block.type !== "image") return null;
  const s = block.source as ImageSource | undefined;
  if (!s) return null;
  if (s.data && (s.type === "base64" || s.media_type)) {
    return { src: `data:${s.media_type ?? "image/png"};base64,${s.data}`, mediaType: s.media_type };
  }
  if (s.url) return { src: s.url, mediaType: s.media_type };
  return null;
}

/** All resolvable images from a message's content (empty for string/text-only). */
export function imagesFromContent(content: string | ContentBlock[] | undefined): ResolvedImage[] {
  if (!content || typeof content === "string") return [];
  return content
    .map(resolveImage)
    .filter((img): img is ResolvedImage => img !== null);
}

/** Concatenated text of a message's content (mirrors prior filter behavior). */
export function textFromContent(content: string | ContentBlock[] | undefined): string {
  if (content == null) return "";
  if (typeof content === "string") return content;
  return content
    .filter((b) => b.type === "text")
    .map((b) => b.text ?? "")
    .join("");
}
