import "@testing-library/jest-dom/vitest";

// jsdom doesn't implement layout methods that scroll-aware components call
// (Element.scrollIntoView, Element.scrollTo). Without these, any component that
// scrolls a highlighted row/card into view throws an unhandled error during
// tests even when the behaviour under test is unrelated. Stub them globally so
// specs exercise real components without each having to guard for jsdom.
if (typeof Element !== "undefined") {
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
  if (!Element.prototype.scrollTo) {
    Element.prototype.scrollTo = function scrollTo() {};
  }
}
