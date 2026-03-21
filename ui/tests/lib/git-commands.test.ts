import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  isGitCommand,
  parseGitSubcommand,
  gitCommandRisk,
  gitRiskLabel,
  gitCommandSummary,
  type GitRisk,
} from "@/lib/git-commands";

// ── isGitCommand + parseGitSubcommand + gitCommandRisk + gitCommandSummary ──
// Complete boundary table from the plan.

const BOUNDARY_TABLE: [
  string,        // command
  boolean,       // isGitCommand
  string | null, // subcommand
  GitRisk | null,// risk (null when not a git command)
  string | null, // summary (null when not a git command)
][] = [
  // Safe commands
  ["git status",                  true,  "status",      "safe",        "status"],
  ["git diff --stat",             true,  "diff",        "safe",        "diff --stat"],
  ["git log --oneline -5",        true,  "log",         "safe",        "log --oneline -5"],
  ["git add src/main.rs",         true,  "add",         "safe",        "add src/main.rs"],

  // Commit commands
  ["git commit -m 'Fix bug'",    true,  "commit",      "commit",      "commit -m 'Fix bug'"],
  ["git commit --amend",          true,  "commit",      "commit",      "commit --amend"],
  ["git merge feature/foo",       true,  "merge",       "commit",      "merge feature/foo"],
  ["git rebase main",             true,  "rebase",      "commit",      "rebase main"],
  ["git cherry-pick abc123",      true,  "cherry-pick", "commit",      "cherry-pick abc123"],

  // State-change commands
  ["git checkout feature/bar",    true,  "checkout",    "state",       "checkout feature/bar"],
  ["git switch main",             true,  "switch",      "state",       "switch main"],
  ["git pull origin main",        true,  "pull",        "state",       "pull origin main"],
  ["git stash",                   true,  "stash",       "state",       "stash"],
  ["git stash pop",               true,  "stash",       "state",       "stash pop"],
  ["git fetch --all",             true,  "fetch",       "state",       "fetch --all"],
  ["git branch feature/new",      true,  "branch",      "state",       "branch feature/new"],

  // Destructive commands
  ["git push --force origin main", true, "push",        "destructive", "push --force origin main"],
  ["git push -f",                 true,  "push",        "destructive", "push -f"],
  ["git reset --hard HEAD~1",     true,  "reset",       "destructive", "reset --hard HEAD~1"],
  ["git clean -fd",               true,  "clean",       "destructive", "clean -fd"],
  ["git branch -D old-branch",    true,  "branch",      "destructive", "branch -D old-branch"],
  ["git checkout -- .",           true,  "checkout",    "destructive", "checkout -- ."],
  ["git restore .",               true,  "restore",     "destructive", "restore ."],

  // With cd prefix
  ["cd /tmp && git status",       true,  "status",      "safe",        "status"],

  // Not git commands
  ["npm test",                    false, null,          null,          null],
  ['echo "git is great"',         false, null,          null,          null],

  // Bare git
  ["git",                         true,  null,          "safe",        ""],

  // Env var prefix
  ["GIT_AUTHOR=x git commit",    true,  "commit",      "commit",      "commit"],
];

describe("git-commands — boundary table", () => {
  describe("isGitCommand", () => {
    it.each(BOUNDARY_TABLE)(
      "isGitCommand(%s) → %s",
      (command, expected) => {
        scenario(
          () => command,
          (cmd) => isGitCommand(cmd),
          (result) => expect(result).toBe(expected),
        );
      },
    );
  });

  describe("parseGitSubcommand", () => {
    it.each(BOUNDARY_TABLE.filter(([, isGit]) => isGit))(
      "parseGitSubcommand(%s) → %s",
      (command, _isGit, expectedSub) => {
        scenario(
          () => command,
          (cmd) => parseGitSubcommand(cmd),
          (result) => expect(result).toBe(expectedSub),
        );
      },
    );

    it.each(BOUNDARY_TABLE.filter(([, isGit]) => !isGit))(
      "parseGitSubcommand(%s) → null",
      (command) => {
        expect(parseGitSubcommand(command)).toBeNull();
      },
    );
  });

  describe("gitCommandRisk", () => {
    it.each(BOUNDARY_TABLE.filter(([, isGit]) => isGit))(
      "gitCommandRisk(%s) → %s",
      (command, _isGit, _sub, expectedRisk) => {
        scenario(
          () => command,
          (cmd) => gitCommandRisk(cmd),
          (result) => expect(result).toBe(expectedRisk),
        );
      },
    );
  });

  describe("gitCommandSummary", () => {
    it.each(BOUNDARY_TABLE.filter(([, isGit]) => isGit))(
      "gitCommandSummary(%s) → %s",
      (command, _isGit, _sub, _risk, expectedSummary) => {
        scenario(
          () => command,
          (cmd) => gitCommandSummary(cmd),
          (result) => expect(result).toBe(expectedSummary),
        );
      },
    );
  });
});

describe("gitRiskLabel", () => {
  it.each<[GitRisk, string]>([
    ["destructive", "Destructive"],
    ["commit", "Commit"],
    ["state", "State Change"],
    ["safe", "Safe"],
  ])("gitRiskLabel(%s) → %s", (risk, expected) => {
    expect(gitRiskLabel(risk)).toBe(expected);
  });
});
