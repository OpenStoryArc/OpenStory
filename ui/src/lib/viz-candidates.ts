/** The Lab's data layer: the falsifiable viz design-space catalog. Each
 *  candidate is a proposed visualization carrying its own falsifier + witness
 *  (a concrete data check that could kill it), so the Lab can render the space
 *  as testable claims, not vibes. Fetched from GET /api/viz-candidates (which
 *  serves docs/research/viz-candidates.json — the source of truth). */

export type CandidateStatus = "idea" | "witnessed" | "refuted" | "built";

export interface VizCandidate {
  readonly id: string;
  readonly name: string;
  readonly d3_shape: string;
  readonly data_shape: string;
  readonly what_it_shows: string;
  readonly openstory_fields_used: readonly string[];
  readonly novelty: number;
  readonly insight_value: number;
  readonly build_cost: number;
  readonly score: number;
  readonly notes?: string;
  readonly hypothesis: string;
  readonly falsifier: string;
  readonly witness: string;
  readonly status?: CandidateStatus;
}

/** Highest score first; ties broken by name for determinism. Pure. */
export function sortByScore(candidates: readonly VizCandidate[]): VizCandidate[] {
  return [...candidates].sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));
}

export async function fetchVizCandidates(baseUrl = ""): Promise<VizCandidate[]> {
  try {
    const r = await fetch(`${baseUrl}/api/viz-candidates`);
    const j = await r.json();
    return Array.isArray(j.candidates) ? (j.candidates as VizCandidate[]) : [];
  } catch {
    return [];
  }
}
