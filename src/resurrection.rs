//! Session resurrection snapshots — save and restore psmux sessions across restarts.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A full snapshot of a psmux session, suitable for JSON serialisation.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct SessionSnapshot {
    pub version: u32,
    pub session_name: String,
    pub timestamp: u64,
    pub windows: Vec<WindowSnapshot>,
    pub active_window_idx: usize,
}

/// Snapshot of a single window inside a session.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct WindowSnapshot {
    pub name: String,
    pub id: usize,
    pub active_path: Vec<usize>,
    pub layout_tree: LayoutTreeNode,
    pub pane_commands: Vec<PaneCommand>,
}

/// Recursive layout tree — either a split with children or a terminal leaf.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum LayoutTreeNode {
    #[serde(rename = "split")]
    Split {
        kind: String,
        sizes: Vec<u16>,
        children: Vec<LayoutTreeNode>,
    },
    #[serde(rename = "leaf")]
    Leaf { id: usize },
}

/// Per-pane metadata needed to resurrect a pane.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PaneCommand {
    pub command: Option<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Return the directory used for resurrection snapshots.
///
/// If `custom` is provided it is used verbatim; otherwise we fall back to
/// `~/.psmux/resurrect/` using `USERPROFILE` (Windows) or `HOME`.
pub fn resurrect_dir(custom: Option<&str>) -> PathBuf {
    if let Some(p) = custom {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    PathBuf::from(home).join(".psmux").join("resurrect")
}

/// Atomically save `snap` as `<session_name>.json` inside `dir`.
///
/// The write goes to a `.json.tmp` file first, then is renamed into place.
/// On `PermissionDenied` the rename is retried once after a short sleep
/// (Windows antivirus scanners can briefly lock newly-created files).
pub fn save_snapshot_to(snap: &SessionSnapshot, dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;

    let final_path = dir.join(format!("{}.json", snap.session_name));
    let tmp_path = dir.join(format!("{}.json.tmp", snap.session_name));

    let json = serde_json::to_string_pretty(snap)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    fs::write(&tmp_path, json)?;

    match fs::rename(&tmp_path, &final_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            thread::sleep(Duration::from_millis(50));
            fs::rename(&tmp_path, &final_path)
        }
        Err(e) => Err(e),
    }
}

/// Load a previously saved snapshot for `session_name` from `dir`.
pub fn load_snapshot_from(session_name: &str, dir: &Path) -> io::Result<SessionSnapshot> {
    let path = dir.join(format!("{session_name}.json"));
    let data = fs::read_to_string(&path)?;
    serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// List session names that have a `.json` snapshot in `dir`.
///
/// Returns an empty vec if the directory does not exist.
pub fn list_resurrectable(dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(String::from)
        })
        .collect();

    names.sort();
    names
}

/// Delete the snapshot file for `session_name` from `dir`.
pub fn delete_snapshot(session_name: &str, dir: &Path) -> io::Result<()> {
    let path = dir.join(format!("{session_name}.json"));
    fs::remove_file(path)
}

// ---------------------------------------------------------------------------
// Build snapshot from live AppState
// ---------------------------------------------------------------------------

use crate::types::{AppState, LayoutKind, Node};

/// Serialize a live `Node` tree into a `LayoutTreeNode` (topology only, no content).
fn serialize_tree(node: &Node) -> LayoutTreeNode {
    match node {
        Node::Leaf(pane) => LayoutTreeNode::Leaf { id: pane.id },
        Node::Split {
            kind,
            sizes,
            children,
        } => LayoutTreeNode::Split {
            kind: match kind {
                LayoutKind::Horizontal => "horizontal".to_string(),
                LayoutKind::Vertical => "vertical".to_string(),
            },
            sizes: sizes.clone(),
            children: children.iter().map(serialize_tree).collect(),
        },
    }
}

/// Collect spawn metadata from leaf panes in DFS order.
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

/// Build a complete snapshot from the current server state.
pub fn build_snapshot(app: &AppState) -> SessionSnapshot {
    let mut windows = Vec::new();
    for win in &app.windows {
        let mut pane_commands = Vec::new();
        collect_pane_commands(&win.root, &mut pane_commands);
        let layout_tree = serialize_tree(&win.root);
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

/// Rebuild a session from a saved snapshot.
///
/// For each window, panes are created with a "Press ENTER to run" banner
/// wrapping the original command (safety measure — prevents auto-running
/// destructive commands like `rm -rf`).  Default-shell panes are spawned
/// directly without the banner.
pub fn apply_snapshot(
    app: &mut AppState,
    pty_system: &dyn portable_pty::PtySystem,
    snap: SessionSnapshot,
) -> io::Result<()> {
    app.session_name = snap.session_name;

    for win_snap in &snap.windows {
        for (pi, pc) in win_snap.pane_commands.iter().enumerate() {
            // Wrap non-default commands in a safety banner
            let cmd: Option<String> = pc.command.as_ref().map(|c| {
                // bash -c with a "press ENTER" prompt before running the real command
                format!(
                    "bash -c 'echo \"Press ENTER to run: {}\" && read && {}'",
                    c.replace('\'', "'\\''"),
                    c.replace('\'', "'\\''")
                )
            });

            if pi == 0 {
                // First pane → new window
                crate::pane::create_window(
                    pty_system,
                    app,
                    cmd.as_deref(),
                    Some(&pc.cwd),
                    None,
                )?;
            } else {
                // Additional panes → split
                crate::pane::split_active_with_command(
                    app,
                    LayoutKind::Vertical,
                    cmd.as_deref(),
                    Some(pty_system),
                    Some(&pc.cwd),
                    None,
                )?;
            }
        }

        // Rearrange panes using tiled layout (MVP — full tree rebuild later)
        if win_snap.pane_commands.len() > 1 {
            crate::layout::apply_layout(app, "tiled");
        }

        // Restore window name
        if let Some(win) = app.windows.last_mut() {
            win.name = win_snap.name.clone();
            win.manual_rename = true;
        }
    }

    // Restore active window index
    if snap.active_window_idx < app.windows.len() {
        app.active_idx = snap.active_window_idx;
    }

    Ok(())
}

/// Best-effort save: build snapshot from AppState and write to disk.
///
/// The snapshot is serialized on the calling thread (cheap — just cloning
/// small metadata), then sent to a background writer thread.  Rapid
/// structural changes (split → resize → split) coalesce: the writer uses a
/// 100ms debounce so at most ~10 writes/sec hit disk.
pub fn save_snapshot(app: &AppState) {
    let snap = build_snapshot(app);
    let dir = resurrect_dir(app.resurrect_dir.as_deref());
    spawn_or_send(snap, dir);
}

/// Lazily initialized channel to the background snapshot writer thread.
static SNAPSHOT_TX: std::sync::OnceLock<std::sync::mpsc::Sender<(SessionSnapshot, PathBuf)>> =
    std::sync::OnceLock::new();

fn spawn_or_send(snap: SessionSnapshot, dir: PathBuf) {
    let tx = SNAPSHOT_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(SessionSnapshot, PathBuf)>();
        thread::Builder::new()
            .name("snapshot-writer".into())
            .spawn(move || {
                // Debounce: drain all pending snapshots, write only the latest.
                while let Ok((mut snap, mut dir)) = rx.recv() {
                    // Drain any queued snapshots (keep the newest)
                    while let Ok((s, d)) = rx.try_recv() {
                        snap = s;
                        dir = d;
                    }
                    // Small delay to coalesce rapid changes
                    thread::sleep(Duration::from_millis(100));
                    // Drain again after the delay
                    while let Ok((s, d)) = rx.try_recv() {
                        snap = s;
                        dir = d;
                    }
                    let _ = save_snapshot_to(&snap, &dir);
                }
            })
            .expect("failed to spawn snapshot writer thread");
        tx
    });
    // Best-effort: if the channel is disconnected, silently drop.
    let _ = tx.send((snap, dir));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn roundtrip() {
        // Build a snapshot with a split layout containing two leaves.
        let snap = SessionSnapshot {
            version: 1,
            session_name: "test-session".into(),
            timestamp: 1700000000,
            windows: vec![WindowSnapshot {
                name: "editor".into(),
                id: 0,
                active_path: vec![0, 0],
                layout_tree: LayoutTreeNode::Split {
                    kind: "horizontal".into(),
                    sizes: vec![50, 50],
                    children: vec![
                        LayoutTreeNode::Leaf { id: 0 },
                        LayoutTreeNode::Leaf { id: 1 },
                    ],
                },
                pane_commands: vec![
                    PaneCommand {
                        command: Some("nvim".into()),
                        cwd: "/home/user/project".into(),
                        env: vec![("TERM".into(), "xterm-256color".into())],
                    },
                    PaneCommand {
                        command: None,
                        cwd: "/home/user".into(),
                        env: vec![],
                    },
                ],
            }],
            active_window_idx: 0,
        };

        // Use a unique temp directory to avoid collisions with other tests.
        let tmp = std::env::temp_dir().join("psmux_resurrection_roundtrip_test");
        if tmp.exists() {
            fs::remove_dir_all(&tmp).expect("clean pre-existing temp dir");
        }

        // Save ----------------------------------------------------------------
        save_snapshot_to(&snap, &tmp).expect("save_snapshot_to should succeed");

        // List ----------------------------------------------------------------
        let names = list_resurrectable(&tmp);
        assert_eq!(names, vec!["test-session".to_string()]);

        // Load ----------------------------------------------------------------
        let loaded =
            load_snapshot_from("test-session", &tmp).expect("load_snapshot_from should succeed");
        assert_eq!(snap, loaded);

        // Delete + cleanup ----------------------------------------------------
        delete_snapshot("test-session", &tmp).expect("delete_snapshot should succeed");
        assert!(list_resurrectable(&tmp).is_empty());

        fs::remove_dir_all(&tmp).ok();
    }
}
