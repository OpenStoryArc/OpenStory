/**
 * Detect the Prism language token for syntax highlighting.
 * Pure function: inputs in, language string out.
 */

const EXT_MAP: Record<string, string> = {
  ".ts": "typescript",
  ".tsx": "tsx",
  ".js": "javascript",
  ".jsx": "jsx",
  ".rs": "rust",
  ".py": "python",
  ".json": "json",
  ".jsonl": "json",
  ".yaml": "yaml",
  ".yml": "yaml",
  ".toml": "toml",
  ".md": "markdown",
  ".sh": "bash",
  ".bash": "bash",
  ".zsh": "bash",
  ".css": "css",
  ".html": "markup",
  ".sql": "sql",
  ".diff": "diff",
  ".patch": "diff",
  ".dockerfile": "docker",
  ".xml": "xml",
  ".graphql": "graphql",
  ".go": "go",
  ".java": "java",
  ".rb": "ruby",
  ".c": "c",
  ".cpp": "cpp",
  ".h": "c",
};

const TOOL_MAP: Record<string, string> = {
  Bash: "bash",
  Grep: "regex",
  Glob: "bash",
};

export function detectLanguage(options: {
  lang?: string;
  filePath?: string;
  toolName?: string;
}): string {
  // Explicit lang wins
  if (options.lang) return options.lang;

  // File extension
  if (options.filePath) {
    const lastDot = options.filePath.lastIndexOf(".");
    if (lastDot !== -1) {
      const ext = options.filePath.slice(lastDot).toLowerCase();
      const lang = EXT_MAP[ext];
      if (lang) return lang;
    }
    // Special files without extensions
    const name = options.filePath.split(/[/\\]/).pop()?.toLowerCase() ?? "";
    if (name === "dockerfile") return "docker";
    if (name === "justfile" || name === "makefile") return "makefile";
  }

  // Tool name fallback
  if (options.toolName) {
    const lang = TOOL_MAP[options.toolName];
    if (lang) return lang;
  }

  return "text";
}
