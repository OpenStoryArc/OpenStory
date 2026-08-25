/**
 * ExportReelDialog — the export flow: collect → scan → preview → save.
 *
 * On mount: `collectBundle` (the one side-effectful step) → `scanBundle` →
 * `bakeReelHtml`. The preview IS the artifact — a sandboxed
 * `<iframe srcdoc>` renders exactly the standalone HTML file the user is
 * about to save, no separate preview renderer. A clean scan gets a single
 * "Save reel file" button; a scan with findings shows them grouped by slide
 * and swaps the primary action to "Export anyway", which re-bakes the same
 * bundle with `scan.acknowledged: true` before download — the export always
 * says, honestly, whether its gate was clean or consciously overridden.
 *
 * Read-only/mirror-not-a-leash: this dialog never mutates the reel or its
 * ink, it only reads them into a portable file.
 */

import { useEffect, useState } from "react";
import { collectBundle } from "@/lib/export-collect";
import { scanBundle, type Finding } from "@/lib/export-scan";
import { bakeReelHtml } from "@/lib/export-template";
import type { ReelBundle } from "@/lib/reel-bundle";

type DialogState =
  | { readonly phase: "collecting" }
  | { readonly phase: "error"; readonly message: string }
  | {
      readonly phase: "ready";
      readonly bundle: ReelBundle;
      readonly degraded: readonly string[];
      readonly findings: readonly Finding[];
      readonly html: string;
    };

function slug(title: string): string {
  const s = title
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s || "reel";
}

function downloadHtml(html: string, title: string): void {
  const blob = new Blob([html], { type: "text/html" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${slug(title)}.reel.html`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** Group findings by slide, preserving first-seen slide order. */
function groupBySlide(findings: readonly Finding[]): { slideId: string; items: Finding[] }[] {
  const order: string[] = [];
  const bySlide = new Map<string, Finding[]>();
  for (const f of findings) {
    if (!bySlide.has(f.slideId)) {
      bySlide.set(f.slideId, []);
      order.push(f.slideId);
    }
    bySlide.get(f.slideId)!.push(f);
  }
  return order.map((slideId) => ({ slideId, items: bySlide.get(slideId)! }));
}

export function ExportReelDialog({
  reelId,
  onClose,
}: {
  readonly reelId: string;
  readonly onClose: () => void;
}) {
  const [state, setState] = useState<DialogState>({ phase: "collecting" });

  useEffect(() => {
    let cancelled = false;
    setState({ phase: "collecting" });
    collectBundle(reelId)
      .then(({ bundle: collected, degraded }) => {
        if (cancelled) return;
        const findings = scanBundle(collected);
        // `buildBundle` always hands back `scan: {findings: 0, acknowledged:
        // false}` — it has no way to know the real count until AFTER
        // scanBundle runs on its own output. Fold the real count in before
        // baking, so the previewed iframe (footer + embedded #reel-bundle
        // JSON) reports the same receipt the findings panel is about to
        // show, from the moment findings are known — never "scan: clean"
        // while N findings sit beside it. `acknowledged` stays false here;
        // handleExportAnyway is the only place that flips it to true.
        const bundle: ReelBundle = {
          ...collected,
          scan: { v: 1, findings: findings.length, acknowledged: false },
        };
        const html = bakeReelHtml(bundle);
        setState({ phase: "ready", bundle, degraded, findings, html });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setState({
          phase: "error",
          message: err instanceof Error ? err.message : "Export failed.",
        });
      });
    return () => {
      cancelled = true;
    };
  }, [reelId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const handleSave = () => {
    if (state.phase !== "ready") return;
    downloadHtml(state.html, state.bundle.reel.title);
  };

  const handleExportAnyway = () => {
    if (state.phase !== "ready") return;
    const acknowledged: ReelBundle = {
      ...state.bundle,
      scan: { v: 1, findings: state.findings.length, acknowledged: true },
    };
    const html = bakeReelHtml(acknowledged);
    downloadHtml(html, acknowledged.reel.title);
  };

  const groups = state.phase === "ready" ? groupBySlide(state.findings) : [];

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Export reel"
      data-testid="export-reel-dialog"
    >
      <div
        className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)] text-[color:var(--text)] shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-[color:var(--divider)] px-5 py-3">
          <h2 className="text-sm font-semibold text-[color:var(--text)]">Export reel</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-xs text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
            aria-label="Close"
            data-testid="export-reel-close"
          >
            ✕
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {state.phase === "collecting" && (
            <p
              className="text-sm text-[color:var(--text-muted)]"
              data-testid="export-reel-collecting"
            >
              Collecting reel…
            </p>
          )}

          {state.phase === "error" && (
            <p className="text-sm text-red-400" data-testid="export-reel-error">
              {state.message}
            </p>
          )}

          {state.phase === "ready" && (
            <>
              <div
                className="mb-4 overflow-hidden rounded-lg border border-[color:var(--divider)]"
                data-testid="export-reel-preview"
              >
                <iframe
                  title="Reel preview"
                  srcDoc={state.html}
                  sandbox="allow-scripts"
                  className="h-[360px] w-full bg-black"
                  data-testid="export-reel-iframe"
                />
              </div>

              {state.degraded.length > 0 && (
                <div
                  className="mb-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-300"
                  data-testid="export-reel-degraded"
                >
                  <p className="font-medium">
                    Snapshot unavailable for {state.degraded.length} slide
                    {state.degraded.length === 1 ? "" : "s"}:
                  </p>
                  <ul className="mt-1 list-disc pl-4">
                    {state.degraded.map((id) => (
                      <li key={id}>{id}</li>
                    ))}
                  </ul>
                </div>
              )}

              {state.findings.length > 0 ? (
                <div
                  className="mb-4 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300"
                  data-testid="export-reel-findings"
                >
                  <p className="mb-2 font-medium">
                    {state.findings.length} finding{state.findings.length === 1 ? "" : "s"} in
                    this export:
                  </p>
                  {groups.map((g) => (
                    <div key={g.slideId} className="mb-2 last:mb-0">
                      <div className="font-mono text-[10px] text-red-300/70">{g.slideId}</div>
                      <ul className="space-y-1">
                        {g.items.map((f, i) => (
                          <li key={i} data-testid="export-reel-finding">
                            <span className="font-mono">{f.family}</span>
                            {" — "}
                            <span className="text-red-300/80">{f.excerpt}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </div>
              ) : (
                <p
                  className="mb-4 text-xs text-[color:var(--text-muted)]"
                  data-testid="export-reel-clean"
                >
                  Scan clean — no sensitive content found.
                </p>
              )}
            </>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[color:var(--divider)] px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-full border border-[color:var(--divider)] px-3 py-1.5 text-xs font-medium text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)]"
            data-testid="export-reel-cancel"
          >
            Cancel
          </button>
          {state.phase === "ready" &&
            (state.findings.length === 0 ? (
              <button
                type="button"
                onClick={handleSave}
                className="rounded-full bg-[color:var(--accent)] px-4 py-1.5 text-xs font-medium text-[color:var(--bg)] transition-opacity hover:opacity-90"
                data-testid="export-reel-primary"
              >
                Save reel file
              </button>
            ) : (
              <button
                type="button"
                onClick={handleExportAnyway}
                className="rounded-full bg-red-500 px-4 py-1.5 text-xs font-medium text-white transition-opacity hover:opacity-90"
                data-testid="export-reel-primary"
              >
                Export anyway
              </button>
            ))}
        </div>
      </div>
    </div>
  );
}
