// sentence.ts — Turn sentences: the grammar of agent behavior.
//
// Each turn is a sentence. The sentence has structure:
//   Subject (Claude) + Verb (action) + Object (what) + Adverbial (why)
//   + Subordinate clauses (supporting actions) + Predicate (outcome)
//
// Tool calls are classified by their ROLE in the turn's narrative:
//   preparatory  — research before acting (Read, Grep, Search)
//   creative     — producing artifacts (Write, Edit, commit)
//   verificatory — checking work (test, build, ls)
//   delegatory   — handing off to a sub-agent (Agent)
//   interactive  — coordination (AskUser, ToolSearch)

import type { StructuralTurn } from "./types.js";

// ─────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────

export type ToolRole = "preparatory" | "creative" | "verificatory" | "delegatory" | "interactive";

export interface SubordinateClause {
  role: ToolRole;
  verb: string;      // "after reading", "while testing", "by delegating"
  object: string;    // "6 Rust files", "the test suite"
  toolCalls: number;
}

export interface TurnSentence {
  subject: string;
  verb: string;
  object: string;
  adverbial: string | null;
  subordinates: SubordinateClause[];
  predicate: string;
  oneLiner: string;
}

// ─────────────────────────────────────────────
// Tool classification
// ─────────────────────────────────────────────

const PREPARATORY_TOOLS = new Set(["Read", "Grep", "Glob", "WebSearch", "WebFetch"]);
const CREATIVE_TOOLS = new Set(["Write", "Edit"]);
const DELEGATORY_TOOLS = new Set(["Agent"]);
const INTERACTIVE_TOOLS = new Set(["AskUserQuestion", "ToolSearch", "ExitPlanMode"]);

export function classifyTool(name: string, input: string): ToolRole {
  if (PREPARATORY_TOOLS.has(name)) return "preparatory";
  if (CREATIVE_TOOLS.has(name)) return "creative";
  if (DELEGATORY_TOOLS.has(name)) return "delegatory";
  if (INTERACTIVE_TOOLS.has(name)) return "interactive";

  // Bash depends on the command
  if (name === "Bash") {
    const lower = input.toLowerCase();
    if (/\b(test|spec|jest|vitest|pytest|cargo test|npm test)\b/.test(lower)) return "verificatory";
    if (/\b(install|brew|apt|npm i |pip )\b/.test(lower)) return "preparatory";
    if (/\bgit (commit|push|add|tag)\b/.test(lower)) return "creative";
    if (/\bgit (status|log|diff|branch)\b/.test(lower)) return "preparatory";
    if (/\b(mkdir|chmod|cp |mv )\b/.test(lower)) return "creative";
    return "verificatory"; // default: checking something
  }

  return "verificatory";
}

// ─────────────────────────────────────────────
// Sentence builder
// ─────────────────────────────────────────────

export function buildSentence(turn: StructuralTurn): TurnSentence {
  const subject = "Claude";

  // Classify all tools
  const classified = turn.applies.map(a => ({
    ...a,
    role: classifyTool(a.toolName, a.inputSummary),
  }));

  // Group by role
  const byRole = groupBy(classified, c => c.role);

  // Determine dominant role and verb
  const { verb, object: verbObject } = extractVerbAndObject(turn, classified, byRole);

  // Extract adverbial from human message
  const adverbial = extractAdverbial(turn);

  // Build subordinate clauses (non-dominant roles)
  const dominantRole = getDominantRole(byRole);
  const subordinates = buildSubordinates(byRole, dominantRole);

  // Predicate
  const predicate = turn.isTerminal ? "answered" : "continued";

  // Compose one-liner
  const oneLiner = composeOneLiner(subject, verb, verbObject, adverbial, subordinates, predicate);

  return {
    subject,
    verb,
    object: verbObject,
    adverbial,
    subordinates,
    predicate,
    oneLiner,
  };
}

// ─────────────────────────────────────────────
// Verb + object extraction
// ─────────────────────────────────────────────

interface ClassifiedApply {
  toolName: string;
  inputSummary: string;
  outputSummary: string;
  isAgent: boolean;
  isError: boolean;
  role: ToolRole;
}

function extractVerbAndObject(
  turn: StructuralTurn,
  classified: ClassifiedApply[],
  byRole: Map<ToolRole, ClassifiedApply[]>,
): { verb: string; object: string } {
  // No tools → pure response
  if (classified.length === 0) {
    return inferFromEval(turn);
  }

  const creative = byRole.get("creative") ?? [];
  const preparatory = byRole.get("preparatory") ?? [];
  const verificatory = byRole.get("verificatory") ?? [];
  const delegatory = byRole.get("delegatory") ?? [];

  // Dominant role determines verb
  if (creative.length > 0) {
    return extractCreativeVerb(creative);
  }
  if (delegatory.length > 0) {
    return extractDelegatoryVerb(delegatory);
  }
  if (preparatory.length > 0 && verificatory.length === 0) {
    return extractPreparatoryVerb(preparatory);
  }
  if (verificatory.length > 0) {
    return extractVerificatoryVerb(verificatory, preparatory);
  }

  return { verb: "worked on", object: summarizeInputs(classified) };
}

function inferFromEval(turn: StructuralTurn): { verb: string; object: string } {
  const content = turn.eval?.content ?? "";
  const lower = content.toLowerCase();

  if (lower.startsWith("yes") || lower.startsWith("no") || lower.startsWith("right") || lower.startsWith("exactly")) {
    return { verb: "responded", object: "" };
  }

  // The object comes from what Claude SAID (the eval content),
  // not what the human asked. The human message is the cause
  // (adverbial), Claude's response is the object (what was explained).
  if (turn.human) {
    const q = turn.human.content.toLowerCase();
    if (q.includes("?") || q.includes("what") || q.includes("how") || q.includes("why") || q.includes("tell me")) {
      return { verb: "explained", object: extractTopic(content) };
    }
  }

  if (content.length > 20) {
    return { verb: "explained", object: extractTopic(content) };
  }

  return { verb: "answered", object: "" };
}

function extractCreativeVerb(creative: ClassifiedApply[]): { verb: string; object: string } {
  const writes = creative.filter(c => c.toolName === "Write");
  const edits = creative.filter(c => c.toolName === "Edit");
  const commits = creative.filter(c => c.inputSummary.includes("git commit") || c.inputSummary.includes("git push"));

  if (commits.length > 0) {
    return { verb: "committed", object: summarizeInputs(creative.filter(c => !c.inputSummary.includes("git"))) || "changes" };
  }
  if (writes.length > 0) {
    return { verb: "wrote", object: summarizeFiles(writes) };
  }
  if (edits.length > 0) {
    return { verb: "edited", object: summarizeFiles(edits) };
  }
  return { verb: "created", object: summarizeInputs(creative) };
}

function extractDelegatoryVerb(delegatory: ClassifiedApply[]): { verb: string; object: string } {
  if (delegatory.length === 1) {
    const desc = delegatory[0].inputSummary;
    return { verb: "delegated", object: desc || "a sub-task" };
  }
  return { verb: "delegated", object: `${delegatory.length} sub-tasks` };
}

function extractPreparatoryVerb(preparatory: ClassifiedApply[]): { verb: string; object: string } {
  const reads = preparatory.filter(c => c.toolName === "Read");
  const greps = preparatory.filter(c => c.toolName === "Grep");
  const searches = preparatory.filter(c => c.toolName === "WebSearch" || c.toolName === "WebFetch");
  const installs = preparatory.filter(c => c.inputSummary.toLowerCase().includes("install"));

  if (installs.length > 0) {
    return { verb: "installed", object: extractInstallTarget(installs[0].inputSummary) };
  }
  if (searches.length > 0) {
    return { verb: "searched", object: searches[0].inputSummary || "the web" };
  }
  if (greps.length > 0) {
    return { verb: "searched for", object: greps[0].inputSummary || "patterns" };
  }
  if (reads.length > 0) {
    return { verb: "read", object: summarizeFiles(reads) };
  }
  return { verb: "explored", object: summarizeInputs(preparatory) };
}

function extractVerificatoryVerb(
  verificatory: ClassifiedApply[],
  preparatory: ClassifiedApply[],
): { verb: string; object: string } {
  const tests = verificatory.filter(c =>
    /test|spec|jest|vitest/.test(c.inputSummary.toLowerCase()));
  if (tests.length > 0) {
    return { verb: "ran tests", object: tests[0].inputSummary.slice(0, 60) };
  }
  if (preparatory.length > verificatory.length) {
    return extractPreparatoryVerb(preparatory);
  }
  return { verb: "checked", object: summarizeInputs(verificatory) };
}

// ─────────────────────────────────────────────
// Adverbial extraction (the why)
// ─────────────────────────────────────────────

function extractAdverbial(turn: StructuralTurn): string | null {
  if (!turn.human) return null;
  const content = turn.human.content.trim();
  if (content.length === 0) return null;

  // Truncate to essence
  const short = content.length > 60
    ? content.slice(0, 57) + "..."
    : content;

  return `"${short}"`;
}

// ─────────────────────────────────────────────
// Subordinate clauses
// ─────────────────────────────────────────────

const ROLE_ORDER: ToolRole[] = ["preparatory", "creative", "verificatory", "delegatory", "interactive"];

const ROLE_VERBS: Record<ToolRole, string> = {
  preparatory: "after reading",
  creative: "writing",
  verificatory: "while testing",
  delegatory: "by delegating to",
  interactive: "asking about",
};

function buildSubordinates(
  byRole: Map<ToolRole, ClassifiedApply[]>,
  dominantRole: ToolRole | null,
): SubordinateClause[] {
  const clauses: SubordinateClause[] = [];

  for (const role of ROLE_ORDER) {
    if (role === dominantRole) continue;
    const tools = byRole.get(role);
    if (!tools || tools.length === 0) continue;

    clauses.push({
      role,
      verb: ROLE_VERBS[role],
      object: summarizeForClause(role, tools),
      toolCalls: tools.length,
    });
  }

  return clauses;
}

function summarizeForClause(role: ToolRole, tools: ClassifiedApply[]): string {
  switch (role) {
    case "preparatory": {
      const reads = tools.filter(t => t.toolName === "Read");
      if (reads.length > 0) return summarizeFiles(reads);
      return `${tools.length} source${tools.length > 1 ? "s" : ""}`;
    }
    case "creative":
      return summarizeFiles(tools);
    case "verificatory":
      return `${tools.length} check${tools.length > 1 ? "s" : ""}`;
    case "delegatory":
      return tools[0]?.inputSummary || "a sub-agent";
    case "interactive":
      return `${tools.length} interaction${tools.length > 1 ? "s" : ""}`;
  }
}

// ─────────────────────────────────────────────
// One-liner composition
// ─────────────────────────────────────────────

function composeOneLiner(
  subject: string,
  verb: string,
  object: string,
  adverbial: string | null,
  subordinates: SubordinateClause[],
  predicate: string,
): string {
  let parts = [`${subject} ${verb}`];
  if (object) parts[0] += ` ${object}`;

  // Add subordinate clauses (max 2 for brevity)
  for (const clause of subordinates.slice(0, 2)) {
    parts.push(`${clause.verb} ${clause.object}`);
  }

  // Add the why
  if (adverbial) {
    parts.push(`because ${adverbial}`);
  }

  return parts.join(", ") + ` → ${predicate}`;
}

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

function getDominantRole(byRole: Map<ToolRole, ClassifiedApply[]>): ToolRole | null {
  // Priority: creative > delegatory > preparatory > verificatory
  if ((byRole.get("creative")?.length ?? 0) > 0) return "creative";
  if ((byRole.get("delegatory")?.length ?? 0) > 0) return "delegatory";
  if ((byRole.get("preparatory")?.length ?? 0) > 0 && (byRole.get("verificatory")?.length ?? 0) === 0) return "preparatory";
  if ((byRole.get("verificatory")?.length ?? 0) > 0) return "verificatory";
  return null;
}

function summarizeFiles(applies: ClassifiedApply[]): string {
  const paths = applies
    .map(a => a.inputSummary)
    .filter(p => p.length > 0);

  if (paths.length === 0) return "files";

  // Extract filenames from paths
  const names = paths.map(p => {
    const parts = p.split("/");
    return parts[parts.length - 1] || p;
  });

  // Detect common patterns
  const extensions = names.map(n => {
    const dot = n.lastIndexOf(".");
    return dot > 0 ? n.slice(dot) : "";
  }).filter(e => e);
  const extCounts = countBy(extensions);
  const dominantExt = [...extCounts.entries()].sort((a, b) => b[1] - a[1])[0];

  if (names.length === 1) return names[0];
  if (names.length <= 3) return names.join(", ");

  // Summarize: "8 Scheme files" or "6 source files"
  if (dominantExt && dominantExt[1] > names.length / 2) {
    const extName = extToLanguage(dominantExt[0]);
    return `${names.length} ${extName} files`;
  }
  return `${names.length} files`;
}

function summarizeInputs(applies: ClassifiedApply[]): string {
  if (applies.length === 0) return "";
  if (applies.length === 1) {
    const s = applies[0].inputSummary;
    return s.length > 60 ? s.slice(0, 57) + "..." : s;
  }
  return `${applies.length} operations`;
}

function extractTopic(humanContent: string): string {
  // Extract the core topic from a question
  const lower = humanContent.toLowerCase();
  // Remove question words
  const cleaned = lower
    .replace(/^(what|how|why|can you|could you|tell me|explain|describe)\s+(is|are|does|do|about|the)?\s*/i, "")
    .replace(/\?+$/, "")
    .trim();
  return cleaned.length > 50 ? cleaned.slice(0, 47) + "..." : cleaned;
}

function extractInstallTarget(input: string): string {
  const match = input.match(/install\s+(\S+)/);
  return match ? match[1] : "dependencies";
}

function extToLanguage(ext: string): string {
  const map: Record<string, string> = {
    ".scm": "Scheme",
    ".ts": "TypeScript",
    ".js": "JavaScript",
    ".rs": "Rust",
    ".py": "Python",
    ".md": "Markdown",
    ".html": "HTML",
    ".css": "CSS",
    ".json": "JSON",
    ".toml": "TOML",
    ".yaml": "YAML",
    ".sh": "shell",
  };
  return map[ext] ?? "source";
}

function groupBy<T>(items: T[], key: (item: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const k = key(item);
    const arr = map.get(k) ?? [];
    arr.push(item);
    map.set(k, arr);
  }
  return map;
}

function countBy(items: string[]): Map<string, number> {
  const map = new Map<string, number>();
  for (const item of items) {
    map.set(item, (map.get(item) ?? 0) + 1);
  }
  return map;
}
