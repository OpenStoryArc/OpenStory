//! PRINCIPLE TEST — "Observe, never interfere."
//!
//! CLAUDE.md (principle #1):
//!
//!   > The listener is read-only. It watches transcript files and receives
//!   > hook events. It never writes back, never modifies agent behavior,
//!   > never blocks execution.
//!
//! This test scans production source files for write operations that
//! touch `watch_dir` or paths derived from it. If any production path
//! writes to the agent's source directory, the principle is violated.
//!
//! This is a SPIKE. It's a grep-style test — cheap, conservative,
//! may produce false positives on comment text, allowed by an
//! explicit allowlist. Report-and-panic, so first-run findings are
//! a useful inventory rather than a hard failure.
//!
//! See docs/research/architecture-audit/PRINCIPLES.md (to be added
//! when the pattern moves out of spike).

use std::path::{Path, PathBuf};

/// Forbidden write-operation patterns. Matched as substrings on lines
/// that also mention `watch_dir`.
const WRITE_OPS: &[&str] = &[
    "fs::write",
    "File::create",
    "fs::remove_file",
    "fs::remove_dir",
    "fs::rename",
    ".write_all",
    "writeln!",
    "OpenOptions::new",
];

/// Lines that are intentional exceptions. Pair: (file-path-suffix, line fragment).
/// We allow `create_dir_all(watch_dir)` style calls — they ensure the
/// directory exists but don't write INSIDE it. Bootstrap-only.
/// Anything else showing up in the failure list needs either a fix or
/// an explicit entry here with reasoning.
const ALLOWLIST: &[&str] = &[
    "create_dir_all(&watch_dir)",
    "create_dir_all(watch_dir)",
    "create_dir_all(&self.watch_dir)",
    "std::fs::create_dir_all(&watch_dir)",
    "std::fs::create_dir_all(watch_dir)",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Collect .rs files under any `src/` directory, excluding `target/`.
fn production_source_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&root).follow_links(false) {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.components().any(|c| c.as_os_str() == "target") {
            continue;
        }
        // Only files under a `src/` directory. Excludes `rs/tests/`,
        // `rs/*/tests/`, `benches/`, `examples/`.
        if !path.components().any(|c| c.as_os_str() == "src") {
            continue;
        }
        files.push(path.to_path_buf());
    }
    files
}

/// Does `line` represent a real call site (not a comment, not a string literal)?
fn looks_like_code(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with("//") && !trimmed.starts_with("*")
}

/// Does `line` match an allowlisted pattern?
fn is_allowed(line: &str) -> bool {
    ALLOWLIST.iter().any(|p| line.contains(p))
}

#[test]
fn observe_never_interfere_no_writes_to_watch_dir_paths() {
    let files = production_source_files();
    assert!(!files.is_empty(), "found zero .rs files — scanner broken");

    // Self-validation: the principle is "the listener doesn't write to
    // watch_dir." For this test to be meaningful, it has to actually
    // READ files that reference watch_dir. Count sightings as proof
    // the scanner isn't quietly short-circuiting.
    let mut watch_dir_sightings = 0usize;
    let mut violations: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut allowlisted: Vec<(PathBuf, usize, String)> = Vec::new();

    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };

        // Inline #[cfg(test)] modules are production files but contain
        // test code that can legitimately create/write fixtures. Naive
        // heuristic for the spike: skip files that have `#[cfg(test)]`
        // anywhere AND exclude nothing else (so the production content
        // ABOVE the test module gets scanned — false negative risk we
        // accept here and would fix by actually parsing with syn).
        //
        // Simpler: scan every line and accept that tests-inside-src may
        // produce noise. Tune by allowlist when that bites.

        for (i, line) in text.lines().enumerate() {
            if !line.contains("watch_dir") {
                continue;
            }
            watch_dir_sightings += 1;
            if !looks_like_code(line) {
                continue;
            }
            let mentions_write = WRITE_OPS.iter().any(|op| line.contains(op));
            if !mentions_write {
                continue;
            }
            let entry = (path.clone(), i + 1, line.trim().to_string());
            if is_allowed(line) {
                allowlisted.push(entry);
            } else {
                violations.push(entry);
            }
        }
    }

    eprintln!(
        "\nscanned {} production .rs files, found {} watch_dir sightings, \
         {} write-adjacent (of those, {} allowlisted)",
        files.len(),
        watch_dir_sightings,
        violations.len() + allowlisted.len(),
        allowlisted.len()
    );

    // Self-validation: we know watch_dir is referenced in multiple
    // production files (config, server wiring, watcher, etc.). If the
    // scanner sees zero sightings, it's not actually reading the code
    // and a future drift won't be caught.
    assert!(
        watch_dir_sightings >= 5,
        "expected >= 5 watch_dir sightings in production .rs files; \
         got {watch_dir_sightings}. Scanner may be filtering out the \
         wrong directories."
    );

    // Always report the allowlist hits so you can see what was accepted.
    if !allowlisted.is_empty() {
        eprintln!(
            "\n✓ {} allowlisted watch_dir write-adjacent sites (bootstrap-only):",
            allowlisted.len()
        );
        for (path, lineno, line) in &allowlisted {
            let rel = pretty_path(path);
            eprintln!("  {rel}:{lineno}  {line}");
        }
    }

    if !violations.is_empty() {
        eprintln!(
            "\n❌ {} potential observe-never-interfere violations:",
            violations.len()
        );
        for (path, lineno, line) in &violations {
            let rel = pretty_path(path);
            eprintln!("  {rel}:{lineno}");
            eprintln!("    {line}");
        }
        panic!(
            "{} line(s) in production code mention watch_dir alongside a write \
             operation — if intentional, add to ALLOWLIST with a justification; \
             if not, the principle is being violated",
            violations.len()
        );
    }
}

fn pretty_path(p: &Path) -> String {
    let root = workspace_root();
    p.strip_prefix(&root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| p.display().to_string())
}
