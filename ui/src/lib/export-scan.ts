import { bundleText, type ReelBundle } from "@/lib/reel-bundle";

export interface Finding {
  readonly slideId: string;
  readonly family: string;
  readonly excerpt: string;
}

const FAMILIES: readonly { family: string; re: RegExp }[] = [
  { family: "aws-key", re: /\bAKIA[0-9A-Z]{16}\b/ },
  { family: "secret-assign", re: /\b(api[_-]?key|token|secret|password|passwd)\b\s*[:=]\s*["']?[^\s"']{8,}/i },
  { family: "pem", re: /-----BEGIN [A-Z ]*PRIVATE KEY-----/ },
  { family: "bearer", re: /\bBearer\s+[A-Za-z0-9\-_.=]{20,}/ },
  { family: "email", re: /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/ },
  { family: "home-path", re: /\/(Users|home)\/[A-Za-z0-9._-]+\// },
];

export function scanBundle(bundle: ReelBundle): Finding[] {
  const out: Finding[] = [];
  for (const row of bundleText(bundle)) {
    for (const { family, re } of FAMILIES) {
      const m = re.exec(row.text);
      if (!m) continue;
      const at = Math.max(0, m.index - 20);
      out.push({ slideId: row.slideId, family, excerpt: row.text.slice(at, at + 80).trim() });
    }
  }
  return out;
}
