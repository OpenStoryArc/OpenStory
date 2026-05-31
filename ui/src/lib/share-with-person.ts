/**
 * POST /api/admin/share-with-person — record consent for `personId` to
 * receive `sessionId`'s events via per-account NATS export/import.
 *
 * Returns nothing on success (server returns 204). Throws on HTTP error
 * with the status + body text so the caller can surface the reason to
 * the operator (404 = unknown session, 409 = no person_id stamp, 503 =
 * multi-account NATS not configured on this node).
 */
export async function shareSessionWithPerson(
  sessionId: string,
  personId: string,
  baseUrl: string = "",
): Promise<void> {
  const res = await fetch(`${baseUrl}/api/admin/share-with-person`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session_id: sessionId, person_id: personId }),
  });
  if (res.status === 204) return;
  const text = await res.text().catch(() => "");
  throw new Error(`share-with-person failed: HTTP ${res.status} ${text}`);
}
