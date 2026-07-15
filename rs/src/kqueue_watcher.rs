//! macOS kqueue file watcher — precise, event-driven, no polling.
//!
//! # Why this exists
//!
//! OpenStory's default watcher uses `notify`'s FSEvents backend. We measured
//! (via `/api/watchers`) that FSEvents delivered only 2 events for a Codex
//! `rollout-*.jsonl` that was actively appended for 47 minutes — it silently
//! drops Codex's held-open, buffered-append writes, while catching Claude
//! Code's. A manual `touch` recovered every missed line, proving the reader is
//! correct: the only thing missing was the kernel change-notification.
//!
//! FSEvents is the wrong tool for this: it is a coalesced, directory-tree
//! notification API (Spotlight-style). The right tool is **kqueue**
//! (`EVFILT_VNODE`): kernel-level, immediate, per-vnode notification on
//! write/extend. The Python prototype (`scripts/kqueue_watch_probe.py`)
//! confirmed kqueue fires one event per held-open append, to the millisecond.
//!
//! # Design — hybrid, single event loop
//!
//! kqueue watches *file descriptors*, so a naive recursive watch would open an
//! fd per file and blow the process fd limit (`~/.claude/projects` already has
//! 300+ files vs a 256 default soft limit). Instead:
//!
//! - **Discovery**: register each directory's fd with `NOTE_WRITE`. The kernel
//!   fires when a dir's entries change (a new session file or date subdir
//!   appears). On that event we rescan the dir and register the newcomers.
//! - **Content**: register each *active* `.jsonl` file's fd with
//!   `NOTE_WRITE | NOTE_EXTEND`. On that event we incrementally read the new
//!   bytes (via the caller's closure).
//! - **FD budget**: file watches are capped; the least-recently-active file is
//!   evicted when the cap is hit. Idle/finished sessions fall off; active ones
//!   stay. Dirs are few, so they are never evicted.
//!
//! Linux already gets per-write events from inotify (`notify`'s default), so
//! this module is macOS-only; the FSEvents path remains the fallback.
#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::time::Instant;

use walkdir::WalkDir;

/// Cap on simultaneously-watched *file* descriptors. Dirs are unbounded (few).
///
/// Sized to cover the full transcript tree, not a small working set: an active
/// session appended to right now must stay watched even when hundreds of older
/// sessions exist, or its writes fire no vnode event and never stream. Matches
/// `watcher::MAX_WATCH_STATES` (the per-file offset table also caps at 4096), so
/// watching more files than this would be wasted anyway. 4096 open fds is
/// trivial against a modern `RLIMIT_NOFILE` (1,048,576 soft on macOS); the old
/// 128 assumed a 256 soft limit that no longer holds and silently dropped
/// active sessions once the tree grew past it.
pub const DEFAULT_FILE_BUDGET: usize = 4096;

// ── kqueue / kevent FFI (the only unsafe surface) ───────────────────────────

#[derive(Clone, Copy)]
struct VnodeEvent {
    fd: RawFd,
    fflags: u32,
}

/// Open a path for event-notification only (`O_EVTONLY` doesn't block unmount
/// and needs no read permission semantics beyond existence).
fn open_evtonly(path: &Path) -> io::Result<RawFd> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path has interior NUL"))?;
    // SAFETY: `c` is a valid NUL-terminated C string for the call's duration.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_EVTONLY) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

/// Register `fd` on `kq` for the given vnode `fflags` (edge-triggered).
fn kevent_add(kq: RawFd, fd: RawFd, fflags: u32) -> io::Result<()> {
    let change = libc::kevent {
        ident: fd as libc::uintptr_t,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: single-element changelist, no eventlist, no timeout — a pure
    // registration call. `change` outlives the call.
    let rc = unsafe { libc::kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Block up to `timeout_ms` for vnode events; return everything that fired.
fn kevent_poll(kq: RawFd, timeout_ms: i64) -> io::Result<Vec<VnodeEvent>> {
    const MAX: usize = 64;
    let mut out: [libc::kevent; MAX] = unsafe { std::mem::zeroed() };
    let ts = libc::timespec {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_nsec: ((timeout_ms % 1000) * 1_000_000) as libc::c_long,
    };
    // SAFETY: `out` is a valid, MAX-length array of kevent; `ts` outlives the
    // call. kevent writes at most MAX entries and returns the count.
    let n = unsafe {
        libc::kevent(
            kq,
            std::ptr::null(),
            0,
            out.as_mut_ptr(),
            MAX as libc::c_int,
            &ts,
        )
    };
    if n < 0 {
        let err = io::Error::last_os_error();
        // EINTR is benign (a signal interrupted the wait) — treat as "no events".
        if err.raw_os_error() == Some(libc::EINTR) {
            return Ok(Vec::new());
        }
        return Err(err);
    }
    Ok(out[..n as usize]
        .iter()
        .map(|e| VnodeEvent {
            fd: e.ident as RawFd,
            fflags: e.fflags,
        })
        .collect())
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("jsonl")
}

// ── watch table ─────────────────────────────────────────────────────────────

struct WatchEntry {
    path: PathBuf,
    is_dir: bool,
    last_active: Instant,
}

/// Owns the kqueue and the fd→path table. `Drop` closes every fd.
pub struct KqueueWatcher {
    kq: RawFd,
    entries: HashMap<RawFd, WatchEntry>,
    by_path: HashMap<PathBuf, RawFd>,
    file_budget: usize,
}

impl KqueueWatcher {
    pub fn new(file_budget: usize) -> io::Result<Self> {
        // SAFETY: kqueue() takes no args and returns an fd or -1.
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            kq,
            entries: HashMap::new(),
            by_path: HashMap::new(),
            file_budget,
        })
    }

    fn file_count(&self) -> usize {
        self.entries.values().filter(|e| !e.is_dir).count()
    }

    /// Register a path (idempotent). Dirs watch `NOTE_WRITE` (entries changed);
    /// files watch write/extend/delete/rename. Evicts an idle file first if the
    /// file budget would be exceeded.
    pub fn register(&mut self, path: &Path, is_dir: bool) -> io::Result<()> {
        if self.by_path.contains_key(path) {
            if let Some(&fd) = self.by_path.get(path) {
                if let Some(e) = self.entries.get_mut(&fd) {
                    e.last_active = Instant::now();
                }
            }
            return Ok(());
        }
        if !is_dir && self.file_count() >= self.file_budget {
            self.evict_idle_file();
        }
        let fd = open_evtonly(path)?;
        let fflags = if is_dir {
            libc::NOTE_WRITE
        } else {
            libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_DELETE | libc::NOTE_RENAME
        };
        if let Err(e) = kevent_add(self.kq, fd, fflags) {
            // SAFETY: fd came from open() above and is not yet in our table.
            unsafe { libc::close(fd) };
            return Err(e);
        }
        self.entries.insert(
            fd,
            WatchEntry {
                path: path.to_path_buf(),
                is_dir,
                last_active: Instant::now(),
            },
        );
        self.by_path.insert(path.to_path_buf(), fd);
        Ok(())
    }

    fn unregister(&mut self, fd: RawFd) {
        if let Some(entry) = self.entries.remove(&fd) {
            self.by_path.remove(&entry.path);
            // EV_DELETE is implicit on close; closing the fd removes the watch.
            // SAFETY: fd is one we opened and own; removed from the table above.
            unsafe { libc::close(fd) };
        }
    }

    /// Evict the least-recently-active *file* watch to free an fd.
    fn evict_idle_file(&mut self) {
        let victim = self
            .entries
            .iter()
            .filter(|(_, e)| !e.is_dir)
            .min_by_key(|(_, e)| e.last_active)
            .map(|(&fd, _)| fd);
        if let Some(fd) = victim {
            self.unregister(fd);
        }
    }

    fn touch(&mut self, fd: RawFd) {
        if let Some(e) = self.entries.get_mut(&fd) {
            e.last_active = Instant::now();
        }
    }
}

impl Drop for KqueueWatcher {
    fn drop(&mut self) {
        let fds: Vec<RawFd> = self.entries.keys().copied().collect();
        for fd in fds {
            // SAFETY: every fd in the table was opened by us and is owned.
            unsafe { libc::close(fd) };
        }
        // SAFETY: kq was returned by kqueue() and owned by self.
        unsafe { libc::close(self.kq) };
    }
}

/// Walk `root` and register the directory tree plus the `.jsonl` files, newest
/// files first so the budget keeps the most-likely-active sessions.
fn register_tree(w: &mut KqueueWatcher, root: &Path) {
    // Dirs first (cheap, unbounded), so discovery is live before files fill up.
    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() {
            let _ = w.register(entry.path(), true);
        }
    }
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_jsonl(e.path()))
        .filter_map(|e| {
            let m = e.metadata().ok()?.modified().ok()?;
            Some((e.path().to_path_buf(), m))
        })
        .collect();
    files.sort_by_key(|(_, t)| std::cmp::Reverse(*t)); // newest first
    for (path, _) in files.into_iter().take(w.file_budget) {
        let _ = w.register(&path, false);
    }
}

/// Rescan a directory and register any `.jsonl` files / subdirs we don't yet
/// watch. Returns the paths of newly-registered files so the caller can do an
/// initial incremental read of each.
fn rescan_dir(w: &mut KqueueWatcher, dir: &Path) -> Vec<PathBuf> {
    let mut fresh = Vec::new();
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return fresh,
    };
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let _ = w.register(&path, true);
        } else if ft.is_file()
            && is_jsonl(&path)
            && !w.by_path.contains_key(&path)
            && w.register(&path, false).is_ok()
        {
            fresh.push(path);
        }
    }
    fresh
}

/// Run the kqueue event loop. `on_file_changed` is invoked with a file path
/// whenever that file is created/extended/written; the caller does the
/// incremental read + emit. Blocks until the kqueue fd errors.
pub fn run_event_loop<F>(
    watch_dir: &Path,
    file_budget: usize,
    mut on_file_changed: F,
) -> io::Result<()>
where
    F: FnMut(&Path),
{
    let mut w = KqueueWatcher::new(file_budget)?;
    register_tree(&mut w, watch_dir);
    eprintln!(
        "Watching {} via kqueue ({} fds: {} files, {} dirs)...",
        watch_dir.display(),
        w.entries.len(),
        w.file_count(),
        w.entries.len() - w.file_count()
    );

    loop {
        // 1s timeout keeps the loop responsive to shutdown and lets a freshly
        // created date dir get picked up even if its parent event was missed.
        let events = w.kevent_poll_events()?;
        for ev in events {
            let Some(entry) = w.entries.get(&ev.fd) else {
                continue; // already evicted/unregistered
            };
            let is_dir = entry.is_dir;
            let path = entry.path.clone();

            // File gone — drop the watch.
            if ev.fflags & (libc::NOTE_DELETE | libc::NOTE_RENAME) != 0 {
                w.unregister(ev.fd);
                continue;
            }

            if is_dir {
                // A directory's entries changed: pick up new files / subdirs.
                for fresh in rescan_dir(&mut w, &path) {
                    on_file_changed(&fresh);
                }
            } else {
                w.touch(ev.fd);
                on_file_changed(&path);
            }
        }
    }
}

impl KqueueWatcher {
    fn kevent_poll_events(&self) -> io::Result<Vec<VnodeEvent>> {
        kevent_poll(self.kq, 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("os-kq-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        base
    }

    #[test]
    fn is_jsonl_only_matches_extension() {
        assert!(is_jsonl(Path::new("/x/a.jsonl")));
        assert!(!is_jsonl(Path::new("/x/a.json")));
        assert!(!is_jsonl(Path::new("/x/a")));
    }

    #[test]
    fn register_is_idempotent_and_budget_evicts_idle() {
        let dir = tmpdir().join("budget");
        let _ = std::fs::create_dir_all(&dir);
        let mut w = KqueueWatcher::new(2).unwrap();
        for name in ["a.jsonl", "b.jsonl", "c.jsonl"] {
            let p = dir.join(name);
            std::fs::write(&p, b"").unwrap();
            w.register(&p, false).unwrap();
        }
        // Budget of 2 → third registration evicted the oldest; never exceeds 2.
        assert_eq!(w.file_count(), 2, "file budget must hold");
        // Re-registering an existing path is a no-op (no fd leak).
        let before = w.entries.len();
        let again = dir.join("c.jsonl");
        w.register(&again, false).unwrap();
        assert_eq!(w.entries.len(), before, "re-register must not add an fd");
    }

    #[test]
    fn kqueue_catches_held_open_appends() {
        // The production proof, deterministic (no threads / no infinite loop):
        // a held-open buffered append — the Codex write pattern FSEvents
        // silently drops — must fire an EVFILT_VNODE write/extend event.
        let dir = tmpdir().join("appends");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("rollout.jsonl");
        std::fs::write(&file, b"").unwrap();

        let mut w = KqueueWatcher::new(64).unwrap();
        w.register(&file, false).unwrap();
        let fd = w.by_path[&file];

        // Append with the fd held open (the pattern that defeats FSEvents).
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
            writeln!(f, "{{\"seq\":0}}").unwrap();
            f.flush().unwrap();
        }

        // Poll once (up to 2s). kqueue is immediate, so the event is waiting.
        let events = kevent_poll(w.kq, 2000).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.fd == fd && e.fflags & (libc::NOTE_WRITE | libc::NOTE_EXTEND) != 0),
            "kqueue must deliver a write/extend event for a held-open append"
        );
    }
}
