/**
 * DoraTiles — the four DORA keys on the Admin tab (D-06), from GET /api/dora,
 * with the window selectable. The file is written by `scripts/dora.py
 * --write` from the node's presence history and git; the tiles only read.
 */

import { useEffect, useState } from "react";
import { tilesFor, windowsOf, type DoraJson } from "@/lib/dora";

type Reading =
  | { readonly kind: "loading" }
  | { readonly kind: "missing"; readonly hint: string }
  | { readonly kind: "error"; readonly reason: string }
  | { readonly kind: "ok"; readonly json: DoraJson };

async function readDora(): Promise<Reading> {
  try {
    const resp = await fetch("/api/dora");
    const body = (await resp.json()) as DoraJson & { error?: string };
    if (resp.status === 404) return { kind: "missing", hint: body.error ?? "run scripts/dora.py --write data/dora.json" };
    if (!resp.ok) return { kind: "error", reason: `HTTP ${resp.status}` };
    return { kind: "ok", json: body };
  } catch (e) {
    return { kind: "error", reason: e instanceof Error ? e.message : String(e) };
  }
}

export function DoraTiles() {
  const [reading, setReading] = useState<Reading>({ kind: "loading" });
  const [window, setWindow] = useState<string>("");

  useEffect(() => {
    void readDora().then((r) => {
      setReading(r);
      if (r.kind === "ok") setWindow((w) => w || windowsOf(r.json)[0] || "");
    });
  }, []);

  return (
    <div data-testid="dora-tiles">
      {reading.kind === "loading" && <p className="text-xs text-[color:var(--text-muted)]">Reading DORA…</p>}
      {reading.kind === "missing" && (
        <p className="text-xs text-[color:var(--text-muted)]">
          No DORA file yet: <code>{reading.hint}</code>
        </p>
      )}
      {reading.kind === "error" && <p className="text-xs text-red-500">DORA unavailable: {reading.reason}</p>}
      {reading.kind === "ok" && (
        <>
          <div className="mb-3 flex items-center gap-3 text-xs text-[color:var(--text-muted)]">
            <label className="flex items-center gap-1">
              window
              <select
                data-testid="dora-window"
                value={window}
                onChange={(e) => setWindow(e.target.value)}
                className="rounded border border-[color:var(--bg-surface)] bg-[color:var(--bg)] px-1 py-0.5 text-[color:var(--text)]"
              >
                {windowsOf(reading.json).map((w) => (
                  <option key={w} value={w}>
                    {w} d
                  </option>
                ))}
              </select>
            </label>
            <span>generated {reading.json.generated_at}</span>
          </div>
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            {tilesFor(reading.json, window).map((t) => (
              <div
                key={t.key}
                data-testid={`dora-tile-${t.key}`}
                className="rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-3"
              >
                <div className="text-xs text-[color:var(--text-muted)]">{t.label}</div>
                <div className="mt-1 flex items-baseline gap-1">
                  <span className="text-2xl font-semibold text-[color:var(--text)]">{t.value}</span>
                  {t.unit && <span className="text-xs text-[color:var(--text-muted)]">{t.unit}</span>}
                </div>
                <div className="mt-1 text-[10px] text-[color:var(--text-muted)]">{t.note}</div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
