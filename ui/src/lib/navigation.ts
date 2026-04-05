/** Navigation types for Live/Explore tab switching and cross-linking. */

export type ViewMode = "live" | "explore" | "story";

/** Payload carried when cross-linking from Live → Explore. */
export interface CrossLink {
  readonly sessionId: string;
  readonly eventId?: string;
}

export type { HashRoute } from "@/lib/hash-route";
