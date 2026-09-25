/**
 * fleet-map — the Fleet tab's rows from GET /api/fleet/presence and
 * GET /api/consistency (C-07).
 *
 * Pure: two bodies in, one row per host out. Each host gets the age of its
 * last beat, its build, the verdict its beat carried (or the one computed
 * from the body on an older build), and the consistency findings this node
 * raised against it. A finding whose id ends in `:<host>` belongs to that
 * host (diverged, behind, stale_snapshot); every other finding (lag,
 * unverified) is about this node and sits on the self row. The level is
 * the worst of the beat's verdict, a stale beat, and the host's findings,
 * so the dot says what the report says.
 */

import {
  ageText,
  type FleetPresence,
  type PresenceNode,
} from "@/lib/fleet-presence";
import { verdictFor, worse, type HealthLevel } from "@/lib/health-verdict";

/** One finding in the verdict's shape, as /api/health and /api/consistency serve it. */
export interface Finding {
  readonly id: string;
  readonly level: HealthLevel;
  readonly text: string;
}

export interface ConsistencyPeer {
  readonly host: string;
  readonly compared: boolean;
  readonly differing_projects: number;
  readonly stale: boolean;
  readonly age_secs?: number | null;
}

/** The whole GET /api/consistency body. */
export interface ConsistencyReport {
  readonly host: string;
  readonly level: HealthLevel;
  readonly findings: readonly Finding[];
  readonly peers: readonly ConsistencyPeer[];
}

export interface FleetMapRow {
  readonly host: string;
  readonly principalId: string;
  readonly isSelf: boolean;
  readonly stale: boolean;
  readonly ageSecs: number | null;
  readonly ageLabel: string;
  readonly build: string | null;
  readonly level: HealthLevel;
  /** The beat's own verdict texts. */
  readonly verdict: readonly string[];
  /** The consistency findings against this host (or about self). */
  readonly consistency: readonly Finding[];
  /** Whether the report compared this host's roll-up (self: always true). */
  readonly compared: boolean;
  readonly differingProjects: number;
}

const LEVELS: ReadonlySet<string> = new Set(["ok", "warn", "critical"]);

function asLevel(raw: unknown): HealthLevel {
  return typeof raw === "string" && LEVELS.has(raw)
    ? (raw as HealthLevel)
    : "ok";
}

/** The findings that belong to `host`: host-suffixed ids for a peer, the
 *  unsuffixed rest for self. */
export function findingsFor(
  findings: readonly Finding[],
  host: string,
  isSelf: boolean,
): Finding[] {
  return findings.filter((f) => {
    const at = f.id.indexOf(":");
    if (at < 0) return isSelf;
    const suffix = f.id.slice(at + 1);
    const prefix = f.id.slice(0, at);
    // lag:<consumer> names a consumer, not a host: about self.
    if (prefix === "lag") return isSelf;
    return suffix === host;
  });
}

/** "aaa111 (0.9.0)", "aaa111", "0.9.0", or null when the beat says nothing. */
export function buildLabel(
  gitSha: string | null,
  version: string | null,
): string | null {
  if (gitSha && version) return `${gitSha} (${version})`;
  return gitSha ?? version ?? null;
}

/** The verdict a beat carried, or the one its body yields on an older build. */
function beatVerdict(node: PresenceNode): {
  level: HealthLevel;
  texts: string[];
} {
  const carried = node.body?.verdict;
  if (carried && Array.isArray(carried.findings)) {
    return {
      level: asLevel(carried.level),
      texts: carried.findings.map((f) => f.text),
    };
  }
  const v = verdictFor(node.body ?? {});
  return { level: v.level, texts: [...v.findings] };
}

export function fleetMapRows(
  presence: FleetPresence,
  report: ConsistencyReport,
): FleetMapRow[] {
  const peers = new Map(report.peers.map((p) => [p.host, p]));
  return presence.nodes.map((n) => {
    const isSelf = n.host === report.host;
    const beat = beatVerdict(n);
    const consistency = findingsFor(report.findings, n.host, isSelf);
    const peer = peers.get(n.host);
    let level: HealthLevel = beat.level;
    if (n.stale) level = "critical";
    for (const f of consistency) level = worse(level, f.level);
    return {
      host: n.host,
      principalId: n.principal_id,
      isSelf,
      stale: n.stale,
      ageSecs: n.age_secs,
      ageLabel: n.age_secs == null ? "never" : `${ageText(n.age_secs)} ago`,
      build: buildLabel(n.git_sha, n.version),
      level,
      verdict: beat.texts,
      consistency,
      compared: isSelf ? true : (peer?.compared ?? false),
      differingProjects: isSelf ? 0 : (peer?.differing_projects ?? 0),
    };
  });
}
