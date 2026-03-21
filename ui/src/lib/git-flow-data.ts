/** Parse git.workflow pattern metadata into renderable steps. */

import { gitCommandRisk, GIT_RISK_COLORS, type GitRisk } from "@/lib/git-commands";

export interface GitFlowStep {
  readonly verb: string;
  readonly command: string;
  readonly risk: GitRisk;
  readonly color: string;
}

/** Extract git flow steps from pattern metadata.
 *  Expects metadata.commands and metadata.verbs as parallel arrays. */
export function parseGitFlowSteps(
  metadata: Readonly<Record<string, unknown>>,
): GitFlowStep[] {
  const commands = metadata.commands;
  const verbs = metadata.verbs;

  if (!Array.isArray(commands) || !Array.isArray(verbs)) return [];

  const len = Math.min(commands.length, verbs.length);
  const steps: GitFlowStep[] = [];

  for (let i = 0; i < len; i++) {
    const command = String(commands[i]);
    const verb = String(verbs[i]);
    const risk = gitCommandRisk(command);
    steps.push({
      verb,
      command,
      risk,
      color: GIT_RISK_COLORS[risk],
    });
  }

  return steps;
}
