import { describe, expect, it } from "vitest";
import { originAgentLabel } from "@/lib/origin-agent";

describe("originAgentLabel", () => {
  it("maps known origin agents to UI labels", () => {
    expect(originAgentLabel("claude-code")).toBe("Claude");
    expect(originAgentLabel("codex")).toBe("Codex");
    expect(originAgentLabel("pi-mono")).toBe("pi-mono");
    expect(originAgentLabel("hermes")).toBe("Hermes");
    expect(originAgentLabel("grok-build")).toBe("Grok");
    expect(originAgentLabel("grok")).toBe("Grok");
  });

  it("hides empty origin agents and preserves unknown ones", () => {
    expect(originAgentLabel(null)).toBeNull();
    expect(originAgentLabel(undefined)).toBeNull();
    expect(originAgentLabel("")).toBeNull();
    expect(originAgentLabel("custom-agent")).toBe("custom-agent");
  });
});
