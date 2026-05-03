/**
 * Local-info client — what `OPEN_STORY_HOST` / `OPEN_STORY_USER`
 * resolved to inside this OpenStory process.
 *
 * The Live tab's session header uses this to mark sessions whose
 * `user` differs from the local resolver's value as "Replicated from
 * another machine" — distinguishing my work from work that arrived
 * via the NATS leaf.
 *
 * Both fields are always strings (the resolver falls back to
 * `"unknown"` rather than null), so the response shape is stable.
 */

export interface LocalInfo {
  readonly host: string;
  readonly user: string;
}

/** Fetch the resolved local host/user pair. Throws on network error. */
export async function fetchLocalInfo(baseUrl: string = ""): Promise<LocalInfo> {
  const res = await fetch(`${baseUrl}/api/local-info`);
  if (!res.ok) {
    throw new Error(`Failed to fetch local info: ${res.status} ${res.statusText}`);
  }
  return res.json();
}
