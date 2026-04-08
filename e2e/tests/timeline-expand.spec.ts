// `timeline row expansion` retired — the inline `row-detail` expand/collapse
// pattern was removed from the Timeline. Row interaction will be re-specified
// during the stream architecture rewrite (see BACKLOG: Stream Architecture).
//
// What this test was originally validating is still a valuable property:
// when virtualized rows expand, the rows below should shift down in
// translateY space without rerendering the whole list. Worth re-asserting
// against whatever pattern the rewrite settles on.
export {};
