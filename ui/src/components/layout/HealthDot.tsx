/**
 * HealthDot — the node's health at a glance in the header (H-08).
 *
 * Polls /api/health (a 503 during replay still carries a body), computes
 * the verdict with the pure `verdictFor`, and draws one dot: green ok,
 * amber warn, red critical. The title names the findings; a click opens a
 * panel with the findings and the raw JSON. Reads only; changes nothing.
 */

import { useCallback, useEffect, useState } from "react";
import { verdictFor, type HealthBody, type Verdict } from "@/lib/health-verdict";

const POLL_MS = 15_000;

const COLOR: Record<Verdict["level"], string> = {
  ok: "bg-green-400",
  warn: "bg-amber-400",
  critical: "bg-red-500",
};

interface Reading {
  readonly verdict: Verdict;
  readonly body: unknown;
}

async function readHealth(): Promise<Reading> {
  try {
    const resp = await fetch("/api/health");
    const body = (await resp.json()) as HealthBody;
    return { verdict: verdictFor(body), body };
  } catch (e) {
    const reason = e instanceof Error ? e.message : String(e);
    return {
      verdict: { level: "critical", findings: [`health endpoint unreachable: ${reason}`] },
      body: null,
    };
  }
}

export function HealthDot() {
  const [reading, setReading] = useState<Reading | null>(null);
  const [open, setOpen] = useState(false);

  const refresh = useCallback(() => {
    void readHealth().then(setReading);
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [refresh]);

  const level = reading?.verdict.level ?? "warn";
  const findings = reading?.verdict.findings ?? ["reading health…"];
  const title = findings.length ? `node health: ${level}. ${findings.join("; ")}` : `node health: ${level}`;

  return (
    <span className="relative flex items-center">
      <button
        type="button"
        data-testid="health-dot"
        data-level={level}
        title={title}
        aria-label={title}
        onClick={() => setOpen((o) => !o)}
        className={`h-2.5 w-2.5 rounded-full ${COLOR[level]} ring-2 ring-[color:var(--bg-surface)]`}
      />
      {open && (
        <div
          data-testid="health-panel"
          className="absolute right-0 top-5 z-50 w-[28rem] max-h-[70vh] overflow-auto rounded-lg border border-[color:var(--divider)] bg-[color:var(--bg-surface)] p-3 text-left text-xs text-[color:var(--text)] shadow-lg"
        >
          <div className="mb-2 flex items-center justify-between">
            <span className="font-medium">
              node health: {level}
            </span>
            <button
              type="button"
              className="text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
              onClick={() => setOpen(false)}
            >
              close
            </button>
          </div>
          {findings.length === 0 ? (
            <p className="text-[color:var(--text-muted)]">no findings</p>
          ) : (
            <ul className="mb-2 list-disc pl-4">
              {findings.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
          )}
          <pre className="whitespace-pre-wrap break-all font-mono text-[10px] text-[color:var(--text-muted)]">
            {JSON.stringify(reading?.body ?? null, null, 2)}
          </pre>
        </div>
      )}
    </span>
  );
}
