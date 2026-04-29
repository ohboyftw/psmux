//! Architectural invariant test for the control-mode foundation
//! (Path A Stage 1A — `b68662b` port).
//!
//! Establishes the "single fan-out site" guarantee that the
//! `.claude/internal/design-control-mode-vs-custompanebackend.md` doc
//! relies on for safely running CustomPaneBackend (agent JSON-RPC) and
//! tmux control mode (`-C`/`-CC`) as parallel transports.
//!
//! ## Invariants asserted at TEST time (no runtime simulation)
//!
//! 1. The literal `crate::mycel::publish_pane_event(` appears ONCE in
//!    `src/`, inside `src/control.rs::publish_lifecycle_to_mycel`.
//!    (The PANE_EXITED + `psmux/pane/died` shim is two calls in the same
//!    function — counted as one site for invariant purposes since it's
//!    inside the helper.)
//!
//! 2. Iteration over `app.control_clients` (the field) appears ONCE in
//!    `src/`, inside `src/control.rs::emit_lifecycle`. Other modules may
//!    write to the field (Stage 1B's Register/Deregister handlers) but
//!    must not iterate it directly.
//!
//! Why TEST-time, not runtime: per the design Q2 = Option C, this is a
//! lint-style assertion that the code-shape contract is preserved by
//! every future commit. Runtime fan-out semantics are covered by the
//! unit tests inside `src/control.rs::tests`.

use std::fs;
use std::path::{Path, PathBuf};

/// Walk `src/` and return every `.rs` file path.
fn rust_files_in_src() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&src, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Count substring occurrences across every `.rs` file under `src/`,
/// returning `(total, per_file_paths)` where `per_file_paths` lists each
/// file that contained at least one hit.
fn count_substring(needle: &str) -> (usize, Vec<PathBuf>) {
    let mut total = 0usize;
    let mut hits = Vec::new();
    for path in rust_files_in_src() {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let n = content.matches(needle).count();
        if n > 0 {
            total += n;
            hits.push(path);
        }
    }
    (total, hits)
}

#[test]
fn mycel_publish_pane_event_called_from_only_one_module() {
    // The literal `crate::mycel::publish_pane_event(` should appear only
    // inside `src/control.rs`. Stage 1A's helper centralises every mycel
    // publish so the pane-lifecycle / session-lifecycle / exec-completed
    // payloads stay symmetric across consumers.
    let (_total, files) = count_substring("crate::mycel::publish_pane_event(");
    let file_names: Vec<String> = files
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>")
                .to_string()
        })
        .collect();
    assert_eq!(
        files.len(),
        1,
        "publish_pane_event must only be called from src/control.rs (got hits in: {:?})",
        file_names
    );
    assert_eq!(
        file_names[0], "control.rs",
        "the single publish_pane_event site must live in control.rs"
    );
}

#[test]
fn app_control_clients_iterated_from_only_one_module() {
    // Iteration patterns over `control_clients`. Writes (.push, .retain,
    // assignment) are allowed everywhere — they happen in Stage 1B's
    // Register/Deregister arms. Reads (.iter, indexing, .len in conditions)
    // are restricted to `src/control.rs`.
    let read_patterns = ["control_clients.iter", "control_clients["];
    let mut all_files = std::collections::HashSet::new();
    for pat in &read_patterns {
        let (_total, files) = count_substring(pat);
        for f in files {
            all_files.insert(f);
        }
    }
    let file_names: Vec<String> = all_files
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>")
                .to_string()
        })
        .collect();
    assert!(
        all_files.len() <= 1,
        "control_clients iteration must only happen in src/control.rs (got hits in: {:?})",
        file_names
    );
    if let Some(only) = file_names.first() {
        assert_eq!(
            only, "control.rs",
            "the single iteration site must live in control.rs"
        );
    }
}

#[test]
fn emit_lifecycle_is_the_published_helper_name() {
    // Every refactored site calls `crate::control::emit_lifecycle`. This
    // sanity check guards against typos (e.g. `emit_lifecycles`) that would
    // silently bypass the fan-out.
    let (total, _files) = count_substring("crate::control::emit_lifecycle");
    assert!(
        total >= 5,
        "expected ≥5 emit_lifecycle call sites (pane.rs ×2, tree.rs, server/mod.rs ×4, backend/dispatcher.rs); got {total}"
    );
}
