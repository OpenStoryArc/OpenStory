/** Session detail view for the Explore tab — fetches and displays full session data from REST.
 *
 *  Kept deliberately quiet: a reader landing on Events should see ONE summary
 *  line, then be into the event cards below. The tool-count pills, Tool
 *  Journey, and Files list are real signal but not what you read FIRST — they
 *  fold behind quiet section controls (same idiom as Story's ▾ details) and
 *  stay folded by default, persisted per-browser.
 */

import { useState, useEffect } from "react";
import type { SessionSynopsis, FileImpact, SessionError } from "@/lib/session-detail";
import { deriveSynopsisMetrics } from "@/lib/session-detail";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { toolColor } from "@/lib/tool-colors";
import type { ToolStep } from "@/lib/tool-journey";
import { FileImpactTable } from "@/components/session/FileImpactTable";
import { ErrorList } from "@/components/session/ErrorList";
import { ToolJourney } from "@/components/session/ToolJourney";
import { usePersistedFlag } from "@/hooks/use-persisted-flag";

interface ExploreDetailProps {
  sessionId: string;
}

interface DetailData {
  synopsis: SessionSynopsis | null;
  files: readonly FileImpact[];
  errors: readonly SessionError[];
  toolSteps: readonly ToolStep[];
}

export function ExploreDetail({ sessionId }: ExploreDetailProps) {
  const [data, setData] = useState<DetailData | null>(null);
  const [loading, setLoading] = useState(true);
  const [journeyOpen, setJourneyOpen] = usePersistedFlag("os.events.journey", false);
  const [filesOpen, setFilesOpen] = usePersistedFlag("os.events.files", false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setData(null);

    Promise.all([
      fetch(`/api/sessions/${sessionId}/synopsis`).then((r) => r.json()).catch(() => null),
      fetch(`/api/sessions/${sessionId}/file-impact`).then((r) => r.json()).catch(() => []),
      fetch(`/api/sessions/${sessionId}/errors`).then((r) => r.json()).catch(() => []),
      fetch(`/api/sessions/${sessionId}/tool-journey`).then((r) => r.json()).catch(() => []),
    ]).then(([synopsis, files, errors, toolSteps]) => {
      if (!cancelled) {
        setData({ synopsis, files, errors, toolSteps });
        setLoading(false);
      }
    });

    return () => { cancelled = true; };
  }, [sessionId]);

  if (loading) {
    return <div className="p-4 text-xs text-[color:var(--text-muted)]">Loading session detail...</div>;
  }

  if (!data) {
    return <div className="p-4 text-xs text-[color:var(--text-muted)]">No data available</div>;
  }

  const m = data.synopsis ? deriveSynopsisMetrics(data.synopsis) : null;
  const synopsis = data.synopsis;

  return (
    <div className="space-y-2 p-4" data-testid="explore-detail">
      {/* The ONE line before the event cards — stats + project/branch, no big
          number blocks, no wall. */}
      {synopsis && m && (
        <div
          className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[length:var(--fs-label)] tabular-nums text-[color:var(--text-muted)]"
          data-testid="explore-summary-line"
        >
          <span><span className="text-[color:var(--text)]">{m.events}</span> events</span>
          <span>·</span>
          <span><span className="text-[color:var(--text)]">{m.tools}</span> tools</span>
          <span>·</span>
          <span>
            <span className={m.errors > 0 ? "text-[color:var(--red)]" : "text-[color:var(--text)]"}>{m.errors}</span> errors
          </span>
          <span>·</span>
          <span><span className="text-[color:var(--text)]">{m.duration}</span> duration</span>
          {(synopsis.project_name || synopsis.label) && <span>·</span>}
          {synopsis.project_name && <span>{synopsis.project_name}</span>}
          {synopsis.label && (
            <span className="text-[color:var(--text)]">{cleanHarnessPreview(synopsis.label)}</span>
          )}
        </div>
      )}

      {/* Tool-count pills + Tool Journey — folded by default so the wall of
          383 step-pills doesn't stand between the reader and the events. */}
      <FoldSection
        label="tool journey"
        countLabel={data.toolSteps.length > 0 ? `${data.toolSteps.length} steps` : undefined}
        open={journeyOpen}
        onToggle={() => setJourneyOpen(!journeyOpen)}
        testId="fold-tool-journey"
      >
        {synopsis && synopsis.top_tools.length > 0 && (
          <div className="flex flex-wrap gap-1.5 mb-2">
            {synopsis.top_tools.slice(0, 5).map((t) => (
              <span
                key={t.tool}
                className="text-[length:var(--fs-label)] px-2 py-0.5 rounded font-medium"
                style={{ color: toolColor(t.tool), backgroundColor: `${toolColor(t.tool)}18` }}
              >
                {t.tool} ({t.count})
              </span>
            ))}
          </div>
        )}
        <ToolJourney steps={data.toolSteps} />
      </FoldSection>

      {/* Files — folded by default, same reasoning. */}
      <FoldSection
        label="files"
        countLabel={data.files.length > 0 ? `${data.files.length}` : undefined}
        open={filesOpen}
        onToggle={() => setFilesOpen(!filesOpen)}
        testId="fold-files"
      >
        <FileImpactTable files={data.files} />
      </FoldSection>

      {/* Errors stay visible unfolded — they're the one thing worth a reader's
          immediate attention, and ErrorList already renders nothing when empty. */}
      <ErrorList errors={data.errors} />
    </div>
  );
}

function FoldSection({ label, countLabel, open, onToggle, testId, children }: {
  label: string;
  countLabel?: string;
  open: boolean;
  onToggle: () => void;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <div data-testid={testId}>
      <button
        type="button"
        onClick={onToggle}
        className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
        aria-expanded={open}
        title={open ? `Hide ${label}` : `Show ${label}`}
      >
        {open ? "▴" : "▾"} {label}
        {countLabel && <span className="opacity-70"> ({countLabel})</span>}
      </button>
      {open && <div className="mt-2">{children}</div>}
    </div>
  );
}
