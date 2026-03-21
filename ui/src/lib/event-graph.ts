/** Event graph: inverted indexes over a flat WireRecord array.
 *
 *  Builds turn, file, tool, and agent indexes in a single pass.
 *  Enables faceted navigation: click a turn, file, or tool to filter events. */

import type { WireRecord } from "@/types/wire-record";
import type { ToolCall } from "@/types/view-record";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A turn: one human prompt -> agent work -> response cycle. */
export interface Turn {
  readonly index: number;
  readonly promptText: string | null;
  readonly promptTimestamp: string | null;
  readonly responseText: string | null;
  readonly eventIds: readonly string[];
  readonly toolCounts: Readonly<Record<string, number>>;
  readonly files: readonly string[];
  readonly hasError: boolean;
}

/** Inverted indexes over a flat event list. */
export interface EventGraph {
  readonly turns: readonly Turn[];
  readonly fileIndex: ReadonlyMap<string, readonly string[]>;
  readonly toolIndex: ReadonlyMap<string, readonly string[]>;
  readonly agentIndex: ReadonlyMap<string, readonly string[]>;
  readonly planIndex: ReadonlyMap<string, readonly string[]>;
  readonly errorIds: readonly string[];
}

/** Active facet selections for filtering. */
export interface ActiveFacets {
  readonly turn?: number;
  readonly file?: string;
  readonly tool?: string;
  readonly agent?: string;
  readonly plan?: string;
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

/** Extract file path from a tool_call record's payload. */
export function extractFilePath(record: WireRecord): string | null {
  if (record.record_type !== "tool_call") return null;

  const payload = record.payload as ToolCall;

  // Try typed_input first
  const ti = payload.typed_input as Record<string, unknown> | undefined;
  if (ti) {
    const fp = ti.file_path as string | undefined;
    if (fp) return fp;
  }

  // Try raw_input
  const ri = payload.raw_input as Record<string, unknown> | undefined;
  if (ri) {
    const fp = (ri.file_path ?? ri.file ?? ri.path) as string | undefined;
    if (fp) return fp;
  }

  return null;
}

/** Extract tool name from a tool_call record. */
function extractToolName(record: WireRecord): string | null {
  if (record.record_type !== "tool_call") return null;
  return (record.payload as ToolCall).name ?? null;
}

/** Extract a plan title from a tool_call record.
 *  - ExitPlanMode: extracts first line of plan content (strips `# ` heading prefix)
 *  - EnterPlanMode: returns "[plan mode]"
 *  - Anything else: returns null */
export function extractPlanTitle(record: WireRecord): string | null {
  if (record.record_type !== "tool_call") return null;

  const payload = record.payload as ToolCall;
  const name = payload.name;

  if (name === "EnterPlanMode") return "[plan mode]";

  if (name === "ExitPlanMode") {
    // Try typed_input first, then raw_input
    const ti = payload.typed_input as Record<string, unknown> | undefined;
    const ri = payload.raw_input as Record<string, unknown> | undefined;
    const planContent = (ti?.plan as string | undefined) ?? (ri?.plan as string | undefined);

    if (!planContent || planContent.trim() === "") return "Untitled plan";

    const firstLine = planContent.trim().split("\n")[0]!.trim();
    // Strip leading markdown heading prefix
    return firstLine.replace(/^#+\s*/, "") || "Untitled plan";
  }

  return null;
}

/** Check if a record is an error. */
function isError(record: WireRecord): boolean {
  if (record.record_type === "error") return true;
  if (record.record_type === "tool_result") {
    const payload = record.payload as Record<string, unknown>;
    if (payload.is_error) return true;
  }
  return false;
}

// ---------------------------------------------------------------------------
// Turn splitting
// ---------------------------------------------------------------------------

/** Split records into turns at user_message boundaries.
 *  Events before the first user_message form turn 0. */
export function splitIntoTurns(records: readonly WireRecord[]): Turn[] {
  const turns: Turn[] = [];
  let currentEvents: WireRecord[] = [];
  let currentPrompt: string | null = null;
  let currentPromptTs: string | null = null;
  let turnIdx = 0;

  function flush() {
    if (currentEvents.length === 0) return;

    const toolCounts: Record<string, number> = {};
    const filesSet = new Set<string>();
    let hasErr = false;
    let response: string | null = null;

    for (const e of currentEvents) {
      const tn = extractToolName(e);
      if (tn) toolCounts[tn] = (toolCounts[tn] ?? 0) + 1;

      const fp = extractFilePath(e);
      if (fp) filesSet.add(fp);

      if (isError(e)) hasErr = true;

      if (e.record_type === "assistant_message") {
        const text = (e.payload as Record<string, unknown>).text as string | undefined;
        if (text) response = text;
      }
    }

    turns.push({
      index: turnIdx,
      promptText: currentPrompt,
      promptTimestamp: currentPromptTs,
      responseText: response,
      eventIds: currentEvents.map((e) => e.id),
      toolCounts,
      files: [...filesSet].sort(),
      hasError: hasErr,
    });
    turnIdx++;
  }

  for (const r of records) {
    if (r.record_type === "user_message") {
      flush();
      currentEvents = [r];
      const text = (r.payload as Record<string, unknown>).text as string | undefined;
      currentPrompt = text ? (text.length > 100 ? text.slice(0, 100) + "..." : text) : null;
      currentPromptTs = r.timestamp;
    } else {
      currentEvents.push(r);
    }
  }

  flush();
  return turns;
}

// ---------------------------------------------------------------------------
// Graph building
// ---------------------------------------------------------------------------

/** Build all indexes in a single pass over the records. */
export function buildEventGraph(records: readonly WireRecord[]): EventGraph {
  const fileMap = new Map<string, string[]>();
  const toolMap = new Map<string, string[]>();
  const agentMap = new Map<string, string[]>();
  const planMap = new Map<string, string[]>();
  const errorIds: string[] = [];

  for (const r of records) {
    const fp = extractFilePath(r);
    if (fp) {
      const list = fileMap.get(fp);
      if (list) list.push(r.id);
      else fileMap.set(fp, [r.id]);
    }

    const tn = extractToolName(r);
    if (tn) {
      const list = toolMap.get(tn);
      if (list) list.push(r.id);
      else toolMap.set(tn, [r.id]);
    }

    if (r.agent_id) {
      const list = agentMap.get(r.agent_id);
      if (list) list.push(r.id);
      else agentMap.set(r.agent_id, [r.id]);
    }

    const planTitle = extractPlanTitle(r);
    if (planTitle) {
      const list = planMap.get(planTitle);
      if (list) list.push(r.id);
      else planMap.set(planTitle, [r.id]);
    }

    if (isError(r)) errorIds.push(r.id);
  }

  return {
    turns: splitIntoTurns(records),
    fileIndex: fileMap,
    toolIndex: toolMap,
    agentIndex: agentMap,
    planIndex: planMap,
    errorIds,
  };
}

// ---------------------------------------------------------------------------
// Faceted queries
// ---------------------------------------------------------------------------

/** Return event IDs matching all active facets (intersection).
 *  No facets = all event IDs. */
export function applyFacets(
  graph: EventGraph,
  records: readonly WireRecord[],
  facets: ActiveFacets,
): string[] {
  const sets: Set<string>[] = [];

  if (facets.turn != null) {
    const turn = graph.turns[facets.turn];
    if (turn) {
      sets.push(new Set(turn.eventIds));
    } else {
      return [];
    }
  }

  if (facets.file != null) {
    sets.push(new Set(graph.fileIndex.get(facets.file) ?? []));
  }

  if (facets.tool != null) {
    sets.push(new Set(graph.toolIndex.get(facets.tool) ?? []));
  }

  if (facets.agent != null) {
    sets.push(new Set(graph.agentIndex.get(facets.agent) ?? []));
  }

  if (facets.plan != null) {
    sets.push(new Set(graph.planIndex.get(facets.plan) ?? []));
  }

  if (sets.length === 0) {
    return records.map((r) => r.id);
  }

  let result = sets[0]!;
  for (let i = 1; i < sets.length; i++) {
    result = new Set([...result].filter((id) => sets[i]!.has(id)));
  }

  // Preserve original order
  return records.filter((r) => result.has(r.id)).map((r) => r.id);
}

/** Get sorted file entries for the facet panel: path + event count + read/write breakdown. */
export interface FileFacet {
  readonly path: string;
  readonly count: number;
  readonly reads: number;
  readonly writes: number;
}

export function fileFacets(
  graph: EventGraph,
  records: readonly WireRecord[],
): FileFacet[] {
  const recordMap = new Map<string, WireRecord>();
  for (const r of records) recordMap.set(r.id, r);

  const facets: FileFacet[] = [];
  for (const [path, ids] of graph.fileIndex) {
    let reads = 0;
    let writes = 0;
    for (const id of ids) {
      const r = recordMap.get(id);
      if (!r) continue;
      const tn = extractToolName(r);
      if (tn === "Read" || tn === "Grep" || tn === "Glob") reads++;
      else if (tn === "Edit" || tn === "Write") writes++;
    }
    facets.push({ path, count: ids.length, reads, writes });
  }

  return facets.sort((a, b) => b.count - a.count);
}

/** Get sorted tool entries for the facet panel. */
export interface ToolFacet {
  readonly name: string;
  readonly count: number;
  readonly turnCount: number;
}

export function toolFacets(graph: EventGraph): ToolFacet[] {
  const facets: ToolFacet[] = [];
  for (const [name, ids] of graph.toolIndex) {
    const turnSet = new Set<number>();
    for (const t of graph.turns) {
      if (name in t.toolCounts) turnSet.add(t.index);
    }
    facets.push({ name, count: ids.length, turnCount: turnSet.size });
  }
  return facets.sort((a, b) => b.count - a.count);
}

/** Get sorted plan entries for the facet panel. */
export interface PlanFacet {
  readonly title: string;
  readonly count: number;
}

export function planFacets(graph: EventGraph): readonly PlanFacet[] {
  return [...graph.planIndex.entries()]
    .map(([title, ids]) => ({ title, count: ids.length }))
    .sort((a, b) => b.count - a.count);
}
