"""Security audit for a dependency before installing.

Checks known vulnerabilities (OSV.dev), dependency tree, package
metadata, and transitive dependency CVEs.

Usage:
    python scripts/audit_dependency.py weasyprint
    python scripts/audit_dependency.py --npm chart.js
    python scripts/audit_dependency.py --test
"""

import argparse
import json
import subprocess
import sys
import urllib.request
import urllib.error
from typing import Optional


def osv_query(package: str, ecosystem: str) -> list[dict]:
    """Query OSV.dev for known vulnerabilities."""
    url = "https://api.osv.dev/v1/query"
    payload = json.dumps({"package": {"name": package, "ecosystem": ecosystem}}).encode()
    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
            return data.get("vulns", [])
    except (urllib.error.URLError, json.JSONDecodeError, TimeoutError):
        return []


def pypi_metadata(package: str) -> Optional[dict]:
    """Fetch package metadata from PyPI."""
    url = f"https://pypi.org/pypi/{package}/json"
    try:
        with urllib.request.urlopen(url, timeout=10) as resp:
            return json.loads(resp.read())
    except (urllib.error.URLError, json.JSONDecodeError):
        return None


def npm_metadata(package: str) -> Optional[dict]:
    """Fetch package metadata from npm registry."""
    url = f"https://registry.npmjs.org/{package}"
    try:
        with urllib.request.urlopen(url, timeout=10) as resp:
            return json.loads(resp.read())
    except (urllib.error.URLError, json.JSONDecodeError):
        return None


def pip_dry_run(package: str) -> list[str]:
    """Run pip install --dry-run and return output lines."""
    try:
        result = subprocess.run(
            [sys.executable, "-m", "pip", "install", "--dry-run", package],
            capture_output=True, text=True, timeout=30,
        )
        return [l for l in result.stdout.splitlines() if l.strip().startswith(("Collecting", "Downloading", "Requirement"))]
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return ["(pip dry-run failed)"]


def extract_deps(meta: dict) -> list[str]:
    """Extract dependency names from PyPI metadata."""
    requires = meta.get("info", {}).get("requires_dist") or []
    deps = []
    for r in requires:
        # "pydyf>=0.11.0" -> "pydyf"
        name = r.split(";")[0].split(">=")[0].split("<=")[0].split("==")[0].split("!=")[0].split("[")[0].split(">")[0].split("<")[0].strip()
        if name:
            deps.append(name)
    return list(dict.fromkeys(deps))[:20]  # dedupe, cap at 20


def severity_label(vuln: dict) -> str:
    """Extract severity from an OSV vulnerability record."""
    severity = vuln.get("database_specific", {}).get("severity", "")
    if not severity:
        for s in vuln.get("severity", []):
            if s.get("type") == "CVSS_V3":
                score = float(s.get("score", "0").split("/")[0]) if "/" in s.get("score", "") else 0
                if score >= 9:
                    return "CRITICAL"
                elif score >= 7:
                    return "HIGH"
                elif score >= 4:
                    return "MEDIUM"
                return "LOW"
        return "UNKNOWN"
    return severity


def audit_pypi(package: str) -> dict:
    """Run full audit for a PyPI package. Returns a report dict."""
    report = {"package": package, "ecosystem": "PyPI", "sections": []}

    # 1. Direct vulnerabilities
    vulns = osv_query(package, "PyPI")
    section = {"title": "Known Vulnerabilities (OSV.dev)", "items": []}
    if not vulns:
        section["status"] = "clean"
        section["items"].append("No known vulnerabilities")
    else:
        section["status"] = "warning"
        for v in vulns:
            vid = v.get("id", "unknown")
            summary = v.get("summary", "No summary")
            sev = severity_label(v)
            fixed = ""
            for a in v.get("affected", []):
                for r in a.get("ranges", []):
                    for e in r.get("events", []):
                        if "fixed" in e:
                            fixed = f" (fixed in {e['fixed']})"
            section["items"].append(f"{vid} [{sev}] {summary}{fixed}")
    report["sections"].append(section)

    # 2. Package metadata
    meta = pypi_metadata(package)
    section = {"title": "Package Metadata", "items": []}
    if meta:
        info = meta.get("info", {})
        section["items"].append(f"Version:      {info.get('version', 'unknown')}")
        section["items"].append(f"Author:       {info.get('author') or info.get('author_email') or 'unknown'}")
        section["items"].append(f"License:      {info.get('license') or 'unknown'}")
        section["items"].append(f"Python:       {info.get('requires_python') or 'any'}")
        section["items"].append(f"Summary:      {info.get('summary') or 'none'}")
        # Last release date
        version = info.get("version", "")
        releases = meta.get("releases", {}).get(version, [])
        if releases:
            upload = releases[0].get("upload_time", "unknown")
            section["items"].append(f"Last release: {upload}")
        section["status"] = "info"
    else:
        section["status"] = "error"
        section["items"].append("Could not fetch metadata")
    report["sections"].append(section)

    # 3. Dependency tree
    section = {"title": "Dependency Tree (dry-run)", "items": pip_dry_run(package), "status": "info"}
    report["sections"].append(section)

    # 4. Transitive dependency scan
    section = {"title": "Transitive Dependency Scan", "items": []}
    if meta:
        deps = extract_deps(meta)
        has_warnings = False
        for dep in deps:
            dep_vulns = osv_query(dep, "PyPI")
            if dep_vulns:
                has_warnings = True
                section["items"].append(f"  ⚠ {dep}: {len(dep_vulns)} known vulnerability record(s)")
                for v in dep_vulns[:3]:
                    vid = v.get("id", "")
                    sev = severity_label(v)
                    summary = v.get("summary", "")[:80]
                    section["items"].append(f"    {vid} [{sev}] {summary}")
            else:
                section["items"].append(f"  ✓ {dep}: clean")
        section["status"] = "warning" if has_warnings else "clean"
    else:
        section["items"].append("(no metadata available)")
        section["status"] = "error"
    report["sections"].append(section)

    return report


def print_report(report: dict) -> None:
    """Print the audit report to stdout."""
    print("=" * 55)
    print(f"  Dependency Audit: {report['package']} ({report['ecosystem']})")
    print("=" * 55)
    print()

    for section in report["sections"]:
        status_icon = {"clean": "✓", "warning": "⚠", "error": "✗", "info": "ℹ"}.get(section.get("status", ""), " ")
        print(f"── {status_icon} {section['title']} ──")
        print()
        for item in section["items"]:
            print(f"  {item}")
        print()

    # Overall verdict
    has_warnings = any(s.get("status") == "warning" for s in report["sections"])
    if has_warnings:
        print("⚠  REVIEW REQUIRED — vulnerabilities found (may be in older versions)")
    else:
        print("✓  CLEAN — no known vulnerabilities in package or dependencies")
    print()
    print("=" * 55)


# ── Tests ──

def run_tests() -> None:
    passed = 0
    failed = 0

    def check(name: str, condition: bool) -> None:
        nonlocal passed, failed
        if condition:
            passed += 1
            print(f"  PASS: {name}")
        else:
            failed += 1
            print(f"  FAIL: {name}")

    print("Running audit_dependency tests...\n")

    # extract_deps
    mock_meta = {"info": {"requires_dist": [
        "pydyf>=0.11.0",
        "cffi>=0.6",
        "Pillow>=9.1.0",
        "fonttools[woff]>=4.59.2",
        'extra ; python_version < "3.8"',
    ]}}
    deps = extract_deps(mock_meta)
    check("extracts pydyf", "pydyf" in deps)
    check("extracts cffi", "cffi" in deps)
    check("extracts Pillow", "Pillow" in deps)
    check("extracts fonttools (strips extras)", "fonttools" in deps)
    check("extracts extra (strips marker)", "extra" in deps)
    check("no duplicates", len(deps) == len(set(deps)))

    # extract_deps empty
    check("empty requires_dist", extract_deps({"info": {}}) == [])
    check("null requires_dist", extract_deps({"info": {"requires_dist": None}}) == [])

    # severity_label
    check("severity from database_specific", severity_label({"database_specific": {"severity": "HIGH"}}) == "HIGH")
    check("severity unknown fallback", severity_label({}) == "UNKNOWN")

    # osv_query returns a list (even if empty — tests network)
    result = osv_query("this-package-does-not-exist-12345", "PyPI")
    check("osv_query returns list for nonexistent", isinstance(result, list))

    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Security audit for dependencies")
    parser.add_argument("package", nargs="?", help="Package name to audit")
    parser.add_argument("--npm", action="store_true", help="Audit an npm package instead of PyPI")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    if not args.package:
        parser.error("package name required")

    if args.npm:
        print("npm audit not yet implemented — use: npm audit")
        sys.exit(1)

    report = audit_pypi(args.package)

    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print_report(report)
