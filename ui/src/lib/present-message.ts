/** Pure classifier for the "present" banner's message. Decides whether the
 *  message is "rich" — multi-line, contains a fenced code block, or is longer
 *  than fits comfortably on one banner line — and therefore should be rendered
 *  as expandable markdown rather than a single clipped line. Keeps the banner's
 *  layout decision testable and out of the component. */

/** One line of banner text comfortably holds ~140 chars before it wants room. */
const ONE_LINE_BUDGET = 140;

export function isRichMessage(text: string): boolean {
  const t = (text ?? "").trim();
  if (!t) return false;
  if (t.includes("\n")) return true;
  if (t.includes("```")) return true;
  return t.length > ONE_LINE_BUDGET;
}
