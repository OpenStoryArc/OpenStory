/**
 * fleet-presence — the Fleet panel's rows from GET /api/fleet/presence (P-05).
 *
 * Pure: nodes in, rows out. Each node gets the health verdict of its last
 * beat's body, unless it has gone stale, which is critical on its own: a
 * node that stopped talking is the finding, whatever it last said. Self is
 * a flag on a row, read through the same path as every other node.
 */

import { verdictFor, type HealthBody, type HealthLevel } from "@/lib/health-verdict";

/** One node as `/api/fleet/presence` reports it. */
export interface PresenceNode {
  readonly host: string;
  readonly principal_id: string;
  readonly person_id: string | null;
  readonly time: string;
  readonly age_secs: number | null;
  readonly stale: boolean;
  readonly status: string | null;
  readonly git_sha: string | null;
  readonly version: string | null;
  readonly body: HealthBody;
}

/** The whole endpoint body. */
export interface FleetPresence {
  readonly interval_secs: number;
  readonly stale_after_secs: number;
  readonly nodes: readonly PresenceNode[];
}

export interface PresenceRow {
  readonly host: string;
  readonly principalId: string;
  readonly personId: string | null;
  readonly gitSha: string | null;
  readonly version: string | null;
  readonly time: string;
  readonly ageSecs: number | null;
  readonly ageLabel: string;
  readonly stale: boolean;
  readonly isSelf: boolean;
  readonly level: HealthLevel;
  readonly findings: readonly string[];
}

/** "5 s", "10 min", "2 h", "3 d"; null reads "never". */
export function ageText(ageSecs: number | null): string {
  if (ageSecs == null) return "never";
  if (ageSecs < 60) return `${ageSecs} s`;
  if (ageSecs < 3_600) return `${Math.floor(ageSecs / 60)} min`;
  if (ageSecs < 86_400) return `${Math.floor(ageSecs / 3_600)} h`;
  return `${Math.floor(ageSecs / 86_400)} d`;
}

export function presenceRows(nodes: readonly PresenceNode[], selfHost: string | null): PresenceRow[] {
  return nodes.map((n) => {
    const age = ageText(n.age_secs);
    const ageLabel = n.age_secs == null ? "never" : `${age} ago`;
    const verdict = verdictFor(n.body ?? {});
    const level: HealthLevel = n.stale ? "critical" : verdict.level;
    const findings = n.stale
      ? [`no beat for ${age} (stale past 3 intervals)`, ...verdict.findings]
      : verdict.findings;
    return {
      host: n.host,
      principalId: n.principal_id,
      personId: n.person_id,
      gitSha: n.git_sha,
      version: n.version,
      time: n.time,
      ageSecs: n.age_secs,
      ageLabel,
      stale: n.stale,
      isSelf: selfHost != null && n.host === selfHost,
      level,
      findings,
    };
  });
}
