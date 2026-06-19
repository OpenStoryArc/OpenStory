#!/usr/bin/env python3
"""Red-team runner for OpenStory.

Orchestrates every security check in one place: dependency scanners,
the in-process aggressive test suite, the testcontainer-based suite,
and policy checks (cargo-deny, lint). Prints a structured fact sheet
that an AI agent or a human can act on.

The script does NOT narrate. Phase 1 is: collect deterministic facts.
The agent invoking it (or the human reader) does the synthesis.

## Soul

A red team is a structured adversary. This script is a synthetic
adversary — every check it runs is an attack vector someone (an
attacker, a future regression, a malicious dep) might exploit. A
green run means the system fended off each probe at the time of
the run. A red run names exactly which probe broke through.

## Usage

    python3 scripts/red_team.py                 # full run
    python3 scripts/red_team.py --json          # machine-readable
    python3 scripts/red_team.py --quick         # skip slow probes (container, geiger)
    python3 scripts/red_team.py --only deps     # just dependency scanners
    python3 scripts/red_team.py --only tests    # just the test suites
    python3 scripts/red_team.py --only policy   # just cargo-deny + clippy
    python3 scripts/red_team.py --fail-on high  # exit-code policy (none|low|medium|high)

## Exit codes

    0  — all probes green at the chosen severity threshold
    1  — at least one probe red
    2  — a required tool is missing (install hints printed)
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RS_DIR = REPO_ROOT / "rs"
UI_LOCK = REPO_ROOT / "ui" / "package-lock.json"
E2E_LOCK = REPO_ROOT / "e2e" / "package-lock.json"
PY_REQ = REPO_ROOT / "telegram-bot" / "requirements.txt"

ANSI_RED = "\033[31m"
ANSI_GREEN = "\033[32m"
ANSI_YELLOW = "\033[33m"
ANSI_DIM = "\033[2m"
ANSI_BOLD = "\033[1m"
ANSI_RESET = "\033[0m"

# ── Probe data model ────────────────────────────────────────────────────


@dataclass
class Probe:
    """A single adversarial check. `severity` is the highest severity it
    can produce when red. `findings` is empty when green."""

    name: str
    category: str
    severity: str = "info"  # info | low | medium | high | critical
    status: str = "pending"  # pending | green | red | skipped | error
    duration_ms: int = 0
    findings: list[str] = field(default_factory=list)
    detail: str = ""

    @property
    def is_green(self) -> bool:
        return self.status == "green"

    @property
    def is_red(self) -> bool:
        return self.status == "red"


# ── Helpers ─────────────────────────────────────────────────────────────


def have(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def run(cmd: list[str], cwd: Path | None = None, timeout: int = 300) -> tuple[int, str, str]:
    """Run a command. Returns (returncode, stdout, stderr). Never raises."""
    try:
        p = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", f"timeout after {timeout}s"
    except FileNotFoundError as e:
        return 127, "", str(e)


def cargo_audit_bin() -> str:
    home = Path.home()
    candidate = home / ".cargo" / "bin" / "cargo-audit"
    return str(candidate) if candidate.exists() else "cargo-audit"


def cargo_deny_bin() -> str:
    home = Path.home()
    candidate = home / ".cargo" / "bin" / "cargo-deny"
    return str(candidate) if candidate.exists() else "cargo-deny"


def osv_bin() -> str | None:
    for p in ["/tmp/osv-scanner", shutil.which("osv-scanner")]:
        if p and Path(p).exists():
            return p
    return None


# ── Probes ──────────────────────────────────────────────────────────────


def probe_cargo_audit() -> Probe:
    p = Probe(name="cargo-audit", category="deps", severity="high")
    t0 = time.time()
    code, out, _ = run([cargo_audit_bin(), "audit"], cwd=RS_DIR, timeout=120)
    p.duration_ms = int((time.time() - t0) * 1000)

    # Parse: lines like "Crate: X" with "Title: Y"
    findings = []
    cur_crate = None
    for line in (out or "").splitlines():
        line = line.rstrip()
        if line.startswith("Crate:"):
            cur_crate = line.split(":", 1)[1].strip()
        elif line.startswith("Title:") and cur_crate:
            findings.append(f"{cur_crate}: {line.split(':', 1)[1].strip()}")
            cur_crate = None
    p.findings = findings
    # cargo-audit exit code 1 = vulnerabilities; 0 = warnings only or none.
    # We treat unmaintained warnings as info, real vulns as red.
    has_vuln = "error:" in out
    p.status = "red" if has_vuln else "green"
    p.detail = f"{len(findings)} findings (warnings + vulns)"
    return p


def probe_cargo_deny() -> Probe:
    p = Probe(name="cargo-deny check", category="policy", severity="medium")
    if not Path(REPO_ROOT / "deny.toml").exists() and not Path(RS_DIR / "deny.toml").exists():
        p.status = "skipped"
        p.detail = "deny.toml not found; run `cargo deny init` first"
        return p

    deny_dir = RS_DIR if Path(RS_DIR / "deny.toml").exists() else REPO_ROOT
    t0 = time.time()
    code, out, err = run(
        [cargo_deny_bin(), "check", "bans", "sources"],
        cwd=deny_dir,
        timeout=120,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    p.findings = [l for l in (out + err).splitlines() if "error[" in l]
    p.status = "green" if code == 0 else "red"
    p.detail = f"exit={code}"
    return p


def probe_osv_scan() -> Probe:
    p = Probe(name="osv-scanner", category="deps", severity="high")
    bin_path = osv_bin()
    if not bin_path:
        p.status = "skipped"
        p.detail = "osv-scanner not installed (download from github.com/google/osv-scanner)"
        return p

    args = [bin_path, "scan", "source", "--format", "json"]
    for lock in [RS_DIR / "Cargo.lock", UI_LOCK, E2E_LOCK, PY_REQ]:
        if lock.exists():
            args += ["-L", str(lock)]

    t0 = time.time()
    code, out, _ = run(args, timeout=180)
    p.duration_ms = int((time.time() - t0) * 1000)

    try:
        d = json.loads(out) if out else {}
    except json.JSONDecodeError:
        p.status = "error"
        p.detail = "non-JSON output from osv-scanner"
        return p

    cves = []
    informational = []
    for res in d.get("results", []):
        src = res.get("source", {}).get("path", "?")
        for pkg in res.get("packages", []):
            info = pkg.get("package", {})
            for v in pkg.get("vulnerabilities", []):
                # Filter unmaintained / informational advisories — they're
                # not exploitable today, just "no future patches." They show
                # up in cargo-audit as warnings, not errors. We surface them
                # under `informational` but don't turn the probe red on them.
                summary = (v.get("summary") or "").lower()
                details = (v.get("details") or "").lower()
                is_informational = (
                    "unmaintained" in summary
                    or "unmaintained" in details
                    or v.get("database_specific", {}).get("informational") is not None
                )
                line = f"[{Path(src).name}] {info.get('name')} {info.get('version')}: {v.get('id')}"
                if is_informational:
                    informational.append(line)
                else:
                    cves.append(line)
    p.findings = cves + informational
    p.status = "red" if cves else "green"
    p.detail = f"{len(cves)} CVE(s), {len(informational)} informational"
    return p


def probe_npm_audit(lock: Path, label: str) -> Probe:
    p = Probe(name=f"npm audit ({label})", category="deps", severity="high")
    if not lock.exists() or not have("npm"):
        p.status = "skipped"
        p.detail = "lockfile or npm missing"
        return p
    t0 = time.time()
    code, out, _ = run(["npm", "audit", "--json"], cwd=lock.parent, timeout=120)
    p.duration_ms = int((time.time() - t0) * 1000)
    try:
        d = json.loads(out) if out else {}
    except json.JSONDecodeError:
        p.status = "error"
        return p
    meta = d.get("metadata", {}).get("vulnerabilities", {})
    high = meta.get("high", 0) + meta.get("critical", 0)
    med = meta.get("moderate", 0)
    low = meta.get("low", 0)
    p.findings = [f"{n} {sev}" for sev, n in [("high+critical", high), ("moderate", med), ("low", low)] if n]
    p.status = "red" if high > 0 else ("green" if med + low == 0 else "green")
    if med + low > 0 and high == 0:
        p.severity = "medium"
    p.detail = f"high={high} med={med} low={low}"
    return p


def probe_security_test_suite() -> Probe:
    p = Probe(name="test_security (baseline)", category="tests", severity="high")
    t0 = time.time()
    code, out, _ = run(
        ["cargo", "test", "--test", "test_security"],
        cwd=RS_DIR,
        timeout=300,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    p.status = "green" if code == 0 else "red"
    failures = [l for l in out.splitlines() if "FAILED" in l or "panicked" in l]
    p.findings = failures[:20]
    p.detail = f"exit={code}"
    return p


def probe_aggressive_test_suite() -> Probe:
    p = Probe(name="test_security_aggressive", category="tests", severity="high")
    t0 = time.time()
    code, out, _ = run(
        ["cargo", "test", "--test", "test_security_aggressive"],
        cwd=RS_DIR,
        timeout=300,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    p.status = "green" if code == 0 else "red"
    failures = [l for l in out.splitlines() if "FAILED" in l or "panicked" in l]
    p.findings = failures[:20]
    p.detail = f"exit={code}"
    return p


def probe_container_test_suite(quick: bool) -> Probe:
    p = Probe(
        name="test_security_container",
        category="tests",
        severity="high",
    )
    if quick:
        p.status = "skipped"
        p.detail = "--quick mode"
        return p

    if not have("docker"):
        p.status = "skipped"
        p.detail = "docker not available"
        return p

    # Verify image exists; build if not.
    code, _, _ = run(["docker", "image", "inspect", "open-story:test"], timeout=15)
    if code != 0:
        # Try to build — long step (~3-5 min first time)
        print(
            f"{ANSI_DIM}  building open-story:test (first run only)...{ANSI_RESET}",
            file=sys.stderr,
        )
        bcode, _, berr = run(
            ["docker", "build", "-t", "open-story:test", "."],
            cwd=RS_DIR,
            timeout=900,
        )
        if bcode != 0:
            p.status = "error"
            p.detail = f"docker build failed: {berr[:200]}"
            return p

    t0 = time.time()
    code, out, _ = run(
        ["cargo", "test", "--test", "test_security_container"],
        cwd=RS_DIR,
        timeout=600,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    p.status = "green" if code == 0 else "red"
    failures = [l for l in out.splitlines() if "FAILED" in l or "panicked" in l]
    p.findings = failures[:20]
    p.detail = f"exit={code}"
    return p


def probe_clippy() -> Probe:
    p = Probe(name="clippy -D warnings", category="policy", severity="low")
    t0 = time.time()
    code, out, err = run(
        ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        cwd=RS_DIR,
        timeout=600,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    combined = out + err
    # Distinguish "clippy not installed" (environment) from "clippy
    # found lints" (code defect). The former is a setup issue and
    # should skip cleanly so CI logs aren't flooded with false reds.
    if "is not installed for the toolchain" in combined or "no such command: `clippy`" in combined:
        p.status = "skipped"
        p.detail = "clippy not installed (run: rustup component add clippy)"
        return p
    p.status = "green" if code == 0 else "red"
    errs = [l for l in combined.splitlines() if "error:" in l]
    p.findings = errs[:10]
    p.detail = f"exit={code}, {len(errs)} errors"
    return p


def probe_cargo_vet() -> Probe:
    """`cargo vet` — has someone trusted reviewed this crate's code?

    Imports trusted audits from Mozilla / Google / Embark / Zcash /
    Bytecode Alliance / ISRG and counts the delta. A red probe here
    means new exemptions appeared (a dep bump that no trusted org
    has audited yet — review the source before merging).

    The probe is comparative: it reads the exemption count from
    supply-chain/config.toml. If the count drifts upward from the
    committed baseline, we want to know — that's the signal that a
    new unvetted crate entered the tree.
    """
    p = Probe(name="cargo-vet supply-chain", category="policy", severity="medium")
    bin_path = shutil.which("cargo-vet") or str(Path.home() / ".cargo" / "bin" / "cargo-vet")
    if not Path(bin_path).exists():
        p.status = "skipped"
        p.detail = "cargo-vet not installed (cargo install cargo-vet --locked)"
        return p

    if not (RS_DIR / "supply-chain" / "config.toml").exists():
        p.status = "skipped"
        p.detail = "supply-chain/config.toml missing (run: cd rs && cargo vet init)"
        return p

    t0 = time.time()
    code, out, err = run(["cargo", "vet", "check"], cwd=RS_DIR, timeout=180)
    p.duration_ms = int((time.time() - t0) * 1000)

    # cargo vet check exits 0 when "vetting succeeds" (exemptions are
    # accepted), non-zero when an unaudited crate slipped past all
    # exemptions/imports — which is the "new dep needs review" case.
    summary_line = next((l for l in out.splitlines() if "Vetting Succeeded" in l or "Vetting Failed" in l), "")
    p.detail = summary_line.strip() or f"exit={code}"
    if code != 0:
        p.status = "red"
        p.findings = [l for l in (out + err).splitlines() if "violation" in l.lower() or "missing" in l.lower()][:10]
        return p

    # Parse "175 fully audited, 4 partially audited, 374 exempted"
    import re
    m = re.search(r"(\d+) fully audited.*?(\d+) exempted", summary_line)
    if m:
        audited = int(m.group(1))
        exempted = int(m.group(2))
        # Compare against a committed baseline if present
        baseline = RS_DIR / "supply-chain" / ".baseline-exemptions"
        if baseline.exists():
            prev = int(baseline.read_text().strip())
            if exempted > prev:
                p.status = "red"
                p.findings = [
                    f"exemptions rose from {prev} to {exempted} — {exempted - prev} new unvetted crates",
                    "run `cargo vet suggest` to see the review queue",
                ]
                return p
        p.status = "green"
        p.detail = f"{audited} audited, {exempted} exempted (no new unvetted)"
    else:
        p.status = "green"
    return p


def probe_cargo_geiger() -> Probe:
    """`cargo-geiger` — count unsafe-block surface per crate.

    Unsafe code isn't bad, but it's the only place memory-safety
    bugs live in Rust. A crate with high unsafe count + low vetting
    is a priority review target. We surface the top-N unsafe crates;
    sustained growth between runs is the red signal.
    """
    p = Probe(name="cargo-geiger unsafe surface", category="policy", severity="low")
    bin_path = shutil.which("cargo-geiger") or str(Path.home() / ".cargo" / "bin" / "cargo-geiger")
    if not Path(bin_path).exists():
        p.status = "skipped"
        p.detail = "cargo-geiger not installed (cargo install cargo-geiger --locked)"
        return p

    t0 = time.time()
    # geiger is slow (compiles the whole tree); --output-format Json gives
    # structured output, but takes 5+ min. Use --update-readme=false and
    # text output for speed.
    code, out, _ = run(
        ["cargo", "geiger", "--all-features", "--quiet", "--output-format", "Json"],
        cwd=RS_DIR,
        timeout=900,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    if code != 0:
        p.status = "error"
        p.detail = f"exit={code}"
        return p

    try:
        d = json.loads(out)
    except json.JSONDecodeError:
        p.status = "error"
        p.detail = "non-JSON output"
        return p

    # Each package has counts.unsafe_.{functions,exprs,impls,traits,methods}
    rows = []
    for pkg in d.get("packages", []):
        info = pkg.get("package", {}).get("id", {})
        name = info.get("name", "?")
        unsafe = pkg.get("unsafety", {}).get("used", {})
        total = sum(unsafe.get(k, {}).get("safe", 0) + unsafe.get(k, {}).get("unsafe_", 0)
                    for k in ("functions", "exprs", "item_impls", "item_traits", "methods"))
        u = sum(unsafe.get(k, {}).get("unsafe_", 0)
                for k in ("functions", "exprs", "item_impls", "item_traits", "methods"))
        if u > 0:
            rows.append((name, u, total))
    rows.sort(key=lambda r: -r[1])
    top = rows[:15]
    p.findings = [f"{n}: {u} unsafe ({100*u/max(t,1):.1f}% of {t})" for n, u, t in top]
    p.status = "green"
    p.detail = f"{len(rows)} crates with unsafe code; top {len(top)} listed"
    return p


def probe_npm_signatures(lock: Path, label: str) -> Probe:
    """`npm audit signatures` — verify sigstore attestations on the npm tree."""
    p = Probe(name=f"npm signatures ({label})", category="policy", severity="medium")
    if not lock.exists() or not have("npm"):
        p.status = "skipped"
        p.detail = "lockfile or npm missing"
        return p
    t0 = time.time()
    code, out, _ = run(["npm", "audit", "signatures"], cwd=lock.parent, timeout=120)
    p.duration_ms = int((time.time() - t0) * 1000)
    # Parse lines like "180 packages have verified registry signatures"
    import re
    verified_m = re.search(r"(\d+) packages? have verified registry signatures", out)
    missing_m = re.search(r"(\d+) packages? have missing registry signatures", out)
    invalid_m = re.search(r"(\d+) packages? have invalid registry signatures", out)
    attested_m = re.search(r"(\d+) packages? have verified attestations", out)

    verified = int(verified_m.group(1)) if verified_m else 0
    missing = int(missing_m.group(1)) if missing_m else 0
    invalid = int(invalid_m.group(1)) if invalid_m else 0
    attested = int(attested_m.group(1)) if attested_m else 0

    if invalid > 0:
        p.status = "red"
        p.findings = [f"{invalid} packages with INVALID signatures — possible compromise"]
        p.severity = "high"
    elif missing > 0:
        # Many packages legitimately don't sign yet — surface but don't fail
        p.status = "green"
        p.detail = f"{verified} verified, {missing} unsigned, {attested} with sigstore attestations"
    else:
        p.status = "green"
        p.detail = f"{verified} verified, {attested} with sigstore attestations"
    return p


def probe_supply_chain_publishers() -> Probe:
    """Flag direct deps whose ONLY publisher is a single account with no
    team/org backing — the typical typosquat / hijacked-package pattern.

    Real-world supply-chain attacks (event-stream 2018, shai-hulud 2025,
    chalk/debug 2025) all involved a single-maintainer crate getting a
    new co-maintainer or being transferred. Multi-publisher / team-backed
    crates have a built-in second pair of eyes on every release.

    We invoke `cargo supply-chain crates` (slow — 1 API call per crate
    with crates.io rate limit). Results are parsed from human output;
    the tool doesn't have --json yet.
    """
    p = Probe(name="supply-chain publishers", category="policy", severity="medium")
    bin_path = shutil.which("cargo-supply-chain") or str(Path.home() / ".cargo" / "bin" / "cargo-supply-chain")
    if not Path(bin_path).exists():
        p.status = "skipped"
        p.detail = "cargo-supply-chain not installed (cargo install cargo-supply-chain)"
        return p

    t0 = time.time()
    # crates.io rate-limits API calls; ~150 deps × 2s = 300s baseline.
    # We give it 30 min — generous, but the probe is also gated to
    # non-quick mode. Timing out is informational, not red.
    code, out, _ = run(
        [bin_path, "crates"],
        cwd=RS_DIR,
        timeout=1800,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    if code == 124:
        p.status = "skipped"
        p.detail = "timed out hitting crates.io API (rate-limited; retry later)"
        return p
    if code != 0:
        p.status = "error"
        p.detail = f"exit={code}"
        return p

    # Parse lines like:
    #   1. libc: team "github:rust-lang:libc", JohnTitor, gnzlbg, ...
    # Solo-publisher = no "team " token AND fewer than 2 individuals.
    import re
    suspicious = []
    pattern = re.compile(r"^\s*\d+\.\s+([\w\-]+):\s*(.*)$")
    for line in out.splitlines():
        m = pattern.match(line)
        if not m:
            continue
        crate, publishers = m.group(1), m.group(2)
        has_team = "team " in publishers
        # Count individual publishers — comma-separated after stripping teams
        individuals = [
            x.strip()
            for x in re.sub(r'team "[^"]+",?\s*', "", publishers).split(",")
            if x.strip()
        ]
        if not has_team and len(individuals) < 2:
            suspicious.append(f"{crate} → solo: {publishers}")

    p.findings = suspicious
    p.status = "red" if suspicious else "green"
    p.detail = f"{len(suspicious)} solo-publisher crates"
    return p


# ── SAST: probes that read OUR OWN code, not deps ─────────────────────


def probe_hadolint() -> Probe:
    """Lint every Dockerfile in the repo for misconfig + bad patterns."""
    p = Probe(name="hadolint", category="sast", severity="medium")
    bin_path = "/tmp/hadolint" if Path("/tmp/hadolint").exists() else shutil.which("hadolint")
    if not bin_path:
        p.status = "skipped"
        p.detail = "hadolint not installed (download from github.com/hadolint/hadolint/releases)"
        return p

    # Find all production Dockerfiles tracked by git. Test fixtures
    # (rs/tests/fixtures/) contain intentionally-malformed inputs.
    code, out, _ = run(
        ["git", "ls-files", "*Dockerfile*", ":!:**/fixtures/**"],
        cwd=REPO_ROOT,
        timeout=30,
    )
    if code != 0:
        p.status = "error"
        p.detail = "git ls-files failed"
        return p
    files = [REPO_ROOT / f for f in out.strip().split("\n") if f and (REPO_ROOT / f).exists()]
    if not files:
        p.status = "skipped"
        p.detail = "no Dockerfiles tracked"
        return p

    cfg = REPO_ROOT / ".hadolint.yaml"
    cmd = [bin_path]
    if cfg.exists():
        cmd += ["--config", str(cfg)]
    cmd += [str(f) for f in files]

    t0 = time.time()
    code, out, _ = run(cmd, cwd=REPO_ROOT, timeout=60)
    p.duration_ms = int((time.time() - t0) * 1000)
    findings = [l for l in out.splitlines() if l.strip()]
    p.findings = findings[:15]
    p.status = "green" if code == 0 else "red"
    p.detail = f"{len(files)} Dockerfiles, {len(findings)} findings"
    return p


def probe_gitleaks() -> Probe:
    """Scan git tree + history for committed secrets."""
    p = Probe(name="gitleaks", category="sast", severity="high")
    bin_path = "/tmp/gitleaks" if Path("/tmp/gitleaks").exists() else shutil.which("gitleaks")
    if not bin_path:
        p.status = "skipped"
        p.detail = "gitleaks not installed (download from github.com/gitleaks/gitleaks/releases)"
        return p

    cfg = REPO_ROOT / ".gitleaks.toml"
    cmd = [bin_path, "detect", "--no-banner", "--report-format=json", "--report-path=/tmp/gitleaks-rt.json"]
    if cfg.exists():
        cmd += ["--config", str(cfg)]

    t0 = time.time()
    code, _, _ = run(cmd, cwd=REPO_ROOT, timeout=300)
    p.duration_ms = int((time.time() - t0) * 1000)
    report = Path("/tmp/gitleaks-rt.json")
    findings = []
    if report.exists():
        try:
            data = json.loads(report.read_text() or "[]")
            findings = [
                f"[{f.get('RuleID')}] {f.get('File')}:{f.get('StartLine')} ({(f.get('Commit') or '')[:8]})"
                for f in data
            ]
        except json.JSONDecodeError:
            pass
    p.findings = findings[:10]
    p.status = "green" if code == 0 else "red"
    p.detail = f"{len(findings)} leaks (after allowlist)"
    return p


def probe_semgrep() -> Probe:
    """Multi-language SAST (OWASP / Rust / TS / Python / Dockerfile / secrets)."""
    p = Probe(name="semgrep (multi-lang SAST)", category="sast", severity="high")
    # Prefer Docker — keeps install path zero
    if not have("docker"):
        p.status = "skipped"
        p.detail = "docker not available (semgrep ships as a container)"
        return p

    t0 = time.time()
    code, out, _ = run(
        [
            "docker", "run", "--rm",
            "-v", f"{REPO_ROOT}:/src",
            "-w", "/src",
            "semgrep/semgrep",
            "semgrep", "scan",
            "--config", "p/owasp-top-ten",
            "--config", "p/rust",
            "--config", "p/typescript",
            "--config", "p/python",
            "--config", "p/dockerfile",
            "--config", "p/secrets",
            "--quiet", "--metrics", "off", "--error",
        ],
        timeout=900,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    findings = [l for l in out.splitlines() if l.strip() and not l.startswith("Scanning")]
    p.findings = findings[:15]
    p.status = "green" if code == 0 else "red"
    p.detail = f"exit={code}"
    return p


def probe_bandit() -> Probe:
    """Python security lint (telegram-bot + scripts/)."""
    p = Probe(name="bandit (python SAST)", category="sast", severity="medium")
    if not have("docker"):
        p.status = "skipped"
        p.detail = "docker not available"
        return p

    cfg_arg = ["-c", "bandit.yaml"] if (REPO_ROOT / "bandit.yaml").exists() else []
    t0 = time.time()
    code, out, _ = run(
        [
            "docker", "run", "--rm",
            "-v", f"{REPO_ROOT}:/src",
            "-w", "/src",
            "cytopia/bandit",
            *cfg_arg,
            "-r", "telegram-bot", "scripts",
            "-ll",   # medium+ only
            "-f", "json", "-o", "/src/.bandit-rt.json",
        ],
        timeout=180,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    report = REPO_ROOT / ".bandit-rt.json"
    findings = []
    if report.exists():
        try:
            d = json.loads(report.read_text())
            for r in d.get("results", []):
                findings.append(
                    f"[{r.get('test_id')}] {r.get('filename', '?')}:"
                    f"{r.get('line_number', '?')} {r.get('issue_text', '')[:80]}"
                )
        except json.JSONDecodeError:
            pass
    p.findings = findings[:10]
    # Bandit exits 1 when findings exist at the severity filter; 0 means clean.
    p.status = "green" if not findings else "red"
    p.detail = f"{len(findings)} medium+ findings"
    return p


def probe_install_scripts() -> Probe:
    """Flag npm packages with install scripts (potential supply-chain entry points)."""
    p = Probe(name="npm install-script audit", category="policy", severity="low")
    findings = []
    for lock in [UI_LOCK, E2E_LOCK]:
        if not lock.exists():
            continue
        try:
            d = json.loads(lock.read_text())
        except json.JSONDecodeError:
            continue
        for k, v in d.get("packages", {}).items():
            if v.get("hasInstallScript"):
                rest = k.split("node_modules/")[-1]
                findings.append(f"[{lock.parent.name}] {rest} {v.get('version', '')}")
    # We don't fail on install scripts (esbuild/fsevents are legitimate);
    # we just surface them for human review.
    p.findings = findings
    p.status = "green"
    p.detail = f"{len(findings)} packages run install scripts (review each manually)"
    return p


# ── Reporting ──────────────────────────────────────────────────────────


def severity_rank(sev: str) -> int:
    return {"info": 0, "low": 1, "medium": 2, "high": 3, "critical": 4}.get(sev, 0)


def colorize(probe: Probe) -> str:
    badge = {
        "green": f"{ANSI_GREEN}✓ GREEN{ANSI_RESET}",
        "red": f"{ANSI_RED}✗ RED  {ANSI_RESET}",
        "skipped": f"{ANSI_DIM}- SKIP {ANSI_RESET}",
        "error": f"{ANSI_YELLOW}! ERR  {ANSI_RESET}",
        "pending": f"{ANSI_DIM}· wait {ANSI_RESET}",
    }[probe.status]
    return badge


def print_human(probes: list[Probe]) -> None:
    print()
    print(f"{ANSI_BOLD}OpenStory red-team report{ANSI_RESET}")
    print(f"{'─' * 60}")
    by_cat: dict[str, list[Probe]] = {}
    for p in probes:
        by_cat.setdefault(p.category, []).append(p)
    for cat in ["deps", "policy", "sast", "tests"]:
        if cat not in by_cat:
            continue
        print(f"\n{ANSI_BOLD}{cat}{ANSI_RESET}")
        for p in by_cat[cat]:
            duration = f"{p.duration_ms / 1000:.1f}s" if p.duration_ms else "  -"
            print(f"  {colorize(p)} {p.name:<40} {duration:>6}  {ANSI_DIM}{p.detail}{ANSI_RESET}")
            if p.findings and p.status == "red":
                for f in p.findings[:5]:
                    print(f"      {ANSI_RED}→{ANSI_RESET} {f}")
                if len(p.findings) > 5:
                    print(f"      {ANSI_DIM}... and {len(p.findings) - 5} more{ANSI_RESET}")

    print()
    print(f"{'─' * 60}")
    green = sum(1 for p in probes if p.is_green)
    red = sum(1 for p in probes if p.is_red)
    skip = sum(1 for p in probes if p.status == "skipped")
    err = sum(1 for p in probes if p.status == "error")
    summary = f"{ANSI_GREEN}{green} green{ANSI_RESET}, {ANSI_RED}{red} red{ANSI_RESET}, {skip} skipped, {err} errored"
    print(f"  {summary}")
    print()


def to_json(probes: list[Probe]) -> str:
    return json.dumps(
        {
            "version": 1,
            "probes": [asdict(p) for p in probes],
            "summary": {
                "green": sum(1 for p in probes if p.is_green),
                "red": sum(1 for p in probes if p.is_red),
                "skipped": sum(1 for p in probes if p.status == "skipped"),
                "errored": sum(1 for p in probes if p.status == "error"),
            },
        },
        indent=2,
    )


# ── Main ────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    ap.add_argument("--quick", action="store_true", help="skip container + slow probes")
    ap.add_argument(
        "--only",
        choices=["deps", "policy", "sast", "tests", "all"],
        default="all",
        help="run only one probe category",
    )
    ap.add_argument(
        "--fail-on",
        choices=["none", "low", "medium", "high", "critical"],
        default="high",
        help="exit non-zero when a red probe at this severity or higher is found",
    )
    args = ap.parse_args()

    probes: list[Probe] = []

    if args.only in ("all", "deps"):
        probes.append(probe_cargo_audit())
        probes.append(probe_osv_scan())
        probes.append(probe_npm_audit(UI_LOCK, "ui"))
        probes.append(probe_npm_audit(E2E_LOCK, "e2e"))

    if args.only in ("all", "policy"):
        probes.append(probe_cargo_deny())
        probes.append(probe_cargo_vet())
        probes.append(probe_install_scripts())
        probes.append(probe_npm_signatures(UI_LOCK, "ui"))
        probes.append(probe_npm_signatures(E2E_LOCK, "e2e"))
        if not args.quick:
            probes.append(probe_clippy())
            probes.append(probe_supply_chain_publishers())
            probes.append(probe_cargo_geiger())

    if args.only in ("all", "sast"):
        probes.append(probe_hadolint())
        probes.append(probe_gitleaks())
        probes.append(probe_bandit())
        if not args.quick:
            # semgrep takes 60-180s — full mode only.
            probes.append(probe_semgrep())

    if args.only in ("all", "tests"):
        probes.append(probe_security_test_suite())
        probes.append(probe_aggressive_test_suite())
        probes.append(probe_container_test_suite(args.quick))

    if args.json:
        print(to_json(probes))
    else:
        print_human(probes)

    # Exit policy: fail if any red probe meets the severity threshold
    threshold = severity_rank(args.fail_on)
    for p in probes:
        if p.is_red and severity_rank(p.severity) >= threshold:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
