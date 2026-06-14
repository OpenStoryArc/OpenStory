/**
 * DataSourceNote — a small provenance line for an admin subsection: which
 * endpoint feeds it and how the data is derived, with a dot that flags the
 * determinism boundary at a glance:
 *
 *   local — a deterministic function of local state (store / config / roles DB)
 *   live  — a live-network read (best-effort; varies with the network)
 *   mixed — mostly local, with one live-probed element
 */

type SourceKind = "local" | "live" | "mixed";

const KIND: Record<SourceKind, { dot: string; label: string }> = {
  local: { dot: "#9ece6a", label: "deterministic · from local state" },
  live: { dot: "#e0af68", label: "live network · best-effort" },
  mixed: { dot: "#7aa2f7", label: "mostly local · one live probe" },
};

export function DataSourceNote({
  endpoint,
  derivation,
  kind,
}: {
  endpoint: string;
  derivation: string;
  kind: SourceKind;
}) {
  const k = KIND[kind];
  return (
    <div
      data-testid="data-source"
      className="mt-1.5 flex items-start gap-1.5 text-[11px] leading-relaxed text-[#565f89]"
    >
      <span
        aria-hidden
        className="mt-[5px] inline-block h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ backgroundColor: k.dot }}
      />
      <span>
        <code className="text-[#7dcfff]">{endpoint}</code>
        {" · "}
        <span className="uppercase tracking-wide">{k.label}</span>
        {" — "}
        {derivation}
      </span>
    </div>
  );
}
