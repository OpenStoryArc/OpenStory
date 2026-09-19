#![cfg(target_os = "macos")]

use open_story::kqueue_watcher::KqueueWatcher;

// Limits are process-wide: exercise each case in a child, never in the
// parallel test runner that also owns sockets and temporary files.
#[test]
fn watcher_handles_inherited_descriptor_limits() {
    for scenario in ["low-soft", "already-high", "low-hard"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "descriptor_limit_child", "--nocapture"])
            .env("OPENSTORY_FD_TEST", scenario)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{scenario}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn descriptor_limit_child() {
    let Ok(scenario) = std::env::var("OPENSTORY_FD_TEST") else {
        return;
    };
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: valid writable rlimit pointer; this test runs in its own process.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    limit.rlim_cur = if scenario == "already-high" {
        16385
    } else {
        256
    };
    if scenario == "low-hard" {
        limit.rlim_max = 256;
    }
    let original_hard = limit.rlim_max;
    // SAFETY: only the isolated child process's limits are changed.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);

    let watcher = KqueueWatcher::new(4096);
    if scenario == "low-hard" {
        let error = watcher
            .err()
            .expect("an insufficient hard limit must be explicit");
        assert!(error.to_string().contains("file descriptor"), "{error}");
        return;
    }
    let mut watcher = watcher.unwrap();
    // SAFETY: valid writable output pointer.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    assert!(
        limit.rlim_cur >= 16384,
        "raise the inherited 256-fd soft limit"
    );
    assert_eq!(limit.rlim_max, original_hard, "preserve the hard limit");
    if scenario == "already-high" {
        assert_eq!(limit.rlim_cur, 16385, "preserve a higher soft limit");
    }
    let temp = tempfile::tempdir().unwrap();
    for index in 0..400 {
        let path = temp.path().join(format!("{index}.jsonl"));
        std::fs::write(&path, "{}\n").unwrap();
        watcher.register(&path, false).unwrap();
    }
}
