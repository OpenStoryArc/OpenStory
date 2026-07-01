/** Stable color per coding-agent platform, for cross-session views. */
const AGENT_COLORS: Record<string, string> = {
  "claude-code": "#7aa2f7",
  openactor: "#bb9af7",
  "pi-mono": "#9ece6a",
  codex: "#e0af68",
  pi: "#2ac3de",
  hermes: "#ff9e64",
};

export function agentColor(agent: string): string {
  return AGENT_COLORS[agent] ?? "#565f89";
}
