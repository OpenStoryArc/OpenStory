/**
 * dora — the four keys as tiles (D-06), from the JSON `scripts/dora.py
 * --write` leaves in the node's data directory and GET /api/dora serves.
 * Pure: the file and a window in, four tiles out, in DORA order.
 */

export interface DoraWindow {
  readonly window_days: number;
  readonly deployments: number;
  readonly deployment_frequency_per_day: number;
  readonly lead_time_hours_median: number | null;
  readonly change_failure_rate: number;
  readonly time_to_restore_minutes_median: number | null;
  readonly open_criticals: number;
}

export interface DoraJson {
  readonly generated_at: string;
  readonly log?: string;
  readonly windows: Readonly<Record<string, DoraWindow>>;
}

export interface DoraTile {
  readonly key: "deployment_frequency" | "lead_time" | "change_failure_rate" | "time_to_restore";
  readonly label: string;
  readonly value: string;
  readonly unit: string;
  readonly note: string;
}

const NO_DATA = { value: "no data", unit: "" } as const;

function num(n: number, digits: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(digits).replace(/\.?0+$/, "");
}

/** Windows in ascending days. */
export function windowsOf(json: DoraJson): string[] {
  return Object.keys(json.windows ?? {}).sort((a, b) => Number(a) - Number(b));
}

export function tilesFor(json: DoraJson, window: string): DoraTile[] {
  const w = json.windows?.[window];
  if (!w) return [];
  const lead = w.lead_time_hours_median;
  const restore = w.time_to_restore_minutes_median;
  return [
    {
      key: "deployment_frequency",
      label: "Deployment frequency",
      value: num(w.deployment_frequency_per_day, 2),
      unit: "/ day",
      note: `${w.deployments} shas in ${w.window_days} d`,
    },
    {
      key: "lead_time",
      label: "Lead time",
      ...(lead == null ? NO_DATA : { value: num(lead, 1), unit: "h" }),
      note: "commit to first beat, median",
    },
    {
      key: "change_failure_rate",
      label: "Change failure rate",
      value: num(Math.round(w.change_failure_rate * 100), 0),
      unit: "%",
      note: "critical in the first hour",
    },
    {
      key: "time_to_restore",
      label: "Time to restore",
      ...(restore == null ? NO_DATA : { value: num(restore, 0), unit: "min" }),
      note: `critical to ok, median${w.open_criticals ? `; ${w.open_criticals} open` : ""}`,
    },
  ];
}
