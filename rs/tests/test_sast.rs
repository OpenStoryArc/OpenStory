//! Static Application Security Testing — runs as part of `cargo test`.
//!
//! Each test shells out to a SAST tool, asserts a clean result, and
//! **skips cleanly** when the tool is missing (so CI without the tools
//! installed doesn't false-alarm). When the tool IS present, a finding
//! fails the test — which means a security-relevant regression in any
//! Dockerfile / committed secret / Python script / source pattern
//! becomes a red CI signal.
//!
//! Tools (all live outside this repo, layered with red_team.py):
//!   - hadolint     — Dockerfile linter
//!   - gitleaks     — committed-secret scanner (tree + history)
//!   - bandit       — Python security linter
//!   - semgrep      — multi-language SAST (via Docker)
//!
//! Run via:
//!   cargo test --test test_sast
//!
//! Why is this in Rust tests/ instead of a separate runner?
//!   - One command (`cargo test`) gates the whole quality surface
//!   - Cross-platform — tests skip when tools are absent on dev laptops
//!     but enforce in CI where the tools ARE present
//!   - No new test runner / GitHub Action to maintain
//!
//! The companion scripts/red_team.py runs these tools (plus dep scanners,
//! cargo-vet, the test_security_* suites) as a one-shot batch with a
//! richer report. This file is the always-on guard.

use std::path::PathBuf;
use std::process::Command;

/// Locate the repo root by walking up from CARGO_MANIFEST_DIR until we
/// find the workspace marker (rs/Cargo.toml or the parent of it).
fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for the open-story crate is rs/. Parent is repo root.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("CARGO_MANIFEST_DIR must be set by cargo");
    manifest.parent().expect("rs/ must have a parent").to_path_buf()
}

/// Returns true if `cmd` resolves on PATH or at the given absolute path.
fn tool_present(path_or_name: &str) -> bool {
    if path_or_name.starts_with('/') {
        std::path::Path::new(path_or_name).exists()
    } else {
        Command::new("which")
            .arg(path_or_name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Skip helper — prints a SKIP line and returns from the test.
macro_rules! skip {
    ($($arg:tt)*) => {{
        eprintln!("SKIP: {}", format!($($arg)*));
        return;
    }};
}

// ── hadolint ────────────────────────────────────────────────────────────

#[test]
fn sast_hadolint_dockerfiles_clean() {
    let bin = if tool_present("/tmp/hadolint") {
        "/tmp/hadolint"
    } else if tool_present("hadolint") {
        "hadolint"
    } else {
        skip!("hadolint not installed");
    };

    let root = repo_root();
    // Discover Dockerfiles via git so we don't pick up vendored ones.
    // Production Dockerfiles only — test fixtures (rs/tests/fixtures/)
    // contain intentionally-malformed inputs for the integration harness
    // and would false-positive any linter.
    let out = Command::new("git")
        .args(["ls-files", "*Dockerfile*", ":!:**/fixtures/**"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    let files: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|f| root.join(f))
        .filter(|p| p.exists())
        .collect();

    if files.is_empty() {
        skip!("no Dockerfiles tracked");
    }

    let cfg = root.join(".hadolint.yaml");
    let mut cmd = Command::new(bin);
    cmd.current_dir(&root);
    if cfg.exists() {
        cmd.arg("--config").arg(&cfg);
    }
    for f in &files {
        cmd.arg(f);
    }

    let out = cmd.output().expect("run hadolint");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "hadolint found Dockerfile issues across {} files:\n{}\n{}",
        files.len(),
        stdout,
        stderr,
    );
}

// ── gitleaks ────────────────────────────────────────────────────────────

#[test]
fn sast_gitleaks_no_committed_secrets() {
    let bin = if tool_present("/tmp/gitleaks") {
        "/tmp/gitleaks"
    } else if tool_present("gitleaks") {
        "gitleaks"
    } else {
        skip!("gitleaks not installed");
    };

    let root = repo_root();
    let cfg = root.join(".gitleaks.toml");
    let mut cmd = Command::new(bin);
    cmd.current_dir(&root).args(["detect", "--no-banner"]);
    if cfg.exists() {
        cmd.arg("--config").arg(&cfg);
    }
    let out = cmd.output().expect("run gitleaks");
    assert!(
        out.status.success(),
        "gitleaks found committed secrets:\n{}",
        String::from_utf8_lossy(&out.stdout),
    );
}

// ── bandit (Python SAST via Docker) ────────────────────────────────────

#[test]
fn sast_bandit_python_clean() {
    if !tool_present("docker") {
        skip!("docker not available (bandit runs via cytopia/bandit image)");
    }

    // Verify the image is pulled — otherwise this test would download
    // ~200MB on every run, which is too aggressive for `cargo test`.
    let inspect = Command::new("docker")
        .args(["image", "inspect", "cytopia/bandit"])
        .output()
        .expect("docker image inspect");
    if !inspect.status.success() {
        skip!("cytopia/bandit image not pulled (docker pull cytopia/bandit)");
    }

    let root = repo_root();
    let out = Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/src", root.display()))
        .args([
            "-w", "/src", "cytopia/bandit",
            "-c", "bandit.yaml",
            "-r", "telegram-bot", "scripts",
            "-ll",
        ])
        .output()
        .expect("run bandit");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // Bandit exits 1 when findings exist at the medium+ severity level.
    assert!(
        out.status.success(),
        "bandit found medium+ Python security issues:\n{}",
        combined,
    );
}

// ── semgrep (multi-lang SAST via Docker, gated by env var) ─────────────

/// Semgrep is slower (60-180s) than the other SAST tools — it pulls
/// rule packs from semgrep.dev on every run. Gated behind
/// `OPEN_STORY_SAST_SEMGREP=1` so `cargo test` stays fast by default.
/// Set the env var in CI to enforce.
#[test]
fn sast_semgrep_owasp_rust_ts_python_dockerfile_secrets_clean() {
    if std::env::var("OPEN_STORY_SAST_SEMGREP").as_deref() != Ok("1") {
        skip!("set OPEN_STORY_SAST_SEMGREP=1 to enable (slow probe)");
    }
    if !tool_present("docker") {
        skip!("docker not available");
    }

    let inspect = Command::new("docker")
        .args(["image", "inspect", "semgrep/semgrep"])
        .output()
        .expect("docker image inspect");
    if !inspect.status.success() {
        skip!("semgrep/semgrep image not pulled (docker pull semgrep/semgrep)");
    }

    let root = repo_root();
    let out = Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/src", root.display()))
        .args([
            "-w", "/src", "semgrep/semgrep",
            "semgrep", "scan",
            "--config", "p/owasp-top-ten",
            "--config", "p/rust",
            "--config", "p/typescript",
            "--config", "p/python",
            "--config", "p/dockerfile",
            "--config", "p/secrets",
            "--quiet", "--metrics", "off", "--error",
        ])
        .output()
        .expect("run semgrep");

    assert!(
        out.status.success(),
        "semgrep found SAST issues:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
