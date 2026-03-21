import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { parseGitFlowSteps, type GitFlowStep } from "@/lib/git-flow-data";

// ── Boundary table for parseGitFlowSteps ────────────────────────────
// Covers every input partition: empty, missing, type mismatches,
// all 4 risk levels, mixed flows, non-string coercion, real-world patterns.

const BOUNDARY_TABLE: [
  string,                         // description
  Record<string, unknown>,        // metadata
  GitFlowStep[],                  // expected steps
][] = [
  // ── Guard clauses ───────────────────────────────────────────────

  // Empty metadata object
  ["empty metadata", {}, []],

  // Missing both commands and verbs
  ["missing both fields", { has_commit: true, length: 3 }, []],

  // commands present but verbs missing
  ["commands without verbs", { commands: ["git status"] }, []],

  // verbs present but commands missing
  ["verbs without commands", { verbs: ["status"] }, []],

  // commands is not an array (string)
  ["commands is string", { commands: "git status", verbs: ["status"] }, []],

  // verbs is not an array (number)
  ["verbs is number", { commands: ["git status"], verbs: 42 }, []],

  // Both empty arrays
  ["both empty arrays", { commands: [], verbs: [] }, []],

  // ── Single-step flows (one per risk level) ──────────────────────

  // Safe: status, diff, log, add
  [
    "single safe: git status",
    { commands: ["git status"], verbs: ["status"] },
    [{ verb: "status", command: "git status", risk: "safe", color: "#565f89" }],
  ],

  // State-change: checkout, switch, pull, fetch, stash, branch
  [
    "single state: git checkout",
    { commands: ["git checkout feature/foo"], verbs: ["checkout"] },
    [{ verb: "checkout", command: "git checkout feature/foo", risk: "state", color: "#7aa2f7" }],
  ],

  // Commit: commit, merge, rebase, cherry-pick
  [
    "single commit: git commit",
    { commands: ["git commit -m 'fix bug'"], verbs: ["commit"] },
    [{ verb: "commit", command: "git commit -m 'fix bug'", risk: "commit", color: "#9ece6a" }],
  ],

  // Destructive: push --force, reset --hard, clean -f, branch -D
  [
    "single destructive: git reset --hard",
    { commands: ["git reset --hard HEAD~1"], verbs: ["reset"] },
    [{ verb: "reset", command: "git reset --hard HEAD~1", risk: "destructive", color: "#f7768e" }],
  ],

  // ── Multi-step flows ────────────────────────────────────────────

  // Standard 4-step commit flow (most common pattern)
  [
    "standard flow: status → add → commit → push",
    {
      commands: ["git status", "git add src/main.rs", "git commit -m 'fix'", "git push"],
      verbs: ["status", "add", "commit", "push"],
    },
    [
      { verb: "status", command: "git status", risk: "safe", color: "#565f89" },
      { verb: "add", command: "git add src/main.rs", risk: "safe", color: "#565f89" },
      { verb: "commit", command: "git commit -m 'fix'", risk: "commit", color: "#9ece6a" },
      { verb: "push", command: "git push", risk: "safe", color: "#565f89" },
    ],
  ],

  // Mixed risk levels in one flow (safe + state + commit + destructive)
  [
    "mixed risk: diff → checkout → merge → push --force",
    {
      commands: ["git diff", "git checkout main", "git merge feature/x", "git push --force"],
      verbs: ["diff", "checkout", "merge", "push"],
    },
    [
      { verb: "diff", command: "git diff", risk: "safe", color: "#565f89" },
      { verb: "checkout", command: "git checkout main", risk: "state", color: "#7aa2f7" },
      { verb: "merge", command: "git merge feature/x", risk: "commit", color: "#9ece6a" },
      { verb: "push", command: "git push --force", risk: "destructive", color: "#f7768e" },
    ],
  ],

  // Long real-world flow (7 steps: fetch → checkout → pull → add → commit → push → log)
  [
    "7-step real-world flow",
    {
      commands: [
        "git fetch --all",
        "git checkout main",
        "git pull origin main",
        "git add .",
        "git commit -m 'release v2'",
        "git push origin main",
        "git log --oneline -3",
      ],
      verbs: ["fetch", "checkout", "pull", "add", "commit", "push", "log"],
    },
    [
      { verb: "fetch", command: "git fetch --all", risk: "state", color: "#7aa2f7" },
      { verb: "checkout", command: "git checkout main", risk: "state", color: "#7aa2f7" },
      { verb: "pull", command: "git pull origin main", risk: "state", color: "#7aa2f7" },
      { verb: "add", command: "git add .", risk: "safe", color: "#565f89" },
      { verb: "commit", command: "git commit -m 'release v2'", risk: "commit", color: "#9ece6a" },
      { verb: "push", command: "git push origin main", risk: "safe", color: "#565f89" },
      { verb: "log", command: "git log --oneline -3", risk: "safe", color: "#565f89" },
    ],
  ],

  // ── Edge cases ──────────────────────────────────────────────────

  // Destructive force-push (standalone)
  [
    "destructive force-push with target",
    { commands: ["git push --force origin main"], verbs: ["push"] },
    [{ verb: "push", command: "git push --force origin main", risk: "destructive", color: "#f7768e" }],
  ],

  // Mismatched array lengths — commands longer than verbs
  [
    "commands longer than verbs → truncate to min",
    {
      commands: ["git status", "git add .", "git commit -m 'x'"],
      verbs: ["status"],
    },
    [{ verb: "status", command: "git status", risk: "safe", color: "#565f89" }],
  ],

  // Mismatched array lengths — verbs longer than commands
  [
    "verbs longer than commands → truncate to min",
    {
      commands: ["git diff"],
      verbs: ["diff", "add", "commit"],
    },
    [{ verb: "diff", command: "git diff", risk: "safe", color: "#565f89" }],
  ],

  // Non-string elements in arrays (numbers, null) — String() coercion
  [
    "non-string elements coerced via String()",
    {
      commands: [42, null],
      verbs: [undefined, "x"],
    },
    [
      // String(42) = "42" — not a git command → risk "safe"
      // String(undefined) = "undefined"
      { verb: "undefined", command: "42", risk: "safe", color: "#565f89" },
      // String(null) = "null" — not a git command → risk "safe"
      { verb: "x", command: "null", risk: "safe", color: "#565f89" },
    ],
  ],

  // All destructive variants
  [
    "all destructive variants",
    {
      commands: [
        "git push --force",
        "git reset --hard HEAD",
        "git clean -fd",
        "git branch -D old",
        "git checkout -- .",
      ],
      verbs: ["push", "reset", "clean", "branch", "checkout"],
    },
    [
      { verb: "push", command: "git push --force", risk: "destructive", color: "#f7768e" },
      { verb: "reset", command: "git reset --hard HEAD", risk: "destructive", color: "#f7768e" },
      { verb: "clean", command: "git clean -fd", risk: "destructive", color: "#f7768e" },
      { verb: "branch", command: "git branch -D old", risk: "destructive", color: "#f7768e" },
      { verb: "checkout", command: "git checkout -- .", risk: "destructive", color: "#f7768e" },
    ],
  ],

  // All state-change variants
  [
    "all state-change variants",
    {
      commands: ["git checkout dev", "git switch main", "git stash", "git pull", "git fetch", "git branch new"],
      verbs: ["checkout", "switch", "stash", "pull", "fetch", "branch"],
    },
    [
      { verb: "checkout", command: "git checkout dev", risk: "state", color: "#7aa2f7" },
      { verb: "switch", command: "git switch main", risk: "state", color: "#7aa2f7" },
      { verb: "stash", command: "git stash", risk: "state", color: "#7aa2f7" },
      { verb: "pull", command: "git pull", risk: "state", color: "#7aa2f7" },
      { verb: "fetch", command: "git fetch", risk: "state", color: "#7aa2f7" },
      { verb: "branch", command: "git branch new", risk: "state", color: "#7aa2f7" },
    ],
  ],

  // All commit variants
  [
    "all commit variants",
    {
      commands: ["git commit -m 'x'", "git merge main", "git rebase main", "git cherry-pick abc"],
      verbs: ["commit", "merge", "rebase", "cherry-pick"],
    },
    [
      { verb: "commit", command: "git commit -m 'x'", risk: "commit", color: "#9ece6a" },
      { verb: "merge", command: "git merge main", risk: "commit", color: "#9ece6a" },
      { verb: "rebase", command: "git rebase main", risk: "commit", color: "#9ece6a" },
      { verb: "cherry-pick", command: "git cherry-pick abc", risk: "commit", color: "#9ece6a" },
    ],
  ],
];

describe("parseGitFlowSteps — boundary table", () => {
  it.each(BOUNDARY_TABLE)(
    "%s",
    (_desc, metadata, expected) => {
      scenario(
        () => metadata,
        (meta) => parseGitFlowSteps(meta),
        (result) => expect(result).toEqual(expected),
      );
    },
  );
});
