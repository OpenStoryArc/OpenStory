import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { detectLanguage } from "@/lib/detect-language";

describe("detectLanguage", () => {
  // Boundary table: every input partition in one place
  const cases: [string, Parameters<typeof detectLanguage>[0], string][] = [
    ["explicit lang wins",                    { lang: "rust" },                              "rust"],
    ["TypeScript by extension",               { filePath: "/src/app.ts" },                   "typescript"],
    ["TSX by extension",                      { filePath: "Component.tsx" },                  "tsx"],
    ["Rust by extension",                     { filePath: "main.rs" },                        "rust"],
    ["Python by extension",                   { filePath: "script.py" },                      "python"],
    ["JSON by extension",                     { filePath: "package.json" },                   "json"],
    ["Bash tool name",                        { toolName: "Bash" },                           "bash"],
    ["Grep tool name",                        { toolName: "Grep" },                           "regex"],
    ["unknown defaults to text",              {},                                              "text"],
    ["explicit lang overrides file ext",      { lang: "diff", filePath: "foo.ts" },           "diff"],
    ["Dockerfile special name",              { filePath: "/app/Dockerfile" },                 "docker"],
    ["Justfile special name",                { filePath: "justfile" },                        "makefile"],
    ["Windows path backslashes",             { filePath: "C:\\Users\\foo\\bar.rs" },          "rust"],
    ["CSS by extension",                     { filePath: "styles.css" },                      "css"],
    ["YAML by extension",                    { filePath: "config.yaml" },                     "yaml"],
    ["YML variant",                          { filePath: "ci.yml" },                          "yaml"],
    ["TOML by extension",                    { filePath: "Cargo.toml" },                      "toml"],
    ["Markdown by extension",                { filePath: "README.md" },                       "markdown"],
    ["Shell by extension",                   { filePath: "deploy.sh" },                       "bash"],
    ["Glob tool name",                       { toolName: "Glob" },                            "bash"],
    ["unknown tool falls through to text",   { toolName: "SomeUnknown" },                     "text"],
    ["JSONL by extension",                   { filePath: "events.jsonl" },                     "json"],
    ["file ext beats tool name",             { filePath: "app.py", toolName: "Bash" },        "python"],
  ];

  it.each(cases)("%s: %j => %s", (_label, input, expected) => {
    scenario(
      () => input,
      (opts) => detectLanguage(opts),
      (result) => expect(result).toBe(expected),
    );
  });
});
