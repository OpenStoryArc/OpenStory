/** Typed fetch wrappers for /api/reels. */

export interface ReelStop {
  sessionId: string;
  eventId: string;
  line: string;
  clipAt?: string;
}

export interface Reel {
  id: string;
  title: string;
  created: string;
  author: string;
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
