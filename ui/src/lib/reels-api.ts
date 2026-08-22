/** Typed fetch wrappers for /api/reels. */

import type { ReelStopKind } from "@/lib/reel-visual";

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
