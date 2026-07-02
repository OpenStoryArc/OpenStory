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

export async function fetchAnnotations(baseUrl = ""): Promise<Annotation[]> {
  try {
    const r = await fetch(`${baseUrl}/api/annotations`);
    const j = await r.json();
    return Array.isArray(j.annotations) ? (j.annotations as Annotation[]) : [];
  } catch {
    return [];
  }
}
