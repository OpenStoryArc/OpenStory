//! PRINCIPLE TEST — "Functional-first, side effects at the edges."
//!
//! CLAUDE.md (principle #4):
//!
//!   > Core logic is pure functions: data in, data out. `translate.rs`,
//!   > `lib/`, `streams/` — no side effects.
//!   > Side effects (file I/O, network, DOM) live at the actor
//!   > boundaries: `watcher.rs`, `connection.ts`, React components.
//!
//! This test scans a declared list of pure modules for forbidden I/O
//! operations. If a function in a pure module reaches for the
//! filesystem, network, or process, the principle is violated.
//!
//! The EDGE modules (watcher, consumers, state, persistence) are where
//! I/O legitimately lives. They're excluded from this test by design
//! — they're the boundary. The pure modules in the list below are the
//! interior: translation, type definitions, record transforms,
//! pattern detection, filter classification, and projection folding.
//!
//! Diagnostic output (`eprintln!`/`println!`) is pragmatic-tolerated
//! and not flagged — stderr logging doesn't corrupt the functional
//! story enough to justify forbidding it in core.

use std::path::{Path, PathBuf};

/// Modules declared to be pure — no I/O beyond diagnostic printing.
/// Listed as paths relative to the Rust workspace root (rs/).
const PURE_MODULES: &[&str] = &[
    // open-story-core: types, translators, subtypes, string helpers
    "core/src/cloud_event.rs",
    "core/src/event_data.rs",
    "core/src/subtype.rs",
    "core/src/strings.rs",
    "core/src/paths.rs",
    "core/src/translate.rs",
    "core/src/translate_pi.rs",
    "core/src/translate_hermes.rs",
    // open-story-views: CloudEvent → ViewRecord transform + tool input parsing
    "views/src/from_cloud_event.rs",
    "views/src/unified.rs",
    "views/src/view_record.rs",
    "views/src/wire_record.rs",
    "views/src/tool_input.rs",
    "views/src/pair.rs",
    "views/src/filter.rs",
    "views/src/markdown.rs",
    // open-story-patterns: state machine + sentence builder
    "patterns/src/eval_apply.rs",
    "patterns/src/sentence.rs",
    // open-story-store: projection fold + extraction helpers + pure query shaping
    "store/src/extract.rs",
    "store/src/projection.rs",
];

/// Forbidden I/O patterns. Matched as substrings on code lines.
/// `eprintln!`/`println!` intentionally not forbidden — diagnostic
/// logging is tolerated even in pure cores.
const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    ("std::fs::", "filesystem I/O"),
    ("tokio::fs::", "async filesystem I/O"),
    ("File::open", "filesystem read"),
    ("File::create", "filesystem write"),
    ("OpenOptions::new", "filesystem write"),
    ("reqwest::", "network I/O (HTTP)"),
    ("async_nats::", "network I/O (NATS)"),
    ("std::process::Command", "subprocess"),
    ("std::net::", "network I/O"),
    ("tokio::net::", "async network I/O"),
    ("std::env::var", "environment variable (external state)"),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn resolve_pure_module(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

/// True if the line is a Rust comment (single-line or doc).
fn looks_like_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("*")
}

/// Scan a file for forbidden patterns. Skip once we hit `#[cfg(test)]`
/// — the tests submodule is allowed to use filesystem, tempfile, etc.
/// (They're tests, not pure-logic code.)
fn scan_file(path: &Path) -> Vec<(usize, String, &'static str)> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return vec![],
    };

    let mut findings = Vec::new();
    let mut in_test_block = false;

    for (i, line) in text.lines().enumerate() {
        // Approximate: once any line declares a test module, bail out.
        // Works because our pure modules put `#[cfg(test)] mod tests`
        // at the bottom of the file.
        if line.contains("#[cfg(test)]") || line.trim_start().starts_with("mod tests") {
            in_test_block = true;
        }
        if in_test_block {
            continue;
        }
        if looks_like_comment(line) {
            continue;
        }
        for (pat, reason) in FORBIDDEN_PATTERNS {
            if line.contains(pat) {
                findings.push((i + 1, line.trim().to_string(), *reason));
                break;
            }
        }
    }

    findings
}

#[test]
fn functional_purity_pure_modules_contain_no_io() {
    let mut total_violations = 0usize;
    let mut by_file: Vec<(&str, Vec<(usize, String, &'static str)>)> = Vec::new();

    for rel in PURE_MODULES {
        let path = resolve_pure_module(rel);
        assert!(
            path.is_file(),
            "PURE_MODULES contains a nonexistent path: {rel}. \
             If a pure module was renamed or removed, update the list."
        );
        let findings = scan_file(&path);
        total_violations += findings.len();
        by_file.push((rel, findings));
    }

    // Always report — pure sanity signal for the scanner.
    eprintln!(
        "\nscanned {} declared-pure modules; {} total I/O call sites found",
        PURE_MODULES.len(),
        total_violations
    );

    if total_violations > 0 {
        eprintln!("\n❌ functional-purity violations:");
        for (rel, findings) in &by_file {
            if findings.is_empty() {
                continue;
            }
            eprintln!("  {rel}:");
            for (lineno, line, reason) in findings {
                eprintln!("    {lineno}  [{reason}]");
                eprintln!("        {line}");
            }
        }
        panic!(
            "{total_violations} I/O operation(s) found in declared-pure modules. \
             Either move the logic to an edge (watcher, consumer, state) or \
             remove it from PURE_MODULES with a note."
        );
    }
}
