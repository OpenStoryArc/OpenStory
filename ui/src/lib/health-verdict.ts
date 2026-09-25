/**
 * health-verdict — one glance from /api/health (H-08).
 *
 * Pure: the health body in, a level and a list of findings out, ranked
 * worst first. The thresholds are the probe's
 * (scripts/node_health_probe.py): stream caps warn at 70% and go critical
 * at 90%; a watcher warns past 300 s and goes critical past 3600 s.
 */

export type HealthLevel = "ok" | "warn" | "critical";

export interface Verdict {
  readonly level: HealthLevel;
  readonly findings: readonly string[];
}

interface Stream {
  readonly name: string;
  readonly percent: number | null;
}
interface Consumer {
  readonly alive: boolean;
  /** B-05: `pending_start` while held until the node serves. */
  readonly state?: string;
  readonly restarts: number;
  readonly lag?: number;
  readonly last_restart?: string | null;
  readonly last_exit?: string | null;
}
interface Watcher {
  readonly actor: string;
  readonly age_secs: number | null;
  readonly publish_failures: number;
}

/** The subset of the health body the verdict reads; everything optional
 *  so an older node's body still yields a verdict. */
export interface HealthBody {
  readonly boot?: {
    readonly phase?: string;
    readonly replay?: { readonly done?: number; readonly total?: number; readonly elapsed_ms?: number };
  };
  readonly bus?: { readonly connected?: boolean };
  readonly leaf?: { readonly configured?: boolean; readonly connected?: boolean; readonly hub?: string | null };
  readonly streams?: readonly Stream[];
  readonly consumers?: Readonly<Record<string, Consumer>>;
  readonly watchers_detail?: readonly Watcher[];
}

const RANK: Record<HealthLevel, number> = { ok: 0, warn: 1, critical: 2 };

export function verdictFor(h: HealthBody): Verdict {
  const critical: string[] = [];
  const warn: string[] = [];

  if (h.bus?.connected === false) critical.push("bus is not connected");
  if (h.leaf?.configured && !h.leaf.connected) critical.push(`leaf to ${h.leaf.hub ?? "hub"} is not connected`);
  if (h.boot?.phase && h.boot.phase !== "serving") {
    const r = h.boot.replay;
    warn.push(r ? `replaying ${r.done ?? 0} of ${r.total ?? 0} sessions` : `node is ${h.boot.phase}`);
  }
  for (const s of h.streams ?? []) {
    if (s.percent == null) continue;
    const pct = Math.round(s.percent * 100);
    if (s.percent >= 0.9) critical.push(`stream ${s.name} at ${pct}% of its cap`);
    else if (s.percent >= 0.7) warn.push(`stream ${s.name} at ${pct}% of its cap`);
  }
  const serving = !h.boot?.phase || h.boot.phase === "serving";
  for (const [name, c] of Object.entries(h.consumers ?? {})) {
    if (c.state === "pending_start") {
      // Held until the node serves: not dead. Still pending once serving is worth a look.
      if (serving) warn.push(`consumer ${name} has not started yet`);
    } else if (!c.alive) critical.push(`consumer ${name} is not alive (${c.restarts} restarts)`);
    else if (c.restarts > 0) warn.push(`consumer ${name} restarted ${c.restarts} times`);
  }
  for (const w of h.watchers_detail ?? []) {
    if (w.age_secs != null) {
      if (w.age_secs > 3600) critical.push(`watcher ${w.actor} last saw an event ${w.age_secs} s ago`);
      else if (w.age_secs > 300) warn.push(`watcher ${w.actor} last saw an event ${w.age_secs} s ago`);
    }
    if (w.publish_failures > 0) warn.push(`watcher ${w.actor} has ${w.publish_failures} publish failures since boot`);
  }

  const findings = [...critical, ...warn];
  const level: HealthLevel = critical.length ? "critical" : warn.length ? "warn" : "ok";
  return { level, findings };
}

export function worse(a: HealthLevel, b: HealthLevel): HealthLevel {
  return RANK[a] >= RANK[b] ? a : b;
}
