export function originAgentLabel(agent: string | null | undefined): string | null {
  switch (agent) {
    case "claude-code":
      return "Claude";
    case "codex":
      return "Codex";
    case "pi-mono":
      return "pi-mono";
    case "hermes":
      return "Hermes";
    case "grok-build":
    case "grok":
      return "Grok";
    case null:
    case undefined:
    case "":
      return null;
    default:
      return agent;
  }
}

export function originAgentColor(agent: string | null | undefined): string {
  switch (agent) {
    case "codex":
      return "#7aa2f7";
    case "claude-code":
      return "#bb9af7";
    case "pi-mono":
      return "#9ece6a";
    case "hermes":
      return "#e0af68";
    case "grok-build":
    case "grok":
      return "#f7768e";
    default:
      return "#565f89";
  }
}
