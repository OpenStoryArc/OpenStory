/**
 * Spec: Domain fact extraction from ToolOutcome data.
 *
 * Pure functions that transform raw ToolOutcome objects into
 * displayable domain facts: file paths, commands, search patterns.
 *
 * Each fact is a deterministic derivation from the ToolOutcome —
 * same outcome, same fact. Always.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  extractDomainFact,
  extractDomainFacts,
} from "@/lib/domain-facts";

// ═══════════════════════════════════════════════════════════════════
// extractDomainFact — one ToolOutcome → one DomainFact
// ═══════════════════════════════════════════════════════════════════

describe("extractDomainFact", () => {
  it("should extract FileCreated with short filename", () => {
    scenario(
      () => ({ type: "FileCreated", path: "/Users/user/projects/OpenStory/rs/patterns/src/eval_apply.rs" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("created");
        expect(fact.label).toBe("eval_apply.rs");
        expect(fact.detail).toContain("eval_apply.rs");
      },
    );
  });

  it("should extract FileModified with short filename", () => {
    scenario(
      () => ({ type: "FileModified", path: "/Users/user/projects/OpenStory/rs/patterns/src/sentence.rs" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("modified");
        expect(fact.label).toBe("sentence.rs");
      },
    );
  });

  it("should extract FileRead with short filename", () => {
    scenario(
      () => ({ type: "FileRead", path: "/Users/user/projects/OpenStory/docs/README.md" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("read");
        expect(fact.label).toBe("README.md");
      },
    );
  });

  it("should extract CommandExecuted with success", () => {
    scenario(
      () => ({ type: "CommandExecuted", command: "cargo test -p open-story-core", succeeded: true }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("command_ok");
        expect(fact.label).toContain("cargo test");
      },
    );
  });

  it("should extract CommandExecuted with failure", () => {
    scenario(
      () => ({ type: "CommandExecuted", command: "cargo test", succeeded: false }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("command_fail");
        expect(fact.label).toContain("cargo test");
      },
    );
  });

  it("should truncate long commands", () => {
    scenario(
      () => ({ type: "CommandExecuted", command: "curl -s http://localhost:3002/api/sessions/very-long-session-id/patterns?type=turn.sentence | python3 -c 'import json'", succeeded: true }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.label.length).toBeLessThanOrEqual(45);
      },
    );
  });

  it("should extract SearchPerformed with pattern", () => {
    scenario(
      () => ({ type: "SearchPerformed", pattern: "docs/**/*.scm", source: "filesystem" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("search");
        expect(fact.label).toContain("docs/**/*.scm");
      },
    );
  });

  it("should extract SubAgentSpawned with description", () => {
    scenario(
      () => ({ type: "SubAgentSpawned", description: "Explore claurst project structure" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("agent");
        expect(fact.label).toContain("Explore claurst");
      },
    );
  });

  it("should extract FileWriteFailed", () => {
    scenario(
      () => ({ type: "FileWriteFailed", path: "/readonly.txt", reason: "Permission denied" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("error");
        expect(fact.label).toContain("readonly.txt");
      },
    );
  });

  it("should extract FileReadFailed", () => {
    scenario(
      () => ({ type: "FileReadFailed", path: "/missing.rs", reason: "File not found" }),
      (outcome) => extractDomainFact(outcome),
      (fact) => {
        expect(fact.kind).toBe("error");
        expect(fact.label).toContain("missing.rs");
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// extractDomainFacts — list of applies → deduplicated domain facts
// ═══════════════════════════════════════════════════════════════════

describe("extractDomainFacts", () => {
  it("should extract facts from applies with outcomes", () => {
    const applies = [
      { tool_outcome: { type: "FileCreated", path: "/a.rs" } },
      { tool_outcome: { type: "FileRead", path: "/b.rs" } },
      { tool_outcome: null },
      { tool_outcome: { type: "CommandExecuted", command: "cargo test", succeeded: true } },
    ];
    const facts = extractDomainFacts(applies as any);
    expect(facts).toHaveLength(3);
    expect(facts[0]!.kind).toBe("created");
    expect(facts[1]!.kind).toBe("read");
    expect(facts[2]!.kind).toBe("command_ok");
  });

  it("should deduplicate repeated file reads", () => {
    const applies = [
      { tool_outcome: { type: "FileRead", path: "/same.rs" } },
      { tool_outcome: { type: "FileRead", path: "/same.rs" } },
      { tool_outcome: { type: "FileRead", path: "/other.rs" } },
    ];
    const facts = extractDomainFacts(applies as any);
    expect(facts).toHaveLength(2);
  });

  it("should deduplicate repeated searches", () => {
    const applies = [
      { tool_outcome: { type: "SearchPerformed", pattern: "TODO", source: "filesystem" } },
      { tool_outcome: { type: "SearchPerformed", pattern: "TODO", source: "filesystem" } },
    ];
    const facts = extractDomainFacts(applies as any);
    expect(facts).toHaveLength(1);
  });

  it("should return empty for no outcomes", () => {
    const applies = [
      { tool_outcome: null },
      { tool_outcome: undefined },
    ];
    expect(extractDomainFacts(applies as any)).toHaveLength(0);
  });

  it("should preserve order: created first, then modified, then reads, then commands", () => {
    const applies = [
      { tool_outcome: { type: "CommandExecuted", command: "ls", succeeded: true } },
      { tool_outcome: { type: "FileRead", path: "/a.rs" } },
      { tool_outcome: { type: "FileCreated", path: "/b.rs" } },
      { tool_outcome: { type: "FileModified", path: "/c.rs" } },
    ];
    const facts = extractDomainFacts(applies as any);
    const kinds = facts.map(f => f.kind);
    expect(kinds).toEqual(["created", "modified", "read", "command_ok"]);
  });
});
