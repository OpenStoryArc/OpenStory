/** Durable overlay annotations — notes an agent/person pins to a session. The
 *  overlay namespace (never the observed event stream). Fetched on load and
 *  appended live from `annotation_added` WS messages. */

export interface Annotation {
  readonly id: string;
  readonly session_id: string;
  readonly body: string;
  readonly issuer: string;
  readonly created_at: string;
}

/** Merge a new annotation into a list, de-duping by id, newest first. Pure. */
export function mergeAnnotation(list: readonly Annotation[], a: Annotation): Annotation[] {
  const without = list.filter((x) => x.id !== a.id);
  return [a, ...without];
}

/** Drop the annotation with `id` from a list. Pure — used for optimistic
 *  removal and for the `annotation_removed` live message. */
export function removeAnnotation(list: readonly Annotation[], id: string): Annotation[] {
  return list.filter((x) => x.id !== id);
}

/** Delete a durable annotation server-side (overlay is user-owned, so it can be
 *  removed). Best-effort; resolves true on success. */
export async function deleteAnnotation(id: string, baseUrl = ""): Promise<boolean> {
  try {
    const r = await fetch(`${baseUrl}/api/annotations/${encodeURIComponent(id)}`, { method: "DELETE" });
    return r.ok;
  } catch {
    return false;
  }
}

export async function fetchAnnotations(baseUrl = ""): Promise<Annotation[]> {
  try {
    const r = await fetch(`${baseUrl}/api/annotations`);
    const j = await r.json();
    return Array.isArray(j.annotations) ? (j.annotations as Annotation[]) : [];
  } catch {
    return [];
  }
}
