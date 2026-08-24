/** Allowlist sanitizer for spotlight snapshots. What leaves the machine is
 *  exactly what was on screen — inert. Never regex-over-HTML. */

const XHTML_NAMESPACE = "http://www.w3.org/1999/xhtml";
const SVG_NAMESPACE = "http://www.w3.org/2000/svg";

// Always drop, regardless of namespace (case-insensitive due to foreign content)
// `template` is included because its content lives in `.content` (a
// DocumentFragment), not `.children` — the walk() below never descends into
// it, so anything inside (script, event-handler attrs) would otherwise pass
// through untouched. Inert under today's innerHTML player, but dropping the
// whole element keeps the sanitizer's guarantee true regardless of how the
// output is later consumed (e.g. a renderer that clones template content).
const ALWAYS_DROP = new Set(["script", "iframe", "object", "embed", "link", "meta", "style", "form", "input", "button", "audio", "video", "source", "foreignobject", "template"]);

// XHTML namespace elements to drop
const DROP_XHTML = new Set(["SCRIPT", "IFRAME", "OBJECT", "EMBED", "LINK", "META", "STYLE", "FORM", "INPUT", "BUTTON", "AUDIO", "VIDEO", "SOURCE"]);

// Safe SVG elements to allow
const SAFE_SVG = new Set(["svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "defs", "pattern", "use", "symbol", "marker", "linearGradient", "radialGradient", "stop"]);

const KEEP_ATTRS = new Set(["class", "style", "title", "alt", "colspan", "rowspan", "datetime", "aria-label", "role", "cx", "cy", "r", "x", "y", "x1", "y1", "x2", "y2", "width", "height", "d", "points", "viewBox", "preserveAspectRatio", "transform", "fill", "stroke", "stroke-width"]);

// Attributes that can contain funcIRI references (url()) and need cleaning
const FUNCIRI_ATTRS = new Set(["fill", "stroke", "clip-path", "filter", "mask", "marker-start", "marker-mid", "marker-end"]);

// Exported so other producers of baked-HTML content (e.g. reel-bundle.ts's
// stroke normalizer) can reuse the same funcIRI-cleaning rule instead of
// duplicating the regex.
export function cleanUrl(value: string): string {
  // drop any url(...) that is not a data: URI or same-document reference (#id)
  // Preserves: url(data:...), url(#ref), url('#ref'), url("#ref")
  // Removes: url(https://...), url(http://...), url(/path)
  return value.replace(/url\(\s*(['"]?)(?!data:)(?!#)[^)]*\1\s*\)/gi, "none");
}

export function sanitizeSnapshotHtml(html: string): string {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const walk = (el: Element): void => {
    for (const child of Array.from(el.children)) {
      const tagLower = child.tagName.toLowerCase();
      const ns = child.namespaceURI;

      // Always drop dangerous elements regardless of namespace
      if (ALWAYS_DROP.has(tagLower)) {
        child.remove();
        continue;
      }

      // For XHTML: use existing uppercase-keyed set
      if (ns === XHTML_NAMESPACE || ns === null) {
        if (DROP_XHTML.has(child.tagName)) {
          child.remove();
          continue;
        }
      }
      // For SVG: only allow explicitly-listed safe elements
      else if (ns === SVG_NAMESPACE) {
        if (!SAFE_SVG.has(tagLower)) {
          child.remove();
          continue;
        }
      }
      // Foreign content (MathML, other): default-deny
      else {
        child.remove();
        continue;
      }

      // Clean attributes
      for (const attr of Array.from(child.attributes)) {
        const name = attr.name.toLowerCase();
        if (name === "src") {
          if (!attr.value.trim().toLowerCase().startsWith("data:")) child.removeAttribute(attr.name);
          continue;
        }
        if (name === "href") {
          child.removeAttribute(attr.name); // snapshots are inert — no links out
          continue;
        }
        if (name === "style") {
          child.setAttribute("style", cleanUrl(attr.value));
          continue;
        }
        if (FUNCIRI_ATTRS.has(name)) {
          child.setAttribute(name, cleanUrl(attr.value));
          continue;
        }
        if (!KEEP_ATTRS.has(name)) child.removeAttribute(attr.name);
      }
      walk(child);
    }
  };
  walk(doc.body);
  return doc.body.innerHTML;
}
