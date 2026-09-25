/** Typed fetch wrappers for /api/reels. */

import type { DrawStroke } from "@/lib/draw";
import type { ReelStopKind } from "@/lib/reel-visual";

/** Marginalia on one beat, as stored on the reel record (wire = file format). */
export interface ReelBeatInk {
  strokes: DrawStroke[];
  updatedAt: string;
}

export interface ReelVisual {
  kind?: string;
  sessionId?: string;
  labels?: string[];
  imageHref?: string;
  title?: string;
}

export interface ReelStop {
  /** Empty for title/diagram/image beats. */
  sessionId?: string;
  eventId?: string;
  line: string;
  clipAt?: string;
  /** Default spotlight when omitted. */
  kind?: ReelStopKind | string;
  visual?: ReelVisual;
}

export interface Reel {
  id: string;
  title: string;
  created: string;
  author: string;
  /** BLUF title card shown (and narrated) before stop 0. */
  opener?: string;
  closer?: string;
  stops: ReelStop[];
  /**
   * Ink per beat, keyed by slide index as a string (opener = "0", then
   * stops, then closer — the same index space BeatInkLayer draws in).
   * Absent when the reel has no ink.
   */
  beatInk?: Record<string, ReelBeatInk>;
}

export interface ReelMeta {
  id: string;
  title: string;
  created: string;
  author: string;
  stopCount: number;
}

export async function fetchReels(): Promise<ReelMeta[]> {
  const res = await fetch("/api/reels");
  if (!res.ok) return [];
  return (await res.json()) as ReelMeta[];
}

export async function fetchReel(id: string): Promise<Reel | null> {
  const res = await fetch(`/api/reels/${encodeURIComponent(id)}`);
  if (!res.ok) return null;
  return (await res.json()) as Reel;
}

/**
 * Replace the ink on one beat of a reel. Empty `strokes` forgets that beat.
 * Resolves false (never throws) when the server rejects or the network is
 * down — the caller keeps its local copy either way.
 */
export async function putReelBeatInk(
  reelId: string,
  beatIndex: number,
  strokes: readonly DrawStroke[],
  updatedAt: string,
): Promise<boolean> {
  try {
    const res = await fetch(`/api/reels/${encodeURIComponent(reelId)}/ink/${beatIndex}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ strokes, updatedAt }),
    });
    return res.ok;
  } catch {
    return false;
  }
}
