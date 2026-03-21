/** Truncation helpers for payload display.
 *
 * Pure functions — no React dependencies. */

/** Format bytes as human-readable size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Build a truncation label showing how much content is hidden.
 *  @param payloadBytes — total payload size in bytes
 *  @param displayedChars — number of characters currently shown */
export function truncationLabel(payloadBytes: number, displayedChars: number): string {
  const hiddenBytes = Math.max(0, payloadBytes - displayedChars);
  return `${formatBytes(hiddenBytes)} hidden — showing ${formatBytes(displayedChars)} of ${formatBytes(payloadBytes)}`;
}

/** Build the REST API URL for fetching full event content. */
export function contentApiUrl(sessionId: string, eventId: string): string {
  return `/api/sessions/${sessionId}/events/${eventId}/content`;
}

/** Copy text to clipboard. Returns true on success. */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
