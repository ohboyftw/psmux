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
    /// OSC 7501 `app` of the agent running in the pane, kept only when it is
    /// a key of [`resume_command_for`]; empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app: String,
    /// The command resurrect will run for `app`, written for readers of the
    /// file.  Restore recomputes it from `app` and never runs this string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

/// The command that resumes an agent, keyed by its OSC 7501 `app` name.
///
/// Both agents resume the most recent conversation of the current directory,
/// so the pane is restored in the agent's cwd.  `app` comes from the pane
/// and only selects a row here; a pane can never supply the command.
pub fn resume_command_for(app: &str) -> Option<&'static str> {
    match app {
        "claude-code" => Some("claude --continue"),
        "pi" => Some("pi --continue"),
        _ => None,
    }
}

/// Whether a saved pane directory still exists, as resurrect checks it.
pub type DirExists<'a> = &'a dyn Fn(&str) -> bool;

/// The command resurrect runs for `pc`: the agent's resume command when the
/// pane ran a known agent and its directory still exists, else the pane's
/// original command.  `--continue` is keyed on the directory, so anywhere
/// else it would resume another conversation, or none.
pub fn restore_command(pc: &PaneCommand, cwd_exists: bool) -> Option<String> {
    match resume_command_for(&pc.app).filter(|_| cwd_exists) {
        Some(cmd) => Some(cmd.to_string()),
        None => pc.command.clone(),
    }
}

/// `-c <cwd>` when the directory exists (a missing one would be ignored by
/// the server without a word), then the restore command.
fn pane_args(pc: &PaneCommand, exists: DirExists) -> Vec<String> {
    let cwd_exists = exists(&pc.cwd);
    let mut args = Vec::new();
    if cwd_exists {
        args.push("-c".to_string());
        args.push(pc.cwd.clone());
    }
    args.extend(restore_command(pc, cwd_exists));
    args
}

/// One line per pane whose saved directory no longer exists, saying where it
/// starts instead and, for an agent pane, that it is not resumed.
pub fn missing_dir_warnings(snap: &SessionSnapshot, exists: DirExists) -> Vec<String> {
    let mut warnings = Vec::new();
    for (wi, ws) in snap.windows.iter().enumerate() {
        for (pi, pc) in ws.pane_commands.iter().enumerate() {
            if exists(&pc.cwd) {
                continue;
            }
            let mut line = format!(
                "window {wi} pane {pi}: {} no longer exists; the pane starts in psmux's default directory",
                pc.cwd
            );
            if let Some(resume) = resume_command_for(&pc.app) {
                line.push_str(&format!(" and does not run `{resume}`"));
            }
            warnings.push(line);
        }
    }
    warnings
}

/// argv (without the executable) that recreates the snapshot's first pane.
///
/// The restore command is passed positionally, never after `--`: positional
/// commands are shell-wrapped like `new-window`/`split-window` ones, while
/// `--` spawns its argument as a raw program name, which fails for a command
/// line such as `claude --continue` and for npm shims such as `pi.cmd`.
pub fn first_pane_new_session_args(
    session_name: &str,
    snap: &SessionSnapshot,
    exists: DirExists,
) -> Vec<String> {
    let mut args = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        session_name.into(),
    ];
    if let Some(pc) = snap.windows.first().and_then(|w| w.pane_commands.first()) {
        args.extend(pane_args(pc, exists));
    }
    args
}

/// One psmux invocation (argv without the executable) per step that rebuilds
/// the rest of the snapshot after [`first_pane_new_session_args`]: a
/// `new-window` for each later window, a `split-window` for each later pane,
/// `select-layout tiled` for a window with several panes, and the window name.
///
/// Every step starts with `-t <session>` before the subcommand, so the global
/// parse routes it to the new session.  Without it a step inherits the target
/// resolved when the session did not exist yet: the session the caller's pane
/// is in (`TMUX`), or the most recent live session.  The steps run in order;
/// `new-window` makes its window active, so the splits and the name that
/// follow it land in that window.
pub fn restore_steps(
    session_name: &str,
    snap: &SessionSnapshot,
    exists: DirExists,
) -> Vec<Vec<String>> {
    let step = |verb: &str, args: Vec<String>| -> Vec<String> {
        let mut v = vec!["-t".to_string(), session_name.to_string(), verb.to_string()];
        v.extend(args);
        v
    };
    let mut steps = Vec::new();
    for (wi, ws) in snap.windows.iter().enumerate() {
        for (pi, pc) in ws.pane_commands.iter().enumerate() {
            let verb = match (wi, pi) {
                (0, 0) => continue,
                (_, 0) => "new-window",
                _ => "split-window",
            };
            steps.push(step(verb, pane_args(pc, exists)));
        }
        if ws.pane_commands.len() > 1 {
            steps.push(step("select-layout", vec!["tiled".into()]));
        }
        if !ws.name.is_empty() {
            // The CLI sends the name unquoted; quote it for the server's
            // tokenizer so a name with spaces is not cut at the first one.
            let quoted = format!("\"{}\"", ws.name.replace('\\', "\\\\").replace('"', "\\\""));
            steps.push(step("rename-window", vec![quoted]));
        }
    }
    steps
}

/// Rebuild session `name` from `snap`, the snapshot loaded from `dir`.
/// `run` runs one psmux invocation (argv without the executable) and says
/// whether it succeeded; `settle` is how long the new server gets to start
/// before the restore steps.
///
/// The snapshot is not deleted, and a copy is kept at [`backup_path`]: a
/// resumed agent can exit at once (no conversation for its directory, or it
/// fails to start), its pane and then the server exit with it, and a retry
/// needs the snapshot.  The file itself is not enough, because the new server
/// rewrites it on its own save points (each restore step, an agent appearing
/// or leaving) and a pane that has died records no agent.
pub fn resurrect(
    name: &str,
    snap: &SessionSnapshot,
    dir: &Path,
    settle: Duration,
    run: &mut dyn FnMut(&[String]) -> bool,
) -> io::Result<()> {
    let backup = backup_path(name, dir);
    fs::copy(dir.join(format!("{name}.json")), &backup)?;
    eprintln!(
        "psmux resurrect: if the restored session ends at once, rename {} to {name}.json and run resurrect again",
        backup.display()
    );
    let exists = |p: &str| Path::new(p).is_dir();
    for warning in missing_dir_warnings(snap, &exists) {
        eprintln!("psmux resurrect: {warning}");
    }
    let steps = restore_steps(name, snap, &exists);
    eprintln!("Spawning session with {} setup commands...", steps.len());
    if !run(&first_pane_new_session_args(name, snap, &exists)) {
        return Err(io::Error::other(format!("new-session for '{name}' failed")));
    }
    thread::sleep(settle);
    for step in &steps {
        if !run(step) {
            eprintln!("Warning: `{}` failed", step[2..].join(" "));
        }
    }
    Ok(())
}

/// Whether an agent-state transition changes what resurrect would resume:
/// an agent appearing in or leaving a pane does; flips between states do not.
pub fn changes_resume_source(t: &crate::agent_state::AgentTransition) -> bool {
    t.old.is_none() != t.new.is_none()
}

/// One row of the OS process table: pid, parent pid, lowercased image name.
pub type ProcessRow = (u32, u32, String);

fn is_shell_image(name: &str) -> bool {
    matches!(
        name,
        "pwsh.exe" | "powershell.exe" | "cmd.exe" | "bash.exe" | "sh.exe"
    )
}

fn is_console_host(name: &str) -> bool {
    matches!(name, "conhost.exe" | "openconsole.exe")
}

/// The agent process of a pane whose child is `root`: `root` itself when the
/// pane was spawned straight into a program, else the shallowest descendant
/// that is neither a shell nor a console host.  Deeper processes (MCP
/// servers, tool commands) are the agent's own children and may run in
/// another directory.  `None` when no such process is alive.
pub fn agent_pid(root: u32, table: &[ProcessRow]) -> Option<u32> {
    let root_name = &table.iter().find(|r| r.0 == root)?.2;
    if !is_shell_image(root_name) {
        return Some(root);
    }
    let mut seen = std::collections::HashSet::from([root]);
    let mut level = vec![root];
    while !level.is_empty() {
        let children: Vec<&ProcessRow> = table
            .iter()
            .filter(|r| level.contains(&r.1) && seen.insert(r.0))
            .collect();
        if let Some(r) = children
            .iter()
            .find(|r| !is_shell_image(&r.2) && !is_console_host(&r.2))
        {
            return Some(r.0);
        }
        level = children
            .iter()
            .filter(|r| is_shell_image(&r.2))
            .map(|r| r.0)
            .collect();
    }
    None
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
    PathBuf::from(crate::paths::psmux_dir()).join("resurrect")
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

/// Keep only the newest `keep` snapshots in `dir`, delete older ones.
///
/// `save_snapshot` runs on every structural change (new-window, split-window,
/// select-layout, ...) for every session, independently of `resurrect-on-exit`,
/// so without a bound the directory grows one file per session name forever —
/// 150 had accumulated by 2026-08-03, all listed as `(resurrectable)`.
/// Mirrors `crash::prune_crashes`, which bounds crash reports the same way.
pub fn prune_snapshots_in(dir: &Path, keep: usize) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = read_dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                return None;
            }
            Some((path, e.metadata().ok()?.modified().ok()?))
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(&path);
        // The copy resurrect kept (`<name>.json.bak`) is not a `.json` file,
        // so nothing else would ever remove it.
        let mut backup = path.into_os_string();
        backup.push(".bak");
        let _ = fs::remove_file(backup);
    }
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

/// Where resurrect keeps a copy of the snapshot it restores from.  Neither
/// [`list_resurrectable`] nor [`prune_snapshots_in`] sees it.
pub fn backup_path(session_name: &str, dir: &Path) -> PathBuf {
    dir.join(format!("{session_name}.json.bak"))
}

/// Delete the snapshot file for `session_name` from `dir`, and the copy
/// resurrect kept of it.
pub fn delete_snapshot(session_name: &str, dir: &Path) -> io::Result<()> {
    let _ = fs::remove_file(backup_path(session_name, dir));
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

/// The process table, read once per snapshot and only if a pane needs it.
type LazyProcessTable = std::cell::OnceCell<Vec<ProcessRow>>;

/// Collect spawn metadata from leaf panes in DFS order.
fn collect_pane_commands(node: &Node, table: &LazyProcessTable, out: &mut Vec<PaneCommand>) {
    match node {
        Node::Leaf(pane) => out.push(pane_command(pane, table)),
        Node::Split { children, .. } => {
            for child in children {
                collect_pane_commands(child, table, out);
            }
        }
    }
}

/// Spawn metadata of one pane.  A live pane whose agent process is still
/// running records the agent's `app` and the agent's own cwd, because
/// `--continue` is keyed on the directory the agent was started in.  A dead
/// pane, or one whose agent exited leaving a done/error record, records none.
fn pane_command(pane: &crate::types::Pane, table: &LazyProcessTable) -> PaneCommand {
    let app = &pane.agent_state.details().app;
    let agent = resume_command_for(app)
        .filter(|_| !pane.dead)
        .and(pane.child_pid)
        .and_then(|pid| {
            agent_pid(
                pid,
                table.get_or_init(crate::platform::process_info::process_table),
            )
        });
    let cwd = agent
        .and_then(crate::platform::process_info::get_process_cwd)
        .or_else(|| {
            pane.spawn_cwd
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let app = if agent.is_some() {
        app.clone()
    } else {
        String::new()
    };
    PaneCommand {
        command: pane.spawn_command.clone(),
        cwd,
        env: pane.spawn_env.clone(),
        resume_command: resume_command_for(&app).map(str::to_string),
        app,
    }
}

/// Build a complete snapshot from the current server state.
pub fn build_snapshot(app: &AppState) -> SessionSnapshot {
    let table = LazyProcessTable::new();
    let mut windows = Vec::new();
    for win in &app.windows {
        let mut pane_commands = Vec::new();
        collect_pane_commands(&win.root, &table, &mut pane_commands);
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
                crate::pane::create_window(pty_system, app, cmd.as_deref(), Some(&pc.cwd), None)?;
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
/// The snapshot is built on the calling thread: cloning small metadata, plus
/// one process-table snapshot and one cwd read per agent pane when any pane
/// reports a known agent.  It is then sent to a background writer thread.  Rapid
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
                        app: "claude-code".into(),
                        resume_command: Some("claude --continue".into()),
                    },
                    PaneCommand {
                        command: None,
                        cwd: "/home/user".into(),
                        env: vec![],
                        app: String::new(),
                        resume_command: None,
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

#[cfg(test)]
#[path = "../tests-rs/test_resurrection_resume.rs"]
mod test_resurrection_resume;

#[cfg(test)]
mod test_snapshot_pruning {
    use super::*;

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("clean pre-existing temp dir");
        }
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Writes `names` in order, oldest first, with distinct modified times.
    fn write_snapshots(dir: &Path, names: &[&str]) {
        for name in names {
            fs::write(dir.join(format!("{}.json", name)), "{}").expect("write snapshot");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn keeps_the_newest_snapshots_up_to_the_limit() {
        let dir = fresh_dir("psmux_prune_keeps_newest");
        write_snapshots(&dir, &["oldest", "middle", "newest"]);

        prune_snapshots_in(&dir, 2);

        let mut kept = list_resurrectable(&dir);
        kept.sort();
        assert_eq!(kept, vec!["middle".to_string(), "newest".to_string()]);
    }

    #[test]
    fn keeps_everything_when_under_the_limit() {
        let dir = fresh_dir("psmux_prune_under_limit");
        write_snapshots(&dir, &["a", "b"]);

        prune_snapshots_in(&dir, 50);

        assert_eq!(list_resurrectable(&dir).len(), 2);
    }

    #[test]
    fn ignores_files_that_are_not_snapshots() {
        let dir = fresh_dir("psmux_prune_ignores_non_json");
        fs::write(dir.join("notes.txt"), "keep me").expect("write");
        write_snapshots(&dir, &["only"]);

        prune_snapshots_in(&dir, 0);

        assert!(dir.join("notes.txt").exists(), "non-.json must be ignored");
        assert!(list_resurrectable(&dir).is_empty(), "snapshots pruned to 0");
    }
}
