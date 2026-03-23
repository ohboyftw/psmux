# Session Resurrection, Hints Mode, Declarative Layouts — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add session resurrection (crash recovery), quick-select hints mode (keyboard-driven copy), and declarative JSON layout files to psmux.

**Architecture:** Three independent modules (`src/resurrection.rs`, `src/hints.rs`, layout extensions in `src/layout.rs`). Each integrates via the existing Mode enum, CtrlReq dispatch, and overlay serialization patterns. No cross-feature dependencies.

**Tech Stack:** Rust, serde/serde_json (existing), regex (existing dep), crossterm, ratatui, Win32 clipboard API.

**Spec:** `docs/superpowers/specs/2026-03-22-three-features-design.md`

---

## File Structure

### New Files
| File | Responsibility |
|------|---------------|
| `src/resurrection.rs` | Snapshot save/load/apply, atomic file writes |
| `src/hints.rs` | Pattern scanning, label assignment, hints key handling |

### Modified Files
| File | Changes |
|------|---------|
| `src/types.rs` | Add `spawn_command`/`spawn_env` to `Pane`, `HintsMode` to `Mode`, `HintMatch`/`HintsState` structs |
| `src/pane.rs` | Populate `spawn_command`/`spawn_env` at pane creation |
| `src/layout.rs` | Add `LayoutFile`/`WindowDef`/`PaneDef`/`TreeDef` types, `load_layout_file()`, `apply_layout_file()` |
| `src/input.rs` | Add `Mode::HintsMode` arm in `handle_key()` |
| `src/server/mod.rs` | Call `save_snapshot()` after structural CtrlReqs, serialize hints overlay, route `.json` source-file |
| `src/client.rs` | Render hints overlay, handle hints key input |
| `src/config.rs` | Add `hint-keys`, `hint-style`, `hint-timeout`, `resurrect-on-exit`, `resurrect-dir` options |
| `src/main.rs` | Add `resurrect` subcommand, `--layout` flag on `new-session`, attach resurrection check |
| `src/cli.rs` | Update help text for new commands/flags |
| `src/lib.rs` | Add `pub mod resurrection;` and `pub mod hints;` |

---

## Task 1: Add spawn metadata to Pane struct

**Files:**
- Modify: `src/types.rs` (Pane struct, ~line 53)
- Modify: `src/pane.rs` (create_window, split helpers)
- Test: `cargo test`

- [ ] **Step 1: Add fields to Pane struct**

In `src/types.rs`, add to the `Pane` struct after the `dead` field:

```rust
    /// Original command string used to spawn this pane (None = default shell)
    pub spawn_command: Option<String>,
    /// Custom environment variables passed via -e at spawn time
    pub spawn_env: Vec<(String, String)>,
```

- [ ] **Step 2: Update all Pane construction sites**

Search for all places that construct `Pane { ... }` in `src/pane.rs` and `src/server/mod.rs`. Add `spawn_command: None, spawn_env: Vec::new()` to each constructor. Then update `create_window()` and `split_active_with_command()` to pass the actual command/env values.

Grep for: `Pane {` across `src/pane.rs`, `src/server/mod.rs`, `src/remote/pane_manager.rs`

- [ ] **Step 3: Populate spawn_command in create_window()**

In `src/pane.rs` `create_window()`, after the `CommandBuilder` is set up, store the command string:

```rust
pane.spawn_command = command.map(|s| s.to_string());
```

- [ ] **Step 4: Populate spawn_env at the CtrlReq handler level**

Env vars are not passed into `split_active_with_command()` directly — they're applied via `crate::util::set_env` at the `CtrlReq::SplitWindow` and `CtrlReq::NewWindow` handler level in `src/server/mod.rs`. After the pane is created, set `spawn_env` on the new pane:

```rust
// In server/mod.rs, after split/create_window succeeds:
if let Some(pane) = active_pane_mut(&mut win.root, &win.active_path) {
    pane.spawn_env = env_vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
}
```

Grep for `CtrlReq::SplitWindow` and `CtrlReq::NewWindow` handlers to find all sites.

- [ ] **Step 5: Run tests, verify build**

Run: `cargo test`
Expected: All existing tests pass. No behavior change.

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/pane.rs src/server/mod.rs src/remote/pane_manager.rs
git commit -m "feat: store spawn_command and spawn_env on Pane for resurrection"
```

---

## Task 2: Session Resurrection — save_snapshot()

**Files:**
- Create: `src/resurrection.rs`
- Modify: `src/lib.rs` (add `pub mod resurrection;`)
- Test: unit tests in `src/resurrection.rs`

- [ ] **Step 1: Write failing roundtrip test**

Create `src/resurrection.rs` with:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct SessionSnapshot {
    pub version: u32,
    pub session_name: String,
    pub timestamp: u64,
    pub windows: Vec<WindowSnapshot>,
    pub active_window_idx: usize,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct WindowSnapshot {
    pub name: String,
    pub id: usize,
    pub active_path: Vec<usize>,
    pub layout_tree: LayoutTreeNode,    // Serialized pane tree topology
    pub pane_commands: Vec<PaneCommand>, // Parallel to leaves in DFS order
}

/// Lightweight serializable tree (no cell content, just topology + sizes).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "type")]
pub enum LayoutTreeNode {
    #[serde(rename = "split")]
    Split {
        kind: String,            // "horizontal" or "vertical"
        sizes: Vec<u16>,
        children: Vec<LayoutTreeNode>,
    },
    #[serde(rename = "leaf")]
    Leaf { id: usize },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PaneCommand {
    pub command: Option<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_snapshot() {
        let snap = SessionSnapshot {
            version: 1,
            session_name: "test".to_string(),
            timestamp: 1234567890,
            windows: vec![WindowSnapshot {
                name: "dev".to_string(),
                id: 0,
                active_path: vec![0],
                pane_commands: vec![
                    PaneCommand {
                        command: Some("cargo watch".to_string()),
                        cwd: "D:/Home/psmux".to_string(),
                        env: vec![("FOO".to_string(), "bar".to_string())],
                    },
                    PaneCommand {
                        command: None,
                        cwd: "D:/Home".to_string(),
                        env: vec![],
                    },
                ],
                layout_tree: LayoutTreeNode::Split {
                    kind: "horizontal".to_string(),
                    sizes: vec![50, 50],
                    children: vec![
                        LayoutTreeNode::Leaf { id: 0 },
                        LayoutTreeNode::Leaf { id: 1 },
                    ],
                },
            }],
            active_window_idx: 0,
        };

        let dir = std::env::temp_dir().join("psmux_test_resurrect");
        let _ = fs::create_dir_all(&dir);
        save_snapshot_to(&snap, &dir).unwrap();
        let loaded = load_snapshot_from("test", &dir).unwrap();
        assert_eq!(snap, loaded);
        let _ = fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Add `pub mod resurrection;` to `src/lib.rs`**

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test roundtrip_snapshot`
Expected: FAIL — `save_snapshot_to` and `load_snapshot_from` not defined yet.

- [ ] **Step 4: Implement save_snapshot_to() and load_snapshot_from()**

```rust
/// Resolve the resurrect directory (default: ~/.psmux/resurrect/)
pub fn resurrect_dir(custom: Option<&str>) -> PathBuf {
    if let Some(d) = custom {
        PathBuf::from(d)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".psmux")
            .join("resurrect")
    }
}

/// Save snapshot atomically: write to .tmp then rename.
pub fn save_snapshot_to(snap: &SessionSnapshot, dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let final_path = dir.join(format!("{}.json", snap.session_name));
    let tmp_path = dir.join(format!("{}.json.tmp", snap.session_name));
    let json = serde_json::to_string_pretty(snap)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(&tmp_path, &json)?;
    // Retry rename once on Windows PermissionDenied (antivirus lock)
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        if e.kind() == io::ErrorKind::PermissionDenied {
            std::thread::sleep(std::time::Duration::from_millis(50));
            fs::rename(&tmp_path, &final_path)?;
        } else {
            return Err(e);
        }
    }
    Ok(())
}

/// Load a snapshot by session name.
pub fn load_snapshot_from(session_name: &str, dir: &Path) -> io::Result<SessionSnapshot> {
    let path = dir.join(format!("{}.json", session_name));
    let json = fs::read_to_string(&path)?;
    serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// List all resurrectable session names.
pub fn list_resurrectable(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect()
}

/// Delete a saved snapshot.
pub fn delete_snapshot(session_name: &str, dir: &Path) -> io::Result<()> {
    let path = dir.join(format!("{}.json", session_name));
    fs::remove_file(&path)
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test roundtrip_snapshot`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/resurrection.rs src/lib.rs
git commit -m "feat: session resurrection save/load/list/delete"
```

---

## Task 3: Session Resurrection — build_snapshot() from AppState

**Files:**
- Modify: `src/resurrection.rs`
- Test: manual (requires running server)

- [ ] **Step 1: Implement build_snapshot()**

Add to `src/resurrection.rs`:

```rust
use crate::types::{AppState, Node};
use crate::tree::active_pane;

/// Build a snapshot from the current server state.
pub fn build_snapshot(app: &AppState) -> SessionSnapshot {
    let mut windows = Vec::new();
    for (i, win) in app.windows.iter().enumerate() {
        let mut pane_commands = Vec::new();
        collect_pane_commands(&win.root, &mut pane_commands);
        let layout_tree = serialize_tree(&win.root);
        // Assert DFS contract: pane_commands must match leaf count
        debug_assert_eq!(pane_commands.len(), count_leaves(&layout_tree));
        windows.push(WindowSnapshot {
            name: win.name.clone(),
            id: win.id,
            active_path: win.active_path.clone(),
            layout_tree,
            pane_commands,
        });
    }
    SessionSnapshot {
        version: 1,
        session_name: app.session_name.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        windows,
        active_window_idx: app.active_idx,
    }
}

fn serialize_tree(node: &Node) -> LayoutTreeNode {
    match node {
        Node::Leaf(pane) => LayoutTreeNode::Leaf { id: pane.id },
        Node::Split { kind, sizes, children } => LayoutTreeNode::Split {
            kind: match kind {
                crate::types::LayoutKind::Horizontal => "horizontal".to_string(),
                crate::types::LayoutKind::Vertical => "vertical".to_string(),
            },
            sizes: sizes.clone(),
            children: children.iter().map(serialize_tree).collect(),
        },
    }
}

fn count_leaves(node: &LayoutTreeNode) -> usize {
    match node {
        LayoutTreeNode::Leaf { .. } => 1,
        LayoutTreeNode::Split { children, .. } => children.iter().map(count_leaves).sum(),
    }
}

fn collect_pane_commands(node: &Node, out: &mut Vec<PaneCommand>) {
    match node {
        Node::Leaf(pane) => {
            let cwd = pane
                .spawn_cwd
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            out.push(PaneCommand {
                command: pane.spawn_command.clone(),
                cwd,
                env: pane.spawn_env.clone(),
            });
        }
        Node::Split { children, .. } => {
            for child in children {
                collect_pane_commands(child, out);
            }
        }
    }
}
```

- [ ] **Step 2: Wire save_snapshot into server structural handlers**

In `src/server/mod.rs`, add after existing `CtrlReq::KillPane`, `CtrlReq::NewWindow`, `CtrlReq::SplitWindow`, `CtrlReq::SelectLayout` handlers, call:

```rust
// After structural change — save resurrection snapshot (best-effort):
let snap = crate::resurrection::build_snapshot(&app);
let dir = crate::resurrection::resurrect_dir(app.resurrect_dir.as_deref());
let _ = crate::resurrection::save_snapshot_to(&snap, &dir);
```

Also call on clean exit (kill-server, detach).

- [ ] **Step 3: Run `cargo clippy` and `cargo test`**

Run: `cargo clippy -- -D warnings && cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/resurrection.rs src/server/mod.rs
git commit -m "feat: build_snapshot from AppState, wire to structural handlers"
```

---

## Task 4: Session Resurrection — resurrect CLI command

**Files:**
- Modify: `src/main.rs` (add `resurrect` subcommand, modify `attach`)
- Modify: `src/cli.rs` (help text)

- [ ] **Step 1: Add `resurrect` subcommand in main.rs**

In the main subcommand match (around line 255), add:

```rust
"resurrect" => {
    let session_name = args.get(1).cloned().unwrap_or_else(|| "default".to_string());
    let dir = crate::resurrection::resurrect_dir(None);
    let snap = crate::resurrection::load_snapshot_from(&session_name, &dir)?;
    eprintln!("Resurrecting session '{}' ({} windows, {} total panes)",
        snap.session_name,
        snap.windows.len(),
        snap.windows.iter().map(|w| w.pane_commands.len()).sum::<usize>());
    // Apply the snapshot: create session, spawn panes with "Press ENTER" banner
    crate::resurrection::apply_snapshot(&mut app, &*pty_system, snap)?;
    Ok(())
}
```

- [ ] **Step 2: Modify `list-sessions` to show resurrectable tag**

In the `ls` / `list-sessions` handler, after listing live sessions, append:

```rust
let dir = crate::resurrection::resurrect_dir(None);
let resurrectable = crate::resurrection::list_resurrectable(&dir);
for name in resurrectable {
    // Skip if a live session with this name exists
    if !live_sessions.contains(&name) {
        println!("{}: (resurrectable)", name);
    }
}
```

- [ ] **Step 3: Add `delete-resurrect` subcommand**

```rust
"delete-resurrect" => {
    let dir = crate::resurrection::resurrect_dir(None);
    if args.get(1).map(|s| s.as_str()) == Some("--all") {
        for name in crate::resurrection::list_resurrectable(&dir) {
            let _ = crate::resurrection::delete_snapshot(&name, &dir);
        }
        eprintln!("All resurrection snapshots deleted.");
    } else if let Some(name) = args.get(1) {
        crate::resurrection::delete_snapshot(name, &dir)?;
        eprintln!("Deleted resurrection snapshot for '{}'.", name);
    } else {
        eprintln!("Usage: psmux delete-resurrect <name|--all>");
    }
    Ok(())
}
```

- [ ] **Step 4: Implement apply_snapshot() in resurrection.rs**

```rust
/// Rebuild a session from a snapshot. Commands are wrapped in a
/// "Press ENTER to run" banner for safety (prevents auto-running rm -rf etc.).
pub fn apply_snapshot(
    app: &mut AppState,
    pty_system: &dyn portable_pty::PtySystem,
    snap: SessionSnapshot,
) -> io::Result<()> {
    app.session_name = snap.session_name;
    for win_snap in &snap.windows {
        // For each window: create panes for each PaneCommand
        for (i, pc) in win_snap.pane_commands.iter().enumerate() {
            let banner_cmd = match &pc.command {
                Some(cmd) => format!("echo 'Press ENTER to run: {}' && read && {}", cmd, cmd),
                None => app.default_shell.clone().unwrap_or_else(|| "bash".to_string()),
            };
            if i == 0 {
                crate::pane::create_window(
                    pty_system, app,
                    Some(&banner_cmd),
                    Some(&pc.cwd),
                    None,
                )?;
            } else {
                crate::pane::split_active(
                    pty_system, app,
                    crate::types::LayoutKind::Vertical,
                    Some(&banner_cmd),
                    Some(&pc.cwd),
                )?;
            }
        }
        // Restore layout topology from saved tree
        // (MVP: use tiled layout as approximation; full tree rebuild in Phase 2)
        if win_snap.pane_commands.len() > 1 {
            crate::layout::apply_layout(app, "tiled");
        }
        // Restore window name
        if let Some(win) = app.windows.last_mut() {
            win.name = win_snap.name.clone();
            win.manual_rename = true;
        }
    }
    if snap.active_window_idx < app.windows.len() {
        app.active_idx = snap.active_window_idx;
    }
    Ok(())
}
```

- [ ] **Step 5: Add attach resurrection check**

In the `attach` handler in `src/main.rs`, after the existing check for live sessions, add:

```rust
// If no live session found, check for resurrectable snapshot
if !session_found {
    let dir = crate::resurrection::resurrect_dir(None);
    if let Ok(snap) = crate::resurrection::load_snapshot_from(&target_session, &dir) {
        eprintln!("Session '{}' has a saved snapshot. Resurrect? [y/N]", target_session);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            crate::resurrection::apply_snapshot(&mut app, &*pty_system, snap)?;
            // Continue to normal attach flow...
        } else {
            std::process::exit(0);
        }
    }
}
```

- [ ] **Step 6: Update CLI help text in cli.rs**

Add to the subcommands section:

```
    resurrect [name]        Restore a session from saved snapshot
    delete-resurrect        Delete resurrection snapshot(s) [name|--all]
```

Add `--layout <file>` to the `new-session` help section.

- [ ] **Step 5: Run `cargo build && cargo test`**

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/cli.rs
git commit -m "feat: resurrect, delete-resurrect CLI commands, list-sessions tag"
```

---

## Task 5: Hints Mode — types and data structures

**Files:**
- Modify: `src/types.rs` (Mode enum, new structs)
- Create: `src/hints.rs` (stub + tests)
- Modify: `src/lib.rs` (add `pub mod hints;`)

- [ ] **Step 1: Add HintMatch and HintsState to types.rs**

```rust
/// A single match found by hints mode pattern scanning.
#[derive(Clone, Debug)]
pub struct HintMatch {
    pub row: u16,
    pub start_col: u16,
    pub end_col: u16,
    pub text: String,
    pub label: String,
}

/// Boxed state for hints mode (keeps Mode enum small).
pub struct HintsState {
    pub matches: Vec<HintMatch>,
    pub input: String,
    pub entered_at: Instant,
}
```

- [ ] **Step 2: Add HintsMode variant to Mode enum**

```rust
    /// Quick-select hints overlay for URLs, paths, hashes
    HintsMode(Box<HintsState>),
```

- [ ] **Step 3: Create src/hints.rs with pattern scanning + tests**

```rust
use regex::Regex;
use crate::types::{AppState, HintMatch, HintsState, Mode};
use std::time::Instant;

const URL_PATTERN: &str = r"https?://[^\s<>\"'\)\]]+";
const PATH_PATTERN: &str = r"(?:[~.][\\/])?[\w\-./\\]*[\\/][\w\-./\\]+\.\w{1,10}(:\d+)?";
const HASH_PATTERN: &str = r"\b[0-9a-f]{7,40}\b";

const DEFAULT_HINT_KEYS: &str = "asdfjkl;";

/// Scan visible text lines for pattern matches, assign labels.
pub fn scan_and_label(lines: &[String], hint_keys: &str) -> Vec<HintMatch> {
    let patterns = [
        Regex::new(URL_PATTERN).unwrap(),
        Regex::new(PATH_PATTERN).unwrap(),
        Regex::new(HASH_PATTERN).unwrap(),
    ];
    let keys: Vec<char> = hint_keys.chars().collect();

    let mut matches = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for pat in &patterns {
            for m in pat.find_iter(line) {
                matches.push(HintMatch {
                    row: row as u16,
                    start_col: m.start() as u16,
                    end_col: m.end() as u16,
                    text: m.as_str().to_string(),
                    label: String::new(), // assigned below
                });
            }
        }
    }

    // Deduplicate by (row, start_col) — keep first pattern match
    matches.sort_by_key(|m| (m.row, m.start_col));
    matches.dedup_by_key(|m| (m.row, m.start_col));

    // Assign labels (cap at n + n*n to avoid duplicate labels)
    let max_labels = keys.len() + keys.len() * keys.len();
    matches.truncate(max_labels);

    let mut label_idx = 0;
    for m in &mut matches {
        m.label = make_label(label_idx, &keys);
        label_idx += 1;
    }

    matches
}

fn make_label(idx: usize, keys: &[char]) -> String {
    let n = keys.len();
    if idx < n {
        keys[idx].to_string()
    } else {
        let first = keys[(idx - n) / n % n];
        let second = keys[(idx - n) % n];
        format!("{}{}", first, second)
    }
}

/// Find the match whose label equals the accumulated input.
pub fn find_match<'a>(matches: &'a [HintMatch], input: &str) -> Option<&'a HintMatch> {
    matches.iter().find(|m| m.label == input)
}

/// Check if any label starts with the given input prefix.
pub fn has_prefix(matches: &[HintMatch], input: &str) -> bool {
    matches.iter().any(|m| m.label.starts_with(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_urls() {
        let lines = vec!["See https://github.com/foo/bar for details".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, "https://github.com/foo/bar");
        assert_eq!(matches[0].label, "a");
    }

    #[test]
    fn scan_file_paths() {
        let lines = vec!["Edit src/main.rs:42 now".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(matches.iter().any(|m| m.text.contains("src/main.rs:42")));
    }

    #[test]
    fn scan_git_hashes() {
        let lines = vec!["commit 8614151 feat: something".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(matches.iter().any(|m| m.text == "8614151"));
    }

    #[test]
    fn labels_unique() {
        let lines = vec!["a https://a.com https://b.com https://c.com https://d.com https://e.com https://f.com https://g.com https://h.com https://i.com".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        let labels: Vec<&str> = matches.iter().map(|m| m.label.as_str()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len(), "labels must be unique");
    }

    #[test]
    fn find_match_exact() {
        let lines = vec!["https://a.com https://b.com".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        let found = find_match(&matches, "a");
        assert!(found.is_some());
        assert_eq!(found.unwrap().text, "https://a.com");
    }

    #[test]
    fn has_prefix_partial() {
        let lines = vec!["https://a.com https://b.com".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(has_prefix(&matches, "a"));
        assert!(!has_prefix(&matches, "z"));
    }
}
```

- [ ] **Step 4: Add `pub mod hints;` to lib.rs**

- [ ] **Step 5: Run tests**

Run: `cargo test -- hints`
Expected: All 6 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/hints.rs src/lib.rs
git commit -m "feat: hints mode types, pattern scanning, label assignment"
```

---

## Task 6: Hints Mode — input handling and clipboard

**Files:**
- Modify: `src/input.rs` (add HintsMode arm)
- Modify: `src/hints.rs` (add enter/exit helpers)

- [ ] **Step 1: Add enter_hints_mode() to hints.rs**

```rust
/// Enter hints mode: scan the active pane's visible output.
pub fn enter_hints_mode(app: &mut AppState) {
    let hint_keys = app.hint_keys.clone();
    let lines = extract_visible_lines(app);
    let matches = scan_and_label(&lines, &hint_keys);
    if matches.is_empty() {
        return; // No matches — stay in current mode
    }
    app.mode = Mode::HintsMode(Box::new(HintsState {
        matches,
        input: String::new(),
        entered_at: Instant::now(),
    }));
}

fn extract_visible_lines(app: &AppState) -> Vec<String> {
    let win = match app.windows.get(app.active_idx) {
        Some(w) => w,
        None => return Vec::new(),
    };
    let pane = match crate::tree::active_pane(&win.root, &win.active_path) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let parser = pane.term.lock().unwrap();
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut lines = Vec::new();
    for row in 0..rows {
        let mut line = String::new();
        for col in 0..cols {
            if let Some(cell) = screen.cell(row, col) {
                line.push_str(cell.contents());
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}
```

- [ ] **Step 2: Add hints command processing in server/mod.rs**

Hints mode uses the client-server pattern: the **client** intercepts keys when `hints_active` is true (Task 7 Step 4) and sends `"hints-input <char>"` or `"overlay-close"` commands to the server. The **server** processes these commands and updates `HintsState`.

In `src/server/mod.rs`, in the command dispatch (where `overlay-close`, `popup-input`, `menu-navigate` etc. are handled), add:

```rust
cmd if cmd.starts_with("hints-input ") => {
    let ch = cmd.trim_start_matches("hints-input ").chars().next().unwrap_or(' ');
    if let Mode::HintsMode(ref mut state) = app.mode {
        state.input.push(ch);
        if let Some(m) = crate::hints::find_match(&state.matches, &state.input) {
            let text = m.text.clone();
            crate::copy_mode::copy_to_system_clipboard(&text);
            if app.set_clipboard != "off" {
                app.clipboard_osc52 = Some(text.clone());
            }
            app.status_message = Some((format!("Copied: {}", text), Instant::now()));
            app.mode = Mode::Passthrough;
        } else if !crate::hints::has_prefix(&state.matches, &state.input) {
            state.input.clear();
        }
        state_dirty = true;
    }
}
```

Also handle `"overlay-close"` for hints mode (add `Mode::HintsMode(_)` to the existing overlay-close match).

No `Mode::HintsMode` arm is needed in `input.rs` `handle_key()` — all hints input flows through the client → server command path.

- [ ] **Step 3: Add hint config fields to AppState**

In `src/types.rs` `AppState`, add:

```rust
    pub hint_keys: String,       // default: "asdfjkl;"
    pub hint_style: String,      // default: "fg=yellow,bold"
    pub hint_timeout: u64,       // ms, default: 5000 (0 = no timeout)
```

Initialize in `AppState::new()` / default constructor.

- [ ] **Step 4: Add config parsing in config.rs**

In `parse_option_value()` match, add:

```rust
"hint-keys" => app.hint_keys = value.to_string(),
"hint-style" => app.hint_style = value.to_string(),
"hint-timeout" => {
    if let Ok(ms) = value.parse::<u64>() {
        app.hint_timeout = ms;
    }
}
"resurrect-on-exit" => app.resurrect_on_exit = matches!(value, "on" | "true" | "1"),
"resurrect-dir" => app.resurrect_dir = Some(value.to_string()),
```

- [ ] **Step 5: Add default prefix keybinding**

In the prefix keybinding defaults (wherever `bind-key` defaults are set up), add:

```rust
// Ctrl+b f → hints mode
("f", KeyModifiers::empty()) => Action::Command("hints".to_string()),
```

And add the `"hints"` command to the action executor to call `crate::hints::enter_hints_mode(app)`.

- [ ] **Step 6: Run `cargo clippy && cargo test`**

- [ ] **Step 7: Commit**

```bash
git add src/input.rs src/hints.rs src/types.rs src/config.rs
git commit -m "feat: hints mode input handling, clipboard, config options"
```

---

## Task 7: Hints Mode — overlay rendering (server + client)

**Files:**
- Modify: `src/server/mod.rs` (serialize hints state in overlay JSON)
- Modify: `src/client.rs` (render hints overlay)

- [ ] **Step 1: Serialize hints state in serialize_overlay_json()**

In `src/server/mod.rs` `serialize_overlay_json()`, add a `Mode::HintsMode` arm:

```rust
Mode::HintsMode(ref state) => {
    let hints_json: Vec<String> = state.matches.iter().map(|m| {
        format!(
            r#"{{"row":{},"start_col":{},"end_col":{},"label":"{}","text":"{}"}}"#,
            m.row, m.start_col, m.end_col,
            m.label.replace('"', "\\\""),
            m.text.replace('"', "\\\"")
        )
    }).collect();
    format!(
        r#","hints_active":true,"hints_input":"{}","hints":[{}]"#,
        state.input.replace('"', "\\\""),
        hints_json.join(",")
    )
}
```

- [ ] **Step 2: Parse hints state in client.rs**

In the `FrameState` struct in `src/client.rs`, add:

```rust
    hints_active: bool,
    hints_input: String,
    hints: Vec<ClientHintMatch>,
```

With:

```rust
struct ClientHintMatch {
    row: u16,
    start_col: u16,
    end_col: u16,
    label: String,
}
```

Parse from the overlay JSON alongside existing popup/menu parsing.

- [ ] **Step 3: Render hints overlay in client.rs**

In the rendering section of `client.rs` (after popup/menu rendering), add:

```rust
if srv_hints_active {
    // Dim all cells in the content area
    for y in content_chunk.y..content_chunk.y + content_chunk.height {
        for x in content_chunk.x..content_chunk.x + content_chunk.width {
            if let Some(cell) = f.buffer_mut().cell_mut(ratatui::layout::Position { x, y }) {
                cell.set_fg(ratatui::style::Color::DarkGray);
            }
        }
    }
    // Overlay labels at match positions
    let label_style = parse_tmux_style(&hint_style_str);
    for hint in &srv_hints {
        let y = content_chunk.y + hint.row;
        for (i, ch) in hint.label.chars().enumerate() {
            let x = content_chunk.x + hint.start_col + i as u16;
            if let Some(cell) = f.buffer_mut().cell_mut(ratatui::layout::Position { x, y }) {
                cell.set_char(ch);
                cell.set_style(label_style);
            }
        }
    }
}
```

- [ ] **Step 4: Handle hints keys in client.rs**

In the client key handling section (where popup/menu keys are handled), add before the passthrough:

```rust
if srv_hints_active {
    match key.code {
        KeyCode::Esc => {
            cmd_batch.push("overlay-close\n".to_string());
        }
        KeyCode::Char(c) => {
            cmd_batch.push(format!("hints-input {}\n", c));
        }
        _ => {}
    }
    continue;
}
```

- [ ] **Step 5: Add hints timeout check in server event loop**

In `src/server/mod.rs`, in the main event loop (near where `display-panes-time` / `PaneChooser` timeout is checked), add:

```rust
if let Mode::HintsMode(ref state) = app.mode {
    if app.hint_timeout > 0 && state.entered_at.elapsed().as_millis() as u64 >= app.hint_timeout {
        app.mode = Mode::Passthrough;
        state_dirty = true;
    }
}
```

- [ ] **Step 6: Run `cargo build && cargo test`**

- [ ] **Step 7: Commit**

```bash
git add src/server/mod.rs src/client.rs
git commit -m "feat: hints mode overlay rendering, client key handling, timeout"
```

---

## Task 8: Declarative Layout Files — schema and parser

**Files:**
- Modify: `src/layout.rs` (add types and parser)
- Test: unit tests in `src/layout.rs`

- [ ] **Step 1: Write failing parse test**

Add to `src/layout.rs`:

```rust
#[derive(Deserialize, Debug)]
pub struct LayoutFile {
    pub version: Option<u32>,
    pub session: Option<String>,
    pub windows: Vec<WindowDef>,
}

#[derive(Deserialize, Debug)]
pub struct WindowDef {
    pub name: Option<String>,
    pub layout: Option<String>,
    pub panes: Option<Vec<PaneDef>>,
    pub tree: Option<TreeDef>,
}

#[derive(Deserialize, Debug)]
pub struct PaneDef {
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum TreeDef {
    #[serde(rename = "split")]
    Split {
        split: String,
        sizes: Vec<u16>,
        children: Vec<TreeDef>,
    },
    #[serde(rename = "pane")]
    Leaf {
        command: Option<String>,
        cwd: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    },
}

pub fn load_layout_file(path: &str) -> io::Result<LayoutFile> {
    let json = std::fs::read_to_string(path)?;
    serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod layout_file_tests {
    use super::*;

    #[test]
    fn parse_simple_layout() {
        let json = r#"{
            "session": "test",
            "windows": [{
                "name": "dev",
                "layout": "tiled",
                "panes": [
                    { "command": "bash" },
                    { "command": "htop", "cwd": "/tmp" }
                ]
            }]
        }"#;
        let layout: LayoutFile = serde_json::from_str(json).unwrap();
        assert_eq!(layout.session.as_deref(), Some("test"));
        assert_eq!(layout.windows.len(), 1);
        assert_eq!(layout.windows[0].panes.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn parse_tree_layout() {
        let json = r#"{
            "windows": [{
                "name": "custom",
                "tree": {
                    "type": "split", "split": "horizontal", "sizes": [60, 40],
                    "children": [
                        { "type": "pane", "command": "vim" },
                        { "type": "pane", "command": "bash" }
                    ]
                }
            }]
        }"#;
        let layout: LayoutFile = serde_json::from_str(json).unwrap();
        assert!(layout.windows[0].tree.is_some());
    }

    #[test]
    fn parse_defaults() {
        let json = r#"{ "windows": [{ "panes": [{}] }] }"#;
        let layout: LayoutFile = serde_json::from_str(json).unwrap();
        assert_eq!(layout.version, None);
        assert_eq!(layout.session, None);
        assert_eq!(layout.windows[0].name, None);
        assert_eq!(layout.windows[0].panes.as_ref().unwrap()[0].command, None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test layout_file_tests`
Expected: PASS (pure serde parsing, no side effects).

- [ ] **Step 3: Commit**

```bash
git add src/layout.rs
git commit -m "feat: declarative layout file JSON schema and parser"
```

---

## Task 9: Declarative Layout Files — apply_layout_file()

**Files:**
- Modify: `src/layout.rs`
- Modify: `src/main.rs` (--layout flag on new-session)
- Modify: `src/server/mod.rs` (source-file routing)

- [ ] **Step 1: Implement apply_layout_file()**

This function follows the same patterns as `CtrlReq::NewWindow` and `CtrlReq::SplitWindow` handlers in `server/mod.rs`. Study those handlers first to match exact `create_window()` and split function signatures.

```rust
const VALID_LAYOUTS: [&str; 5] = [
    "even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled",
];

/// Apply a layout file: creates windows and panes.
pub fn apply_layout_file(
    app: &mut AppState,
    pty_system: &dyn portable_pty::PtySystem,
    layout: LayoutFile,
) -> io::Result<()> {
    for win_def in &layout.windows {
        // Validate layout name if specified
        if let Some(ref name) = win_def.layout {
            if !VALID_LAYOUTS.contains(&name.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Unknown layout '{}'. Valid: {}", name, VALID_LAYOUTS.join(", ")),
                ));
            }
        }

        if let Some(ref tree) = win_def.tree {
            // Validate tree sizes
            validate_tree_sizes(tree)?;
            // Custom tree path: build Node tree recursively, spawning panes
            apply_tree_def(app, pty_system, tree)?;
        } else if let Some(ref panes) = win_def.panes {
            // Flat panes + named layout path
            // 1. Create first pane as a new window
            let first = &panes[0];
            crate::pane::create_window(
                pty_system, app,
                first.command.as_deref(),
                first.cwd.as_deref(),
                None, // shell_override
            )?;
            // 2. Split for each additional pane
            for pane_def in panes.iter().skip(1) {
                crate::pane::split_active(
                    pty_system, app,
                    crate::types::LayoutKind::Vertical,
                    pane_def.command.as_deref(),
                    pane_def.cwd.as_deref(),
                )?;
            }
            // 3. Apply named layout to rearrange the flat splits
            let layout_name = win_def.layout.as_deref().unwrap_or("tiled");
            if panes.len() > 1 {
                apply_layout(app, layout_name);
            }
        } else {
            // No panes, no tree: create empty default shell window
            crate::pane::create_window(pty_system, app, None, None, None)?;
        }

        // Set window name
        if let Some(ref name) = win_def.name {
            if let Some(win) = app.windows.last_mut() {
                win.name = name.clone();
                win.manual_rename = true;
            }
        }
    }
    Ok(())
}

fn validate_tree_sizes(tree: &TreeDef) -> io::Result<()> {
    if let TreeDef::Split { sizes, children, .. } = tree {
        if sizes.len() != children.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sizes length ({}) != children length ({})", sizes.len(), children.len()),
            ));
        }
        if sizes.iter().any(|&s| s == 0) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "sizes cannot contain 0"));
        }
        for child in children {
            validate_tree_sizes(child)?;
        }
    }
    Ok(())
}

fn apply_tree_def(
    app: &mut AppState,
    pty_system: &dyn portable_pty::PtySystem,
    tree: &TreeDef,
) -> io::Result<()> {
    // Implementation: recursively walk TreeDef, creating panes and splits.
    // Follow the same pattern as the tmux layout string restore in
    // parse_tmux_layout_string() — collect leaves, build Node tree, assign panes.
    // This requires studying the existing tree-building code in layout.rs
    // (parse_tmux_layout_string, lines 1287-1449) and adapting it for TreeDef.
    match tree {
        TreeDef::Leaf { command, cwd, .. } => {
            crate::pane::create_window(
                pty_system, app,
                command.as_deref(),
                cwd.as_deref(),
                None,
            )?;
        }
        TreeDef::Split { split, sizes, children } => {
            // Create first child (as window or pane)
            apply_tree_def(app, pty_system, &children[0])?;
            // Split for remaining children
            let kind = if split == "horizontal" {
                crate::types::LayoutKind::Horizontal
            } else {
                crate::types::LayoutKind::Vertical
            };
            for child in children.iter().skip(1) {
                if let TreeDef::Leaf { command, cwd, .. } = child {
                    crate::pane::split_active(
                        pty_system, app, kind,
                        command.as_deref(), cwd.as_deref(),
                    )?;
                } else {
                    // Nested split: split first, then restructure
                    // This is complex — for MVP, flatten nested trees
                    // into sequential splits. Full tree reconstruction
                    // can be added later using the same approach as
                    // parse_tmux_layout_string().
                    apply_tree_def(app, pty_system, child)?;
                }
            }
        }
    }
    Ok(())
}
```

Note: `split_active()` signature must match the actual function — check `src/pane.rs` for exact parameters. The tree path for deeply nested custom layouts is best-effort in MVP; the flat `panes` + `layout` path covers 90% of use cases.

- [ ] **Step 2: Add `--layout` flag to new-session in main.rs**

In the `new-session` flag parsing loop (~line 496+), add:

```rust
"--layout" => {
    if let Some(path) = args_iter.next() {
        layout_file_path = Some(path.to_string());
    }
}
```

Then after session creation, if `layout_file_path` is set:

```rust
if let Some(ref path) = layout_file_path {
    let layout = crate::layout::load_layout_file(path)?;
    crate::layout::apply_layout_file(&mut app, &*pty_system, layout)?;
}
```

- [ ] **Step 3: Route .json files in source-file handler**

In `src/server/mod.rs`, in the `CtrlReq::SourceFile` handler, add before the existing `source_file()` call:

```rust
if path.ends_with(".json") {
    match crate::layout::load_layout_file(&path) {
        Ok(layout) => {
            if let Err(e) = crate::layout::apply_layout_file(&mut app, &*pty_system, layout) {
                eprintln!("Layout error: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to load layout file: {}", e),
    }
} else {
    source_file(&mut app, &path);
}
```

- [ ] **Step 4: Run `cargo clippy && cargo test`**

- [ ] **Step 5: Commit**

```bash
git add src/layout.rs src/main.rs src/server/mod.rs
git commit -m "feat: apply_layout_file, --layout flag, source-file .json routing"
```

---

## Task 10: Integration testing and cleanup

**Files:**
- Modify: `src/cli.rs` (final help text updates)
- All files: `cargo fmt && cargo clippy -- -D warnings && cargo test`

- [ ] **Step 1: Update all CLI help text**

Ensure `src/cli.rs` documents:
- `resurrect [name]`
- `delete-resurrect [name|--all]`
- `new-session --layout <file>`
- `source-file <path>` (note: `.json` files are treated as layout files)
- `Ctrl+b f` → hints mode in the keybindings help

- [ ] **Step 2: Run full check suite**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
```

- [ ] **Step 3: Manual smoke test**

1. `psmux new-session -s test` → split a few panes → `psmux kill-server` → check `~/.psmux/resurrect/test.json` exists
2. `psmux resurrect test` → verify session layout is recreated
3. In a running session, `Ctrl+b f` → verify hints appear on URLs/paths
4. Create a `test-layout.json` → `psmux new-session --layout test-layout.json` → verify panes spawn correctly

- [ ] **Step 4: Final commit**

```bash
git add src/cli.rs
git commit -m "docs: update CLI help for resurrection, hints, layout features"
```

---

## Summary

| Task | Feature | Est. Steps |
|------|---------|-----------|
| 1 | Spawn metadata on Pane | 6 |
| 2 | Resurrection save/load | 6 |
| 3 | Resurrection build_snapshot | 4 |
| 4 | Resurrection CLI | 6 |
| 5 | Hints types + scanning | 6 |
| 6 | Hints input + clipboard | 7 |
| 7 | Hints overlay rendering | 6 |
| 8 | Layout schema + parser | 3 |
| 9 | Layout apply + CLI | 5 |
| 10 | Integration + cleanup | 4 |
| **Total** | | **53 steps** |
