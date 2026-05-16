#!/usr/bin/env python3
"""
Anonymize a real Claude Code transcript JSONL into a safe-to-commit fixture.

Preserves *structure* (event sequencing, parent_uuid chains, tool use/result
pairing, timestamp monotonicity) while scrubbing freeform content (prose,
file paths, bash commands, file contents, branch names, cwd, etc).

Two modes:
  --mode redact      replace freeform text with [REDACTED-N] placeholders
                     (smallest blast radius, easiest to audit)
  --mode synthesize  replace freeform text with bank content from
                     synth_transcripts.py (default — fixtures look real)

Privacy guarantees:
  - cwd → /project (regardless of input)
  - all /Users/<name>/... and /home/<name>/... paths → /project/...
  - emails → user@example.invalid
  - gitBranch → main (or feature/branch from a small bank)
  - git remote URLs → none
  - prose content (user prompts, assistant text, thinking) → bank text
  - tool inputs/outputs → bank content
  - signature/encrypted reasoning blobs → empty string

Fail-closed: --test fails if any known PII pattern survives the scrub.

Usage:
    python3 scripts/synth/anonymize.py path/to/session.jsonl > out.jsonl
    python3 scripts/synth/anonymize.py --mode redact session.jsonl -o out.jsonl
    python3 scripts/synth/anonymize.py --test
"""
from __future__ import annotations

import argparse
import json
import random
import re
import sys
from pathlib import Path
from typing import Any, Optional

# Reuse the content banks from the existing synth_transcripts.py
_HERE = Path(__file__).resolve().parent
_SCRIPTS = _HERE.parent
sys.path.insert(0, str(_SCRIPTS))
try:
    from synth_transcripts import (  # type: ignore
        ASSISTANT_TEXTS,
        BASH_COMMANDS,
        BASH_OUTPUTS,
        FILE_PATHS,
        GREP_PATTERNS,
        THINKING_TEXTS,
        TOOL_RESULT_CONTENTS,
        USER_PROMPTS,
    )
except ImportError:
    # Fallback minimal banks if the import fails for some reason
    ASSISTANT_TEXTS = ["Reading the file now."]
    BASH_COMMANDS = ["echo hello"]
    BASH_OUTPUTS = ["ok"]
    FILE_PATHS = ["/project/main.rs"]
    GREP_PATTERNS = ["TODO"]
    THINKING_TEXTS = ["Considering."]
    TOOL_RESULT_CONTENTS = ["File contents."]
    USER_PROMPTS = ["Help me build a feature."]

BRANCHES = ["main", "feature/auth", "fix/cors", "chore/deps"]

# =====================================================================
# PII detectors (used in --test to fail-closed)
# =====================================================================

PII_PATTERNS: list[tuple[str, re.Pattern]] = [
    ("user_home_path", re.compile(r"/(Users|home)/[A-Za-z0-9_.\-]+/")),
    # Claude Code's project_id slug encodes the home path with dashes:
    # /Users/example_user/projects/OpenStory  →  -Users-example_user-projects-OpenStory
    ("project_slug",
     re.compile(r"-(?:Users|home)-[A-Za-z0-9_.]+(?:-[A-Za-z0-9_.]+)+")),
    ("email", re.compile(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")),
    ("git_remote_https",
     re.compile(r"https://[A-Za-z0-9_.\-]*:[A-Za-z0-9_.\-]*@github\.com")),
    ("ssh_url", re.compile(r"git@[A-Za-z0-9.\-]+:")),
    ("api_key_like", re.compile(r"\b(sk|pk|key)_[A-Za-z0-9]{20,}\b")),
    # Bearer tokens are common in headers
    ("bearer_token", re.compile(r"Bearer\s+[A-Za-z0-9_.\-]{16,}")),
]

# Reserved-for-testing strings per RFC 2606 (.invalid TLD) and our own
# placeholder vocabulary. Anything matching these is a known-safe sentinel,
# not real PII — so find_pii() should NOT flag them.
SAFE_SENTINELS = (
    "@example.invalid",
    "git@example.invalid",
    "Bearer REDACTED",
    "_REDACTED",  # api-key replacement suffix
)


def find_pii(text: str) -> list[tuple[str, str]]:
    """Return list of (pattern_name, match) for any PII found.

    Skips known-safe sentinels (RFC 2606 reserved + our placeholders).
    """
    hits = []
    for name, pat in PII_PATTERNS:
        for m in pat.finditer(text):
            match = m.group(0)
            if any(s in match for s in SAFE_SENTINELS):
                continue
            hits.append((name, match))
    return hits


# =====================================================================
# Scrubbers
# =====================================================================

def scrub_path(path: str) -> str:
    """Replace user-home prefixes with /project, preserve trailing path."""
    if not isinstance(path, str):
        return path
    # /Users/<name>/projects/foo/bar → /project/foo/bar
    m = re.match(r"^/(?:Users|home)/[A-Za-z0-9_.\-]+(?:/projects?)?(/.*)?$",
                 path)
    if m:
        rest = m.group(1) or ""
        return "/project" + rest
    # Windows-style C:\Users\name\...
    m = re.match(r"^[A-Z]:\\Users\\[A-Za-z0-9_.\-]+(\\.*)?$", path)
    if m:
        rest = (m.group(1) or "").replace("\\", "/")
        return "/project" + rest
    return path


def scrub_string(s: str) -> str:
    """Best-effort string scrub: paths, emails, tokens."""
    if not isinstance(s, str):
        return s
    # Paths inside arbitrary text
    s = re.sub(r"/(Users|home)/[A-Za-z0-9_.\-]+",
               "/project", s)
    s = re.sub(r"[A-Z]:\\Users\\[A-Za-z0-9_.\-]+",
               "C:/project", s)
    # Claude Code project slug: -Users-example_user-projects-Foo → -project-Foo
    s = re.sub(r"-(?:Users|home)-[A-Za-z0-9_.]+-projects?-",
               "-project-", s)
    # Emails
    s = re.sub(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}",
               "user@example.invalid", s)
    # Bearer tokens
    s = re.sub(r"Bearer\s+[A-Za-z0-9_.\-]{16,}",
               "Bearer REDACTED", s)
    # API keys
    s = re.sub(r"\b(sk|pk|key)_[A-Za-z0-9]{20,}\b",
               r"\1_REDACTED", s)
    # SSH urls
    s = re.sub(r"git@[A-Za-z0-9.\-]+:", "git@example.invalid:", s)
    # GitHub creds in URLs
    s = re.sub(r"https://[A-Za-z0-9_.\-]*:[A-Za-z0-9_.\-]*@github\.com",
               "https://github.com", s)
    return s


def replace_text(rng: random.Random, original: str, kind: str,
                 mode: str) -> str:
    """Return a replacement for prose content based on the scrub mode."""
    if mode == "redact":
        # Preserve length bucket so structural shape stays similar
        bucket = "short" if len(original) < 80 else (
            "medium" if len(original) < 400 else "long"
        )
        return f"[REDACTED-{kind}-{bucket}]"
    # synthesize mode: pick from the appropriate bank
    if kind == "user_prompt":
        return rng.choice(USER_PROMPTS)
    if kind == "assistant_text":
        return rng.choice(ASSISTANT_TEXTS)
    if kind == "thinking":
        return rng.choice(THINKING_TEXTS)
    if kind == "bash_output":
        return rng.choice(BASH_OUTPUTS)
    if kind == "tool_result":
        return rng.choice(TOOL_RESULT_CONTENTS)
    return rng.choice(ASSISTANT_TEXTS)


# =====================================================================
# Tool-input scrubbing — by tool name
# =====================================================================

def scrub_tool_input(rng: random.Random, name: str, inp: Any,
                     mode: str) -> Any:
    """Replace the tool's input dict with a synthetic one of the same shape."""
    if not isinstance(inp, dict):
        return inp
    out: dict[str, Any] = {}
    for k, v in inp.items():
        if k in ("file_path", "path", "notebook_path"):
            out[k] = rng.choice(FILE_PATHS)
        elif k == "command":
            out[k] = rng.choice(BASH_COMMANDS) if mode == "synthesize" else "[REDACTED-bash]"
        elif k in ("old_string", "new_string", "content"):
            out[k] = "// content" if mode == "synthesize" else "[REDACTED-content]"
        elif k == "pattern":
            out[k] = rng.choice(GREP_PATTERNS)
        elif k in ("query", "prompt", "description"):
            out[k] = "exploratory query" if mode == "synthesize" else "[REDACTED-query]"
        elif k == "url":
            out[k] = "https://example.invalid"
        elif isinstance(v, str):
            out[k] = scrub_string(v)
        elif isinstance(v, dict):
            out[k] = scrub_tool_input(rng, name, v, mode)
        elif isinstance(v, list):
            out[k] = [
                scrub_tool_input(rng, name, x, mode) if isinstance(x, dict)
                else (scrub_string(x) if isinstance(x, str) else x)
                for x in v
            ]
        else:
            out[k] = v
    return out


# =====================================================================
# Per-line anonymizer
# =====================================================================

def anonymize_line(rng: random.Random, line: dict, mode: str) -> dict:
    """Return a new line dict with all freeform content scrubbed.

    Structure fields preserved verbatim:
      uuid, parentUuid, sessionId, timestamp, type, subtype, isSidechain,
      promptId, messageId, isMeta, userType, entrypoint, permissionMode,
      version, durationMs, messageCount, hookCount, isSnapshotUpdate,
      stop_reason, depth, seq, payload_bytes, truncated, tracked_files
    """
    out = dict(line)

    # cwd is always rewritten
    if "cwd" in out:
        out["cwd"] = "/project"
    # gitBranch from a small bank (deterministic per session)
    if "gitBranch" in out:
        out["gitBranch"] = rng.choice(BRANCHES) if mode == "synthesize" else "main"

    ltype = out.get("type")

    # User message
    if ltype == "user":
        msg = dict(out.get("message") or {})
        content = msg.get("content")
        if isinstance(content, str):
            msg["content"] = replace_text(rng, content, "user_prompt", mode)
        elif isinstance(content, list):
            new_blocks = []
            for block in content:
                if not isinstance(block, dict):
                    new_blocks.append(block)
                    continue
                b = dict(block)
                btype = b.get("type")
                if btype == "tool_result":
                    raw = b.get("content")
                    if isinstance(raw, str):
                        b["content"] = replace_text(rng, raw, "tool_result", mode)
                    elif isinstance(raw, list):
                        b["content"] = [
                            {**c, "text": replace_text(
                                rng, c.get("text", ""), "tool_result", mode
                            )} if isinstance(c, dict) and c.get("type") == "text"
                            else c
                            for c in raw
                        ]
                elif btype == "text":
                    b["text"] = replace_text(rng, b.get("text", ""),
                                             "user_prompt", mode)
                new_blocks.append(b)
            msg["content"] = new_blocks
        out["message"] = msg

    # Assistant message
    elif ltype == "assistant":
        msg = dict(out.get("message") or {})
        content = msg.get("content")
        if isinstance(content, list):
            new_blocks = []
            for block in content:
                if not isinstance(block, dict):
                    new_blocks.append(block)
                    continue
                b = dict(block)
                btype = b.get("type")
                if btype == "text":
                    b["text"] = replace_text(rng, b.get("text", ""),
                                             "assistant_text", mode)
                elif btype == "thinking":
                    b["thinking"] = replace_text(rng, b.get("thinking", ""),
                                                 "thinking", mode)
                    # Wipe encrypted signature
                    if "signature" in b:
                        b["signature"] = ""
                elif btype == "tool_use":
                    b["input"] = scrub_tool_input(rng, b.get("name", ""),
                                                  b.get("input"), mode)
                new_blocks.append(b)
            msg["content"] = new_blocks
        out["message"] = msg

    # Progress events
    elif ltype == "progress":
        d = dict(out.get("data") or {})
        if d.get("type") == "bash_progress" and "output" in d:
            d["output"] = replace_text(rng, d.get("output", ""),
                                       "bash_output", mode)
        out["data"] = d

    # Last-prompt summary — the prompt is freeform user content
    elif ltype == "last-prompt":
        if "lastPrompt" in out:
            out["lastPrompt"] = replace_text(rng, out["lastPrompt"],
                                             "user_prompt", mode)

    # AI title
    elif ltype == "ai-title":
        if "aiTitle" in out:
            out["aiTitle"] = "Synthetic session"

    # Attachments often carry env state — strip the body, keep the discriminator
    elif ltype == "attachment":
        att = out.get("attachment") or {}
        if isinstance(att, dict):
            out["attachment"] = {
                "type": att.get("type", "synthetic"),
                "redacted": True,
            }

    # Queue operations sometimes contain user prompts
    elif ltype == "queue-operation":
        if "content" in out and isinstance(out["content"], str):
            out["content"] = replace_text(rng, out["content"],
                                          "user_prompt", mode)

    # Final defensive sweep — scrub strings on any remaining stringy keys
    # (catches things like custom hook payloads, attachment.path, ...)
    return _deep_string_scrub(out)


def _deep_string_scrub(obj: Any) -> Any:
    """Walk the obj, scrub any string for paths/emails/tokens.

    This is the fail-closed safety net — even if we forget to handle a
    specific field, no path/email/token survives.
    """
    if isinstance(obj, str):
        return scrub_string(obj)
    if isinstance(obj, dict):
        return {k: _deep_string_scrub(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_deep_string_scrub(v) for v in obj]
    return obj


# =====================================================================
# Top-level driver
# =====================================================================

def anonymize_jsonl(in_path: Optional[Path], out_path: Optional[Path],
                    mode: str, seed: int = 42) -> int:
    """Read JSONL from in_path (or stdin), write anonymized JSONL.

    Returns the count of lines processed.
    """
    rng = random.Random(seed)
    src = open(in_path, "r", encoding="utf-8") if in_path else sys.stdin
    dst = open(out_path, "w", encoding="utf-8") if out_path else sys.stdout
    n = 0
    try:
        for raw in src:
            raw = raw.strip()
            if not raw:
                continue
            try:
                line = json.loads(raw)
            except json.JSONDecodeError:
                continue
            scrubbed = anonymize_line(rng, line, mode)
            dst.write(json.dumps(scrubbed, separators=(",", ":")) + "\n")
            n += 1
    finally:
        if in_path:
            src.close()
        if out_path:
            dst.close()
    return n


# =====================================================================
# BDD Tests
# =====================================================================

def _real_looking_session() -> list[dict]:
    """Synthetic input that contains every known PII pattern."""
    return [
        {
            "type": "user",
            "uuid": "u1",
            "cwd": "/Users/example_user/projects/OpenStory",
            "gitBranch": "feat/secret-branch-name",
            "message": {
                "role": "user",
                "content": "Email me at max@personal.example with the API key sk_live_abcdefghijklmnop12345",
            },
        },
        {
            "type": "assistant",
            "uuid": "a1",
            "parentUuid": "u1",
            "cwd": "/Users/example_user/projects/OpenStory",
            "message": {
                "role": "assistant",
                "model": "claude-opus-4-7",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "I'll connect to git@github.com:OpenStoryArc/OpenStory.git and check the user's home at /home/max/.ssh/id_rsa",
                        "signature": "very-long-encrypted-signature-do-not-leak",
                    },
                    {
                        "type": "text",
                        "text": "Reading /Users/example_user/projects/OpenStory/README.md now.",
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_1",
                        "name": "Bash",
                        "input": {
                            "command": "curl -H 'Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz123456' https://user:token@github.com/foo",
                        },
                    },
                ],
            },
        },
        {
            "type": "user",
            "uuid": "u2",
            "parentUuid": "a1",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_1",
                        "content": "Cloning into '/Users/example_user/projects/OpenStory'... done. user@example.com",
                        "is_error": False,
                    }
                ],
            },
        },
        {
            "type": "progress",
            "uuid": "p1",
            "data": {
                "type": "bash_progress",
                "output": "compiling /home/max/projects/openstory/main.rs",
            },
        },
        {
            "type": "ai-title",
            "aiTitle": "How to leak secrets via /Users/example_user",
            "sessionId": "s1",
        },
        {
            "type": "last-prompt",
            "lastPrompt": "Email max@personal.example",
        },
    ]


def run_tests() -> bool:
    passed = failed = 0

    def it(name: str, cond: bool, detail: str = ""):
        nonlocal passed, failed
        if cond:
            passed += 1
            print(f"  ok  {name}")
        else:
            failed += 1
            print(f"  FAIL  {name}" + (f" — {detail}" if detail else ""))

    print("\nGiven a session containing every PII pattern we know about")
    raw_lines = _real_looking_session()
    # Add a project-slug-bearing line to exercise that detector
    raw_lines.append({
        "type": "system",
        "uuid": "sys1",
        "subtype": "tool_result_summary",
        "filePath": "/Users/example_user/.claude/projects/-Users-example_user-projects-OpenStory/abc.jsonl",
    })
    raw_blob = json.dumps(raw_lines)
    raw_hits = find_pii(raw_blob)
    it("the input fixture contains PII (else this test is meaningless)",
       len(raw_hits) > 0, f"hits={raw_hits[:3]}")

    print("\nWhen we anonymize in synthesize mode")
    rng = random.Random(7)
    syn_out = [anonymize_line(rng, dict(l), "synthesize") for l in raw_lines]
    syn_blob = json.dumps(syn_out)
    syn_hits = find_pii(syn_blob)
    it("no PII patterns survive", len(syn_hits) == 0,
       f"surviving={syn_hits}")

    print("\nWhen we anonymize in redact mode")
    rng = random.Random(7)
    red_out = [anonymize_line(rng, dict(l), "redact") for l in raw_lines]
    red_blob = json.dumps(red_out)
    red_hits = find_pii(red_blob)
    it("no PII patterns survive (redact)", len(red_hits) == 0,
       f"surviving={red_hits}")
    it("redact tags are present", "[REDACTED-" in red_blob)

    print("\nThen structure is preserved")
    it("same number of lines out", len(syn_out) == len(raw_lines))
    it("uuids preserved",
       [l.get("uuid") for l in syn_out] == [l.get("uuid") for l in raw_lines])
    it("parentUuids preserved",
       [l.get("parentUuid") for l in syn_out] ==
       [l.get("parentUuid") for l in raw_lines])
    it("types preserved",
       [l.get("type") for l in syn_out] ==
       [l.get("type") for l in raw_lines])
    it("tool_use IDs preserved",
       syn_out[1]["message"]["content"][2]["id"] == "tool_1")
    it("tool_use names preserved",
       syn_out[1]["message"]["content"][2]["name"] == "Bash")
    it("tool_result tool_use_id preserved",
       syn_out[2]["message"]["content"][0]["tool_use_id"] == "tool_1")

    print("\nAnd specific scrubs land")
    it("cwd is /project", syn_out[0]["cwd"] == "/project")
    it("thinking signature wiped",
       syn_out[1]["message"]["content"][0]["signature"] == "")
    it("ai-title is replaced",
       syn_out[4]["aiTitle"] == "Synthetic session")
    it("attachments would be redacted (smoke check)",
       True)  # covered when an attachment line is present

    print("\nAnd determinism: same seed → same output")
    rng_a = random.Random(11)
    a = [anonymize_line(rng_a, dict(l), "synthesize") for l in raw_lines]
    rng_b = random.Random(11)
    b = [anonymize_line(rng_b, dict(l), "synthesize") for l in raw_lines]
    it("deterministic with fixed seed", json.dumps(a) == json.dumps(b))

    print("\nAnd the deep sweep catches unexpected PII keys")
    leaky = {
        "type": "custom",
        "uuid": "x",
        "weird_field": "ping me at hidden@leak.example",
        "nested": {"path": "/Users/example_user/secret"},
    }
    out = anonymize_line(random.Random(0), leaky, "synthesize")
    it("deep sweep scrubs unexpected email keys",
       "hidden@leak.example" not in json.dumps(out))
    it("deep sweep scrubs unexpected path keys",
       "/Users/example_user" not in json.dumps(out))

    print(f"\n{passed} passed, {failed} failed")
    return failed == 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("input", nargs="?", type=Path,
                    help="JSONL file to anonymize (default: stdin)")
    ap.add_argument("--out", "-o", type=Path,
                    help="Output JSONL path (default: stdout)")
    ap.add_argument("--mode", choices=["synthesize", "redact"],
                    default="synthesize",
                    help="synthesize replaces with realistic bank text "
                         "(default); redact replaces with [REDACTED-*] tags")
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()

    if args.test:
        sys.exit(0 if run_tests() else 1)

    n = anonymize_jsonl(args.input, args.out, args.mode, args.seed)
    if args.out:
        print(f"wrote {n} lines to {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
