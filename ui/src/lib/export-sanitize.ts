/** Allowlist sanitizer for spotlight snapshots. What leaves the machine is
 *  exactly what was on screen — inert. Never regex-over-HTML. */

const DROP_ELEMENTS = new Set(["SCRIPT", "IFRAME", "OBJECT", "EMBED", "LINK", "META", "STYLE", "FORM", "INPUT", "BUTTON", "AUDIO", "VIDEO", "SOURCE"]);
const KEEP_ATTRS = new Set(["class", "style", "title", "alt", "colspan", "rowspan", "datetime", "aria-label", "role"]);

function cleanStyle(value: string): string {
  // drop any url(...) that is not a data: URI
  return value.replace(/url\(\s*(['"]?)(?!data:)[^)]*\1\s*\)/gi, "none");
}

export function sanitizeSnapshotHtml(html: string): string {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const walk = (el: Element): void => {
    for (const child of Array.from(el.children)) {
      if (DROP_ELEMENTS.has(child.tagName)) {
        child.remove();
        continue;
      }
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
          child.setAttribute("style", cleanStyle(attr.value));
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
