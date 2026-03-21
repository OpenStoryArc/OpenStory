/** Git command analysis — pure functions for risk classification */

export type GitRisk = "destructive" | "commit" | "state" | "safe";

/** Extract the git invocation from a command string, handling cd prefixes and env vars */
function extractGitPart(command: string): string | null {
  let cmd = command.trim();

  // Strip `cd <path> &&` prefix
  const cdMatch = cmd.match(/^cd\s+(?:"[^"]+"|'[^']+'|\S+)\s+&&\s+(.+)$/);
  if (cdMatch) cmd = cdMatch[1]!.trim();

  // Strip env var prefixes like `GIT_AUTHOR=x`
  while (/^\w+=\S+\s/.test(cmd)) {
    cmd = cmd.replace(/^\w+=\S+\s+/, "").trim();
  }

  // Must start with `git` followed by end-of-string or whitespace
  if (/^git(\s|$)/.test(cmd)) return cmd;
  return null;
}

/** Is this command a git command? */
export function isGitCommand(command: string): boolean {
  return extractGitPart(command) !== null;
}

/** Parse the git subcommand (e.g., "commit", "push") */
export function parseGitSubcommand(command: string): string | null {
  const gitPart = extractGitPart(command);
  if (!gitPart) return null;
  const parts = gitPart.split(/\s+/);
  // parts[0] is "git", parts[1] is the subcommand
  return parts[1] ?? null;
}

/** Classify a git command by risk level. Order: destructive > commit > state > safe */
export function gitCommandRisk(command: string): GitRisk {
  const gitPart = extractGitPart(command);
  if (!gitPart) return "safe";

  const sub = parseGitSubcommand(command);
  if (!sub) return "safe";

  // Destructive checks (order matters)
  if (sub === "push" && (/--force\b/.test(gitPart) || /\s-\w*f/.test(gitPart))) return "destructive";
  if (sub === "reset" && /--hard\b/.test(gitPart)) return "destructive";
  if (sub === "clean" && /\s-\w*f/.test(gitPart)) return "destructive";
  if (sub === "branch" && /\s-D\b/.test(gitPart)) return "destructive";
  if (sub === "checkout" && /--\s+\./.test(gitPart)) return "destructive";
  if (sub === "restore" && /\s\.\s*$/.test(gitPart)) return "destructive";

  // Commit
  if (["commit", "merge", "rebase", "cherry-pick"].includes(sub)) return "commit";

  // State change
  if (["checkout", "switch", "stash", "pull", "fetch", "branch"].includes(sub)) return "state";

  return "safe";
}

/** Human-readable label for a risk level */
export function gitRiskLabel(risk: GitRisk): string {
  switch (risk) {
    case "destructive": return "Destructive";
    case "commit":      return "Commit";
    case "state":       return "State Change";
    case "safe":        return "Safe";
  }
}

/** Compact summary: strip cd prefix and `git ` prefix, showing just the git-specific part */
export function gitCommandSummary(command: string): string {
  const gitPart = extractGitPart(command);
  if (!gitPart) return command;
  // Remove `git ` prefix
  const afterGit = gitPart.replace(/^git\s*/, "");
  return afterGit;
}

/** Risk color mapping (Tokyonight palette) */
export const GIT_RISK_COLORS: Record<GitRisk, string> = {
  destructive: "#f7768e",
  commit:      "#9ece6a",
  state:       "#7aa2f7",
  safe:        "#565f89",
};
