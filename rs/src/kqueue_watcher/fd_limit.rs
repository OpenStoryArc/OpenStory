//! kqueue holds one descriptor per watched file/directory, across all actors.

use std::io;
use std::sync::Mutex;

// Three default 4096-file watcher budgets, plus directories, DBs and sockets.
const REQUIRED_DESCRIPTORS: libc::rlim_t = 16_384;
static LIMIT_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn prepare() -> io::Result<()> {
    // Watcher actors start concurrently. Serialize the process-wide read/set
    // so another actor cannot restore an older soft limit in between them.
    let _guard = LIMIT_LOCK
        .lock()
        .map_err(|_| io::Error::other("file descriptor limit lock poisoned"))?;
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: limit is a valid writable rlimit for this synchronous call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(io::Error::other(format!(
            "cannot read file descriptor limit: {}",
            io::Error::last_os_error()
        )));
    }
    if limit.rlim_cur >= REQUIRED_DESCRIPTORS {
        return Ok(());
    }
    if limit.rlim_max < REQUIRED_DESCRIPTORS {
        return Err(io::Error::other(format!(
            "file descriptor hard limit {} is below the watcher requirement of {REQUIRED_DESCRIPTORS}; raise the launcher's hard limit and restart OpenStory",
            limit.rlim_max
        )));
    }
    let previous = limit.rlim_cur;
    limit.rlim_cur = REQUIRED_DESCRIPTORS;
    // SAFETY: limit is initialized; only the soft limit is raised, within the
    // existing hard limit. No privileges or changes to other processes needed.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
        return Err(io::Error::other(format!(
            "cannot raise file descriptor soft limit from {previous} to {REQUIRED_DESCRIPTORS}: {}; configure the launcher's file limit and restart OpenStory",
            io::Error::last_os_error()
        )));
    }
    eprintln!("Raised file descriptor soft limit from {previous} to {REQUIRED_DESCRIPTORS} for live watchers");
    Ok(())
}
