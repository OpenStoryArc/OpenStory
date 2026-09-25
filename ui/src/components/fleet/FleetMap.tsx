/**
 * FleetMap — the Fleet tab (C-07): each host's beat age, build, verdict,
 * and the consistency findings against this node, from
 * GET /api/fleet/presence and GET /api/consistency. Polls both every 15 s.
 * Reads only; the map steers nothing.
 */

import { useCallback, useEffect, useState } from "react";
import {
  fleetMapRows,
  type ConsistencyReport,
  type FleetMapRow,
} from "@/lib/fleet-map";
import type { FleetPresence } from "@/lib/fleet-presence";
import type { HealthLevel } from "@/lib/health-verdict";

const POLL_MS = 15_000;

const COLOR: Record<HealthLevel, string> = {
  ok: "bg-green-400",
  warn: "bg-amber-400",
  critical: "bg-red-500",
};

const TEXT: Record<HealthLevel, string> = {
  ok: "text-green-500",
  warn: "text-amber-500",
  critical: "text-red-500",
};

type Reading =
  | { readonly kind: "loading" }
  | { readonly kind: "error"; readonly reason: string }
  | {
      readonly kind: "ok";
      readonly intervalSecs: number;
      readonly level: HealthLevel;
      readonly selfHost: string;
      readonly rows: readonly FleetMapRow[];
    };

async function readJson<T>(url: string): Promise<T> {
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`${url}: HTTP ${resp.status}`);
  return (await resp.json()) as T;
}

async function readMap(): Promise<Reading> {
  try {
    const [presence, report] = await Promise.all([
      readJson<FleetPresence>("/api/fleet/presence"),
      readJson<ConsistencyReport>("/api/consistency"),
    ]);
    return {
      kind: "ok",
      intervalSecs: presence.interval_secs,
      level: report.level,
      selfHost: report.host,
      rows: fleetMapRows({ ...presence, nodes: presence.nodes ?? [] }, report),
    };
  } catch (e) {
    return {
      kind: "error",
      reason: e instanceof Error ? e.message : String(e),
    };
  }
}

function Badge({
  children,
  tone,
}: {
  readonly children: string;
  readonly tone: "accent" | "red";
}) {
  const cls =
    tone === "accent"
      ? "bg-[color:var(--accent)]/20 text-[color:var(--accent)]"
      : "bg-red-500/15 text-red-500";
  return (
    <span
      className={`rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wider ${cls}`}
    >
      {children}
    </span>
  );
}

export function FleetMap() {
  const [reading, setReading] = useState<Reading>({ kind: "loading" });

  const refresh = useCallback(() => {
    void readMap().then(setReading);
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [refresh]);

  return (
    <div data-testid="fleet-map" className="flex-1 min-h-0 overflow-y-auto p-6">
      <header className="mb-4">
        <h2 className="text-xl font-semibold text-[color:var(--text)] mb-1">
          Fleet
        </h2>
        <p className="text-xs text-[color:var(--text-muted)]">
          Every node's last beat, and whether it agrees with this one: from{" "}
          <code>/api/fleet/presence</code> and <code>/api/consistency</code>.
        </p>
      </header>

      {reading.kind === "loading" && (
        <p className="text-sm text-[color:var(--text-muted)]">
          Reading the fleet…
        </p>
      )}
      {reading.kind === "error" && (
        <p className="text-sm text-red-500">
          Fleet unavailable: {reading.reason}
        </p>
      )}
      {reading.kind === "ok" && (
        <>
          <p className="mb-3 text-xs text-[color:var(--text-muted)]">
            Consistency from <code>{reading.selfHost}</code>:{" "}
            <span
              data-testid="fleet-level"
              className={`font-semibold ${TEXT[reading.level]}`}
            >
              {reading.level}
            </span>
            . Beats every {reading.intervalSecs} s.
          </p>
          {reading.rows.length === 0 ? (
            <p className="text-sm text-[color:var(--text-muted)]">
              No presence yet.
            </p>
          ) : (
            <ul className="divide-y divide-[color:var(--bg-surface)] rounded-lg border border-[color:var(--bg-surface)]">
              {reading.rows.map((r) => (
                <li
                  key={`${r.host}/${r.principalId}`}
                  data-testid={`fleet-host-${r.host}`}
                  className="px-3 py-2 text-xs"
                >
                  <div className="flex items-center gap-3">
                    <span
                      data-testid="fleet-dot"
                      data-level={r.level}
                      title={r.level}
                      className={`h-2.5 w-2.5 shrink-0 rounded-full ${COLOR[r.level]}`}
                    />
                    <code
                      className="font-semibold text-[color:var(--text)]"
                      title={r.principalId}
                    >
                      {r.host}
                    </code>
                    {r.isSelf && <Badge tone="accent">self</Badge>}
                    {r.stale && <Badge tone="red">stale</Badge>}
                    {!r.isSelf && !r.compared && (
                      <span className="text-[color:var(--text-muted)]">
                        not compared
                      </span>
                    )}
                    <span className="ml-auto text-[color:var(--text-muted)]">
                      {r.ageLabel}
                    </span>
                    {r.build && (
                      <code className="text-[color:var(--text-muted)]">
                        {r.build}
                      </code>
                    )}
                  </div>
                  {(r.verdict.length > 0 || r.consistency.length > 0) && (
                    <ul className="mt-1 ml-5 space-y-0.5 text-[color:var(--text-muted)]">
                      {r.verdict.map((text) => (
                        <li key={`v:${text}`} data-testid="fleet-verdict">
                          {text}
                        </li>
                      ))}
                      {r.consistency.map((f) => (
                        <li
                          key={f.id}
                          data-testid="fleet-finding"
                          className={TEXT[f.level]}
                        >
                          <code>{f.id}</code> {f.text}
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
