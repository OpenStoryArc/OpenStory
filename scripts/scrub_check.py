#!/usr/bin/env python3
"""Scrub check — scan for sensitive data before committing.

Checks for: API keys, private IPs, personal paths, tokens, credentials.
Run before any commit that includes fixtures, session data, or scripts.

Usage:
    python3 scripts/scrub_check.py
    python3 scripts/scrub_check.py --fix   # sanitize fixture files in-place
"""

import json
import os
import re
import sys

SENSITIVE_PATTERNS = [
    (r"sk-ant-[a-zA-Z0-9\-_]+", "Anthropic API key"),
    (r"sk-[a-zA-Z0-9]{20,}", "API key"),
    (r"Bearer [a-zA-Z0-9\-_.]+", "Bearer token"),
    (r"192\.168\.\d+\.\d+", "Private IP (192.168.x.x)"),
    (r"172\.(1[6-9]|2\d|3[01])\.\d+\.\d+", "Private IP (172.16-31.x.x)"),
    (r"100\.\d+\.\d+\.\d+", "Tailscale IP (100.x.x.x)"),
    (r"10\.\d+\.\d+\.\d+", "Private IP (10.x.x.x)"),
]

PERSONAL_PATH_PATTERN = r"/Users/[a-zA-Z0-9_\-]+"

SCAN_DIRS = [
    "rs/tests/fixtures",
    "scripts",
    "docs/research",
    "story.html",
]

SKIP_EXTENSIONS = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".woff", ".woff2", ".db"}
SKIP_DIRS = {"node_modules", "target", ".git", "data", "data.old"}


def scan_file(path):
    issues = []
    try:
        with open(path, "r", errors="replace") as f:
            for lineno, line in enumerate(f, 1):
                for pattern, desc in SENSITIVE_PATTERNS:
                    for match in re.finditer(pattern, line):
                        issues.append((path, lineno, desc, match.group()[:40]))
                for match in re.finditer(PERSONAL_PATH_PATTERN, line):
                    issues.append((path, lineno, "Personal path", match.group()))
    except (UnicodeDecodeError, IsADirectoryError):
        pass
    return issues


def scan_json_fixture(path):
    """Deep scan JSON fixtures for sensitive data in nested values."""
    issues = []
    try:
        with open(path) as f:
            data = json.load(f)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return issues

    def walk(obj, path_str=""):
        if isinstance(obj, str):
            for pattern, desc in SENSITIVE_PATTERNS:
                if re.search(pattern, obj):
                    issues.append((path, 0, f"{desc} in JSON value", f"{path_str}: {obj[:40]}"))
            if re.search(PERSONAL_PATH_PATTERN, obj):
                issues.append((path, 0, "Personal path in JSON value", f"{path_str}: {obj[:60]}"))
        elif isinstance(obj, dict):
            for k, v in obj.items():
                walk(v, f"{path_str}.{k}")
        elif isinstance(obj, list):
            for i, v in enumerate(obj):
                walk(v, f"{path_str}[{i}]")

    walk(data)
    return issues


def sanitize_fixture(path):
    """Replace personal paths in JSON fixture files."""
    with open(path) as f:
        content = f.read()

    original = content
    content = re.sub(r"/Users/[a-zA-Z0-9_\-]+", "/Users/user", content)
    content = re.sub(r"sk-ant-[a-zA-Z0-9\-_]+", "sk-ant-REDACTED", content)

    if content != original:
        with open(path, "w") as f:
            f.write(content)
        return True
    return False


def main():
    fix_mode = "--fix" in sys.argv
    all_issues = []

    for scan_dir in SCAN_DIRS:
        if not os.path.exists(scan_dir):
            continue
        if os.path.isfile(scan_dir):
            all_issues.extend(scan_file(scan_dir))
            continue
        for root, dirs, files in os.walk(scan_dir):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for fname in files:
                ext = os.path.splitext(fname)[1].lower()
                if ext in SKIP_EXTENSIONS:
                    continue
                fpath = os.path.join(root, fname)
                all_issues.extend(scan_file(fpath))
                if fpath.endswith(".json"):
                    all_issues.extend(scan_json_fixture(fpath))

    if all_issues:
        print(f"Found {len(all_issues)} issues:\n")
        for path, lineno, desc, value in all_issues:
            loc = f"{path}:{lineno}" if lineno > 0 else path
            print(f"  {desc}")
            print(f"    {loc}")
            print(f"    {value}")
            print()

        if fix_mode:
            fixed = 0
            for scan_dir in SCAN_DIRS:
                if not os.path.exists(scan_dir):
                    continue
                if os.path.isfile(scan_dir):
                    continue
                for root, dirs, files in os.walk(scan_dir):
                    dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
                    for fname in files:
                        if fname.endswith(".json"):
                            fpath = os.path.join(root, fname)
                            if sanitize_fixture(fpath):
                                print(f"  Fixed: {fpath}")
                                fixed += 1
            print(f"\nFixed {fixed} files.")
        else:
            print("Run with --fix to sanitize fixture files.")
        sys.exit(1)
    else:
        print("No sensitive data found.")
        sys.exit(0)


if __name__ == "__main__":
    main()
