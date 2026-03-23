# psmux Three-Feature Design: Resurrection, Hints, Declarative Layouts

**Date**: 2026-03-22
**Status**: Draft
**Branch**: ohboy-builds

## Overview

Three independent features to add to psmux, each addressing a distinct gap identified through competitive analysis (Zellij, WezTerm, Kitty, Ghostty) and real-world usage patterns (1-2 GB long-running agent sessions, agent swarm orchestration, daily terminal workflow).

**Approach**: Independent modules, no cross-feature dependencies for MVP. Schemas converge later (resurrection outputs become loadable layout files in a future phase).

---

## Feature 1: Session Resurrection

### Problem

psmux sessions are lost on crash, `kill-server`, or system restart. With agent swarms running across multiple panes, losing a session means losing workspace state, command history context, and having to manually reconstruct the layout.

### Prerequisite: Pane Spawn Metadata

The existing `Pane` struct (`src/types.rs`) does not store the original spawn command or custom env vars — they are consumed during `CommandBuilder` setup and discarded. Two new fields must be added to `Pane`:

```rust
// Added to Pane struct in src/types.rs
pub spawn_command: Option<String>,           // Original command string (None = default shell)
pub spawn_env: Vec<(String, String)>,        // Custom env vars passed via -e
```

These fields are populated at spawn time in `create_window()`, `create_window_raw()`, and the split helpers in `pane.rs`. The existing `spawn_cwd: Option<PathBuf>` already covers working directory.

### Data Model

```rust
// src/resurrection.rs

#[derive(Serialize, Deserialize)]
struct SessionSnapshot {
    version: u32,                       // Schema version (starts at 1)
    session_name: String,
    timestamp: u64,                     // Unix epoch seconds
    windows: Vec<WindowSnapshot>,
    active_window_idx: usize,
}

#[derive(Serialize, Deserialize)]
struct WindowSnapshot {
    name: String,
    id: usize,
    layout: LayoutJson,                // Existing LayoutJson from layout.rs (tree structure)
    active_path: Vec<usize>,
    pane_commands: Vec<PaneCommand>,    // Parallel to tree leaves in DFS order
}

#[derive(Serialize, Deserialize)]
struct PaneCommand {
    command: Option<String>,            // Original command string (None = default shell)
    cwd: String,                        // Working directory at snapshot time
    env: Vec<(String, String)>,         // Custom env vars passed via -e
}
```

**DFS traversal contract**: `pane_commands` entries correspond 1:1 with leaf nodes in `layout` in depth-first order. `save_snapshot()` must assert `pane_commands.len() == leaf_count(layout)`.

No scrollback serialization in MVP. Snapshot files stay small (<10 KB).

### Schema Versioning

Version starts at 1. Unknown fields are silently ignored (`#[serde(deny_unknown_fields)]` is NOT used). On load, if `version > SUPPORTED_VERSION`, log a warning and attempt best-effort restoration. Future versions (e.g., adding scrollback) increment the version but remain backward-compatible for the fields that exist in v1.

### Save Triggers

**Phase 1 (MVP)** — Structural changes only:
- `split-window`, `kill-pane`, `new-window`, `close-window`
- `select-layout`, `swap-pane`, `move-pane`
- `kill-server`, `detach` (clean exit)

After the server processes each of these `CtrlReq` variants, call `save_snapshot()`.

**Phase 2** — Add periodic timer:
- Every 30 seconds in the server event loop, re-save current state
- Catches cwd changes and env mutations that structural triggers miss

### Storage

- **Path**: `~/.psmux/resurrect/<session_name>.json`
- **Atomic write**: Write to `<session_name>.json.tmp`, then `fs::rename()` to final path. On Windows, `fs::rename()` uses `MoveFileExW` which can fail if the destination is held open (e.g., antivirus). `save_snapshot()` should retry once after a short sleep on `PermissionDenied` errors.
- **On crash**: Last good snapshot survives (rename is atomic on same-volume NTFS)
- **Cleanup**: Snapshots deleted when session exits cleanly (configurable: `set -g resurrect-on-exit off` to keep)

### Restore Flow

1. `psmux attach -t <name>` — if no live session exists but a snapshot does, prompt: "Session `<name>` has a saved snapshot. Resurrect? [y/N]"
2. `psmux resurrect <name>` — explicit restore, no prompt
3. Restoration steps:
   a. Read and parse `~/.psmux/resurrect/<name>.json`
   b. Create new session with saved name
   c. For each window: build pane tree from `LayoutJson`, spawn each leaf's command in its saved cwd
   d. Commands are NOT auto-run — show "Press ENTER to run: `<command>`" banner (Zellij pattern, prevents dangerous re-execution of `rm -rf` etc.)
4. `psmux list-sessions` shows `(resurrectable)` next to dead sessions that have snapshots

### CLI

```
psmux resurrect [session_name]      # Restore a dead session from snapshot
psmux list-sessions                 # Shows (resurrectable) tag for available snapshots
psmux delete-resurrect [name|--all] # Delete snapshot(s)
```

### Config

```
set -g resurrect-on-exit on         # Keep snapshot after clean exit (default: off)
set -g resurrect-dir "~/.psmux/resurrect"  # Snapshot directory
```

### Module

New file: `src/resurrection.rs`
- `save_snapshot(app: &AppState)` — serialize current state, atomic write
- `load_snapshot(session_name: &str) -> Result<SessionSnapshot>` — read and parse
- `apply_snapshot(app: &mut AppState, snapshot: SessionSnapshot)` — rebuild session
- `list_resurrectable() -> Vec<String>` — scan resurrect directory
- `delete_snapshot(session_name: &str)` — remove snapshot file

Integration points:
- `src/server/mod.rs` — call `save_snapshot()` after structural `CtrlReq` handlers
- `src/main.rs` — add `resurrect` subcommand, modify `attach` to check for snapshots
- `src/cli.rs` — add `resurrect` and `delete-resurrect` to CLI help

---

## Feature 2: Quick Select / Hints Mode

### Problem

Grabbing URLs, file paths, and git hashes from terminal output requires entering copy mode, navigating to the text, selecting it character by character, and yanking. This is slow — especially when agent output is dense with actionable references.

### Mode

New enum variant using boxed state to avoid bloating the `Mode` enum:

```rust
struct HintsState {
    matches: Vec<HintMatch>,
    labels: Vec<String>,
    input: String,
    entered_at: Instant,    // For timeout tracking
}

// In Mode enum:
HintsMode(Box<HintsState>),
```

### Entry & Exit

- **Enter**: `Ctrl+b f` (new prefix binding) or `:hints` command
- **Exit**: `Esc`, successful selection, or timeout
- **Timeout**: 5 seconds default, configurable `set -g hint-timeout 5000` (ms, 0 = no timeout)

### Pattern Scanning

Three built-in regex patterns, applied in order across visible cells of the active pane:

| Pattern | Regex | Examples |
|---------|-------|---------|
| URL | `https?://[^\s<>"'\)\]]+` | `https://github.com/foo/bar` |
| File path | `(?:[~.][\\/])?[\w\-./\\]*[\\/][\w\-./\\]+\.\w{1,10}(:\d+)?` | `src/main.rs:42`, `./foo/bar.txt` |
| Git hash | `\b[0-9a-f]{7,40}\b` | `8614151`, `27378bc` |

**Scanning source**: Read visible cells from the active pane's VT100 parser screen (same data source as `capture-pane`). Reconstruct lines, apply regexes, record match positions (row, start_col, end_col, matched_text).

**Deduplication**: Same text appearing multiple times gets multiple hints (each position independently selectable).

**Ordering**: Top-left to bottom-right.

### Label Assignment

Labels use home-row characters for ergonomic reach:

- Single-char labels first: `a`, `s`, `d`, `f`, `j`, `k`, `l`, `;`
- Two-char labels: `aa`, `as`, `ad`, `af`, `aj`, ...
- Max ~64 hints per screen (8 single + 56 two-char)
- Configurable: `set -g hint-keys "asdfjkl;"` (default: home row)

### Rendering

**Client-server architecture note**: psmux uses a client-server model where the server serializes state as JSON (`dump_layout_json_fast()`) and the client renders via ratatui. Hints mode state must be serialized into the dump-state JSON (following the same pattern as `PopupMode`/`MenuMode` in `serialize_overlay_json()`). The client-side rendering code reads the serialized hint matches and renders the overlay.

When `Mode::HintsMode` is active, the rendering path calls `render_hints_overlay()`:

1. Dim all visible cells (set foreground to dark gray)
2. For each match, overlay the label text at the match's start position using `hint-style` (default: `fg=yellow,bold`)
3. The label replaces the first N characters of the match visually (not in the actual terminal buffer)

```
Before hints:                     After Ctrl+b f:
───────────────────               ───────────────────
8614151 feat: Tier 0              [a] feat: Tier 0
919820f fix: television           [s] fix: television
See https://github.com/foo        See [d]://github.com/foo
Edit src/main.rs:42               Edit [f]main.rs:42
```

### Selection

1. User types label character(s) — `input` field accumulates keystrokes
2. On unique match: copy matched text to system clipboard via `copy_to_system_clipboard()` AND set `app.clipboard_osc52` for OSC 52 delivery (mirrors the existing yank pattern in copy mode — required for SSH/remote scenarios where the server is detached from the user's terminal)
3. Status line shows "Copied: `<value>`" for 2 seconds
4. Mode exits automatically

If the typed characters don't match any label prefix, beep and stay in hints mode.

### Config

```
set -g hint-keys "asdfjkl;"              # Label characters
set -g hint-style "fg=yellow,bold"       # Label highlight style
set -g hint-timeout 5000                 # Timeout in ms (0 = no timeout)
```

### Module

New file: `src/hints.rs`
- `enter_hints_mode(app: &mut AppState)` — scan visible cells, build matches, assign labels, set mode
- `render_hints_overlay(f: &mut Frame, app: &AppState, area: Rect)` — overlay labels on screen
- `handle_hints_key(app: &mut AppState, key: KeyEvent) -> io::Result<bool>` — process label input, copy, exit

Integration points:
- `src/types.rs` — add `Mode::HintsMode` variant and `HintMatch` struct
- `src/input.rs` — add `Mode::HintsMode` arm in `handle_key()`, delegate to `handle_hints_key()`
- `src/rendering.rs` — call `render_hints_overlay()` when in hints mode
- `src/config.rs` — add `hint-keys`, `hint-style`, `hint-timeout` options
- Default keybinding: `bind-key f hints` in prefix table

### Dependencies

- `regex` crate (already in `Cargo.toml` or add if missing — lightweight, no features needed)

---

## Feature 3: Declarative Layout Files

### Problem

Setting up multi-pane agent workspaces requires manually running `new-session`, `split-window`, `send-keys` sequences every time. No way to define a reusable workspace configuration or share it across machines.

### Schema

```json
{
  "version": 1,
  "session": "agent-swarm",
  "windows": [
    {
      "name": "dev",
      "layout": "tiled",
      "panes": [
        { "command": "claude-code", "cwd": "D:/Home/psmux" },
        { "command": "cargo watch -x test", "cwd": "D:/Home/psmux" },
        { "command": "bash" }
      ]
    },
    {
      "name": "monitor",
      "layout": "main-vertical",
      "panes": [
        { "command": "htop" },
        { "command": "tail -f /var/log/syslog" }
      ]
    }
  ]
}
```

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `version` | No | `1` | Schema version for forward compatibility |
| `session` | No | `"default"` | Session name |
| `windows` | Yes | — | Array of window definitions |
| `windows[].name` | No | `""` | Window name |
| `windows[].layout` | No | `"tiled"` | Named preset: `even-horizontal`, `even-vertical`, `main-horizontal`, `main-vertical`, `tiled` |
| `windows[].panes` | Yes (if no `tree`) | — | Array of pane definitions, minimum 1 |
| `panes[].command` | No | default shell | Shell command string (parsed by shell) |
| `panes[].cwd` | No | caller's cwd | Working directory |
| `panes[].env` | No | `{}` | Extra environment variables |
| `windows[].tree` | No | — | Custom split tree (overrides `layout` + `panes`) |

### Custom Tree Layout

For precise split control, `tree` replaces `layout` + `panes`:

```json
{
  "windows": [{
    "name": "custom",
    "tree": {
      "type": "split", "split": "horizontal", "sizes": [60, 40],
      "children": [
        { "type": "pane", "command": "vim" },
        {
          "type": "split", "split": "vertical", "sizes": [50, 50],
          "children": [
            { "type": "pane", "command": "cargo watch" },
            { "type": "pane", "command": "bash" }
          ]
        }
      ]
    }
  }]
}
```

The `tree` structure uses `"type": "split"` / `"type": "pane"` discrimination (matching the existing `LayoutJson` pattern) and maps directly to `Node::Split` and `Node::Leaf`.

### CLI

```
psmux new-session --layout path/to/layout.json              # New session from file
psmux new-session --layout path/to/layout.json -s myname     # Override session name
psmux source-file layout.json                                # Apply to running session (adds windows as new tabs)
```

### Loading Flow

1. Parse JSON file into `LayoutFile` struct
2. For each window definition:
   a. If `tree` present: recursively build `Node` tree, spawn pane commands at each leaf
   b. If `panes` + `layout` present: spawn N panes as flat leaves, call existing `apply_layout(app, layout_name)` to arrange
3. Set window names, activate first window
4. If applied to existing session via `source-file`: append windows as new tabs

### Serde Types

```rust
// In src/layout.rs (extend existing file)

#[derive(Deserialize)]
struct LayoutFile {
    version: Option<u32>,
    session: Option<String>,
    windows: Vec<WindowDef>,
}

#[derive(Deserialize)]
struct WindowDef {
    name: Option<String>,
    layout: Option<String>,              // Named preset
    panes: Option<Vec<PaneDef>>,         // Flat pane list (used with layout)
    tree: Option<TreeDef>,               // Custom tree (overrides layout+panes)
}

#[derive(Deserialize)]
struct PaneDef {
    command: Option<String>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum TreeDef {
    #[serde(rename = "split")]
    Split {
        split: String,                   // "horizontal" or "vertical"
        sizes: Vec<u16>,                 // Percentages — must sum to ~100, len must match children
        children: Vec<TreeDef>,
    },
    #[serde(rename = "pane")]
    Leaf {
        command: Option<String>,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
    },
}
```

This uses internally-tagged discrimination (matching the existing `LayoutJson` pattern). The custom tree JSON becomes:

```json
{
  "type": "split", "split": "horizontal", "sizes": [60, 40],
  "children": [
    { "type": "pane", "command": "vim" },
    { "type": "split", "split": "vertical", "sizes": [50, 50],
      "children": [
        { "type": "pane", "command": "cargo watch" },
        { "type": "pane", "command": "bash" }
      ]
    }
  ]
}
```

**Sizes validation**: On load, if `sizes.len() != children.len()`, return a parse error. If sizes don't sum to 100, auto-normalize proportionally (e.g., `[60, 60]` → `[50, 50]`). Zero-valued sizes are rejected.
```

### Module

Extend `src/layout.rs`:
- `load_layout_file(path: &str) -> Result<LayoutFile>` — read and parse JSON
- `apply_layout_file(app: &mut AppState, layout_file: LayoutFile) -> Result<()>` — create windows/panes

Integration points:
- `src/main.rs` — parse `--layout` flag in `new-session` command
- `src/cli.rs` — add `--layout` to `new-session` help text
- `src/server/mod.rs` — in the `CtrlReq::SourceFile` handler, check file extension: if `.json`, dispatch to `apply_layout_file()`; otherwise, dispatch to existing `source_file()` for tmux config. This routing happens at the call site, not inside `source_file()` itself.
- Invalid `layout` preset names (e.g., typo `"tled"`) produce a clear error message naming the invalid value and listing valid options.

### Storage Convention

Layout files live wherever the user places them. Suggested convention:
- Personal layouts: `~/.psmux/layouts/`
- Project layouts: checked into repo (e.g., `.psmux/swarm.json`)

---

## Testing Strategy

### Session Resurrection
- Unit test: `save_snapshot()` → `load_snapshot()` roundtrip preserves all fields
- Unit test: atomic write (verify `.tmp` file is cleaned up)
- Integration test: create session with splits → `save_snapshot()` → `kill-server` → `resurrect` → verify same pane count and layout structure

### Quick Select / Hints
- Unit test: regex patterns match expected inputs, reject non-matches
- Unit test: label assignment is deterministic and unique
- Unit test: `handle_hints_key()` with single-char and two-char labels
- Integration test: populate pane with known content → `enter_hints_mode()` → verify correct matches found

### Declarative Layouts
- Unit test: parse minimal JSON → correct `LayoutFile` struct
- Unit test: parse with `tree` field → correct nested `TreeDef`
- Unit test: missing optional fields use defaults
- Integration test: `load_layout_file()` + `apply_layout_file()` → correct pane count and window names

---

## Non-Goals

- **Scrollback serialization** in resurrection (Phase 2+)
- **Periodic save timer** (Phase 2 — MVP uses structural triggers only)
- **Plugin system** for custom hint patterns (use config regex instead)
- **Remote layout loading** from URLs (Zellij has this, not needed for MVP)
- **Layout file format negotiation** (JSON only, no TOML/KDL/YAML)
- **Cross-feature schema unification** (resurrection and layout files converge later)
