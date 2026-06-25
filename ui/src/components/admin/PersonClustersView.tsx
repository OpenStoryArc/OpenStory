/**
 * PersonClustersView — fleet grouped by sovereign owner (Phase 5.9).
 *
 * Renders `Topology.clusters_by_person` as one card per person, each
 * listing the hosts where that person has sessions. Hosts can appear in
 * multiple cards — a shared dev box is normal.
 *
 * The cross-person sharing overlay (dashed edges between persons who have
 * consented exports/imports) lands in a follow-up commit once the
 * `share_policy` cross-person reader endpoint exists. For now this is the
 * baseline view: "who owns what."
 */

import type { PersonCluster } from "@/lib/admin-api";

interface Props {
  readonly clusters: readonly PersonCluster[];
  /** This device's own host — rendered with a "you are here" marker. */
  readonly selfHost: string;
}

export function PersonClustersView({ clusters, selfHost }: Props) {
  if (clusters.length === 0) {
    return (
      <p className="text-sm text-[#565f89]">
        No person clustering yet — either no sessions have a{" "}
        <code>person_id</code> stamp, or the directory bootstrap hasn't run.
        Sessions written before <code>person_id</code> support shipped don't
        appear here.
      </p>
    );
  }
  return (
    <div className="grid gap-3" data-testid="person-clusters-view">
      {clusters.map((c) => (
        <article
          key={c.person_id}
          className="rounded border border-[#24283b] bg-[#16161e] p-3"
          data-testid={`person-cluster-${c.person_id}`}
        >
          <header className="mb-2 flex items-baseline justify-between">
            <h4 className="text-sm font-medium text-[#bb9af7]">{c.person_id}</h4>
            <span className="text-xs text-[#565f89]">
              {c.hosts.length} {c.hosts.length === 1 ? "host" : "hosts"}
            </span>
          </header>
          <ul className="flex flex-wrap gap-1.5">
            {c.hosts.map((h) => {
              const isSelf = h === selfHost;
              return (
                <li
                  key={h}
                  className={`rounded px-2 py-0.5 text-xs font-mono ${
                    isSelf
                      ? "bg-[#9ece6a]/20 text-[#9ece6a]"
                      : "bg-[#24283b] text-[#c0caf5]"
                  }`}
                  title={isSelf ? "this device" : undefined}
                >
                  {h}
                  {isSelf && <span className="ml-1 text-[10px]">●</span>}
                </li>
              );
            })}
          </ul>
        </article>
      ))}
    </div>
  );
}
