/** Shared syntax-highlighting theme.
 *
 *  Forks `vscDarkPlus` to recolor comments gray (the default is green,
 *  which clashes with our Tokyonight palette where green encodes
 *  thinking/successful results). Also exports the shared gutter style
 *  used by any SyntaxHighlighter that renders line numbers. */

import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";

/** Tokyonight muted gray — readable, no green cast. */
const COMMENT_GRAY = "#737aa2";

/** Gutter color — a notch darker than the comment color so line numbers
 *  sit behind the code visually. */
const LINE_NUMBER_GRAY = "#565f89";

type TokenStyle = { color?: string; [k: string]: unknown };
type Theme = Record<string, TokenStyle>;

const base = vscDarkPlus as Theme;

export const codeTheme: Theme = {
  ...base,
  comment: { ...base.comment, color: COMMENT_GRAY },
  prolog: { ...base.prolog, color: COMMENT_GRAY },
};

export const lineNumberStyle = {
  color: LINE_NUMBER_GRAY,
  minWidth: "2.25em",
  paddingRight: "0.75em",
};
