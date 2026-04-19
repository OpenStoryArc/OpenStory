//! CLI boot must fail loudly when NATS is unreachable.
//!
//! Phase 1 commit 1.1 of the TDD plan
//! (`/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`).
//!
//! Before this commit, the CLI silently fell back to `NoopBus` when NATS
//! wasn't available — and the production pipeline collapsed into a
//! synchronous demo-mode function (`ingest_events` with the
//! `!bus.is_active()` guard added in commit 970043a). Users who couldn't
//! tell the two modes apart ended up with a system that *looked* like
//! it worked but behaved fundamentally differently from the reactive
//! actor pipeline.
//!
//! With NATS required, the CLI now fails fast with a clear error when
//! NATS is unreachable. This test pins that behavior.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Spawn a child, wait up to `timeout`, kill on timeout. Return (exit code,
/// stdout, stderr). Captures whatever the child wrote before termination.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> (Option<i32>, String, String) {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn CLI");

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("try_wait failed: {e}"),
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let _ = child.wait();

    (status.and_then(|s| s.code()), stdout, stderr)
}

/// Assert the CLI exits non-zero with `NATS unavailable` on stderr when
/// given an unreachable NATS URL.
#[test]
fn cli_fails_fast_when_nats_unreachable() {
    // Port 59999 — almost certainly unbound. If this ever collides with
    // a real service, pick another high port.
    let bin = env!("CARGO_BIN_EXE_open-story");

    let tmp = tempfile::tempdir().expect("create temp dir");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).expect("create watch dir");

    let mut cmd = Command::new(bin);
    cmd.args([
        "serve",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
        "--data-dir",
    ])
    .arg(tmp.path())
    .args(["--watch-dir"])
    .arg(&watch_dir)
    .args(["--nats-url", "nats://127.0.0.1:59999"]);

    // 8s is generous — a fast NATS connect refusal returns in <100ms.
    // If this times out, the CLI is still using the NoopBus fallback
    // and silently succeeding, which is exactly the bug this test guards.
    let (exit_code, stdout, stderr) = run_with_timeout(cmd, Duration::from_secs(8));

    match exit_code {
        None => panic!(
            "CLI did not exit within timeout — it is still silently \
             falling back to NoopBus when NATS is unreachable. \
             This test expects a hard failure.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        Some(0) => panic!(
            "CLI exited successfully despite unreachable NATS — fallback \
             to NoopBus must be removed for NATS to be a hard requirement.\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        Some(_code) => {}
    }

    assert!(
        stderr.contains("NATS") && (stderr.contains("unavailable") || stderr.contains("required")),
        "stderr should clearly name NATS as unavailable/required. \
         Got stderr:\n{stderr}"
    );
}
