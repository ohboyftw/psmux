//! Rows of the client-side choose-tree overlay (prefix + w), with the
//! OSC 7501 agent-state glyph rolled up worst-of from panes to windows to
//! session headers, and the 1 s re-poll that keeps the glyphs current while
//! the overlay is open.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ratatui::prelude::*;

use crate::agent_state::{parse_state, state_glyph, worst_label};
use crate::util::WinTree;

/// How often the open overlay asks every session for a fresh `list-tree`.
pub const REPOLL_INTERVAL: Duration = Duration::from_secs(1);

/// One session's windows, as its server's `list-tree` returned them.
pub type SessionTree = (String, Vec<WinTree>);

/// One line of the overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// True for session headers and window rows.
    pub is_win: bool,
    /// Window id; `usize::MAX` marks a session header.
    pub id: usize,
    /// Pane id on pane rows, else 0.
    pub sub_id: usize,
    /// Text shown, without the glyph.
    pub label: String,
    /// Session the row belongs to.
    pub session: String,
    /// Worst-of agent state label; empty when nothing is reported.
    pub agent: &'static str,
}

impl TreeRow {
    /// True when the row is a session header.
    pub fn is_header(&self) -> bool {
        self.is_win && self.id == usize::MAX
    }

    fn same_target(&self, other: &TreeRow) -> bool {
        self.session == other.session
            && self.is_win == other.is_win
            && self.id == other.id
            && self.sub_id == other.sub_id
    }
}

/// The glyph span drawn after a row's label, or `None` when nothing is
/// reported.
pub fn glyph_span(agent: &str) -> Option<Span<'static>> {
    let state = parse_state(agent)?;
    let color = match state {
        vt100::ProgramState::Blocked => Color::LightRed,
        vt100::ProgramState::Error => Color::Red,
        vt100::ProgramState::Working => Color::Yellow,
        vt100::ProgramState::Done => Color::Green,
        vt100::ProgramState::Idle => Color::DarkGray,
    };
    Some(Span::styled(
        state_glyph(Some(state)),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// `label` followed by the agent glyph, when `agent` names a state.
pub fn with_agent_glyph(label: Span<'static>, agent: &str) -> Line<'static> {
    match glyph_span(agent) {
        Some(glyph) => Line::from(vec![label, Span::raw(" "), glyph]),
        None => Line::from(label),
    }
}

fn window_agent(w: &WinTree) -> &'static str {
    worst_label(w.panes.iter().map(|p| p.agent_state.as_str()))
}

fn row(is_win: bool, id: usize, sub_id: usize, label: String, session: &str) -> TreeRow {
    TreeRow {
        is_win,
        id,
        sub_id,
        label,
        session: session.to_string(),
        agent: "",
    }
}

/// Builds the overlay rows: each session header, then its windows, and the
/// panes of the current session's windows.  Other sessions stay collapsed
/// to windows, but their headers still roll up every pane.
pub fn build_rows(sessions: &[SessionTree], current: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for (name, wins) in sessions {
        let is_current = name == current;
        let attached = if is_current { " (attached)" } else { "" };
        let mut header = row(
            true,
            usize::MAX,
            0,
            format!("{name}: {} windows{attached}", wins.len()),
            name,
        );
        header.agent = worst_label(wins.iter().map(window_agent));
        rows.push(header);
        for (wi, w) in wins.iter().enumerate() {
            let mut win = row(
                true,
                w.id,
                0,
                format!("  {wi}: {} ({} panes)", w.name, w.panes.len()),
                name,
            );
            win.agent = window_agent(w);
            rows.push(win);
            if !is_current {
                continue;
            }
            for p in &w.panes {
                let mut pane = row(false, w.id, p.id, format!("    {}", p.title), name);
                pane.agent = worst_label([p.agent_state.as_str()]);
                rows.push(pane);
            }
        }
    }
    rows
}

/// Rows from the current session's last pushed tree, used when no session
/// answered `list-tree`.
pub fn fallback_rows(last_tree: &[WinTree], current: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for w in last_tree {
        let mut win = row(true, w.id, 0, w.name.clone(), current);
        win.agent = window_agent(w);
        rows.push(win);
        for p in &w.panes {
            let mut pane = row(false, w.id, p.id, p.title.clone(), current);
            pane.agent = worst_label([p.agent_state.as_str()]);
            rows.push(pane);
        }
    }
    rows
}

/// Rows from `sessions`, or from `last_tree` when no session answered.
pub fn rows_or_fallback(
    sessions: &[SessionTree],
    current: &str,
    last_tree: &[WinTree],
) -> Vec<TreeRow> {
    let rows = build_rows(sessions, current);
    if rows.is_empty() {
        fallback_rows(last_tree, current)
    } else {
        rows
    }
}

/// The index in `new` of the row `old[selected]` named, so a re-poll keeps
/// the cursor on the same target; clamped when that row is gone.
pub fn reselect(old: &[TreeRow], selected: usize, new: &[TreeRow]) -> usize {
    old.get(selected)
        .and_then(|o| new.iter().position(|n| n.same_target(o)))
        .unwrap_or_else(|| selected.min(new.len().saturating_sub(1)))
}

fn query_session_tree(port: u16, key: &str) -> Option<Vec<WinTree>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut ss = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)).ok()?;
    let _ = ss.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = writeln!(ss, "AUTH {key}");
    let _ = ss.write_all(b"list-tree\n");
    let _ = ss.flush();
    let mut br = BufReader::new(ss);
    let mut auth = String::new();
    let _ = br.read_line(&mut auth);
    let mut line = String::new();
    br.read_line(&mut line).ok()?;
    serde_json::from_str(line.trim()).ok()
}

fn session_port(path: &std::path::Path) -> Option<(String, u16)> {
    let (base, ext) = path.file_name()?.to_str()?.rsplit_once('.')?;
    if ext != "port" || crate::session::is_warm_session(base) {
        return None;
    }
    let port = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some((base.to_string(), port))
}

/// Asks every non-warm session's server for its `list-tree`; the current
/// session sorts first, the rest by name.  Sessions that do not answer, or
/// answer JSON that does not parse, are left out.
pub fn collect_sessions(current: &str) -> Vec<SessionTree> {
    let Ok(entries) = std::fs::read_dir(crate::paths::psmux_dir()) else {
        return Vec::new();
    };
    let mut sessions: Vec<SessionTree> = entries
        .flatten()
        .filter_map(|e| session_port(&e.path()))
        .filter_map(|(base, port)| {
            let key = crate::session::read_session_key(&base).unwrap_or_default();
            query_session_tree(port, &key).map(|wins| (base, wins))
        })
        .collect();
    sessions.sort_by(|a, b| (a.0 != current, &a.0).cmp(&(b.0 != current, &b.0)));
    sessions
}

/// Re-polls `list-tree` off the render thread while the overlay is open, so
/// slow or stale sessions never stall drawing.
pub struct TreePoller {
    pending: Option<Receiver<Vec<SessionTree>>>,
    last: Instant,
}

impl Default for TreePoller {
    fn default() -> Self {
        Self {
            pending: None,
            last: Instant::now(),
        }
    }
}

impl TreePoller {
    /// Call when the overlay opens: drops any poll in flight and waits a
    /// full interval before the next.
    pub fn restart(&mut self) {
        self.pending = None;
        self.last = Instant::now();
    }

    /// Call every loop iteration.  Returns fresh sessions when a poll
    /// finished; starts the next one when the interval has passed.
    pub fn poll(&mut self, open: bool, current: &str) -> Option<Vec<SessionTree>> {
        if !open {
            self.pending = None;
            return None;
        }
        if let Some(rx) = &self.pending {
            return match rx.try_recv() {
                Ok(sessions) => {
                    self.restart();
                    Some(sessions)
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.restart();
                    None
                }
            };
        }
        if self.last.elapsed() >= REPOLL_INTERVAL {
            let (tx, rx) = mpsc::channel();
            let current = current.to_string();
            std::thread::spawn(move || {
                let _ = tx.send(collect_sessions(&current));
            });
            self.pending = Some(rx);
        }
        None
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_tree_chooser.rs"]
mod tests;
