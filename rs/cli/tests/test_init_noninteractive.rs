//! `open-story init` must refuse to run without an interactive terminal:
//! exit non-zero, point the user at the non-interactive path, and write
//! nothing. This pins the no-tty contract so the wizard never hangs or
//! silently clobbers a config in CI / scripted contexts.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Spawn with a null stdin (no tty), wait up to `timeout`, return
/// (exit code, stdout, stderr).
fn run_no_tty(mut cmd: Command, timeout: Duration) -> (Option<i32>, String, String) {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn CLI");

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("try_wait failed: {e}"),
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut stdout);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut stderr);
    }
    let _ = child.wait();
    (status.and_then(|s| s.code()), stdout, stderr)
}

#[test]
fn init_without_tty_exits_nonzero_with_instructions_and_writes_nothing() {
    let bin = env!("CARGO_BIN_EXE_open-story");
    let tmp = tempfile::tempdir().expect("temp dir");

    let mut cmd = Command::new(bin);
    cmd.arg("init").arg("--data-dir").arg(tmp.path());

    let (exit_code, stdout, stderr) = run_no_tty(cmd, Duration::from_secs(8));

    match exit_code {
        None => panic!(
            "init hung without a tty — it must bail immediately.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        Some(0) => panic!(
            "init exited 0 without a tty — it must refuse to prompt.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        Some(_) => {}
    }

    assert!(
        stderr.contains("interactive terminal") && stderr.contains("serve --init-config"),
        "stderr should explain it needs a tty and point at the non-interactive path.\nGot:\n{stderr}"
    );

    assert!(
        !tmp.path().join("config.toml").exists(),
        "init must not write config.toml when it can't prompt"
    );
}
