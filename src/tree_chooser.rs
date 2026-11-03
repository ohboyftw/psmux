//! Rows of the client-side choose-tree overlay (prefix + w), with the
//! OSC 7501 agent-state glyph rolled up worst-of from panes to windows to
//! session headers, and the 1 s re-poll that keeps the glyphs current while
//! the overlay is open.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ratatui::prelude::*;

use crate::agent_state::{parse_state, state_glyph, worst_label};
use crate::util::WinTree;

/// How often the open overlay asks every session for a fresh `list-tree`.
pub const REPOLL_INTERVAL: Duration = Duration::from_secs(1);

/// Polls in a row a listed session may miss before its rows are dropped.
pub const MAX_MISSES: u32 = 3;

/// One session's windows, as its server's `list-tree` returned them.
pub type SessionTree = (String, Vec<WinTree>);

/// One poll of every session.
#[derive(Clone, Debug, Default)]
pub struct SessionPoll {
    /// Every non-warm session with a port file, answering or not.
    pub listed: Vec<String>,
    /// The sessions that answered, current first, the rest by name.
    pub answered: Vec<SessionTree>,
}

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

/// The glyph span for a state, or `None` when nothing is reported.
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

/// The two-cell slot drawn before a row's label: the glyph and a space, or
/// two spaces when nothing is reported.  It leads the row so a long label
/// that runs past the overlay's edge never hides the glyph.  `base` is the
/// row's style and the glyph's colour is laid over it, so a selected row's
/// background runs unbroken through the slot.
pub fn glyph_slot(agent: &str, base: Style) -> Span<'static> {
    match glyph_span(agent) {
        Some(glyph) => Span::styled(format!("{} ", glyph.content), base.patch(glyph.style)),
        None => Span::styled("  ", base),
    }
}

/// The glyph slot, then `label`.
pub fn with_agent_glyph(label: Span<'static>, agent: &str) -> Line<'static> {
    Line::from(vec![glyph_slot(agent, label.style), label])
}

/// One overlay row drawn in `style`: the glyph slot, then the label.
pub fn row_line(row: &TreeRow, style: Style) -> Line<'static> {
    with_agent_glyph(Span::styled(row.label.clone(), style), row.agent)
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

/// A window row, then its pane rows when `with_panes`.
fn window_rows(rows: &mut Vec<TreeRow>, session: &str, wi: usize, w: &WinTree, with_panes: bool) {
    let label = format!("  {wi}: {} ({} panes)", w.name, w.panes.len());
    let mut win = row(true, w.id, 0, label, session);
    win.agent = window_agent(w);
    rows.push(win);
    if !with_panes {
        return;
    }
    for p in &w.panes {
        let mut pane = row(false, w.id, p.id, format!("    {}", p.title), session);
        pane.agent = worst_label([p.agent_state.as_str()]);
        rows.push(pane);
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
        let label = format!("{name}: {} windows{attached}", wins.len());
        let mut header = row(true, usize::MAX, 0, label, name);
        header.agent = worst_label(wins.iter().map(window_agent));
        rows.push(header);
        for (wi, w) in wins.iter().enumerate() {
            window_rows(&mut rows, name, wi, w, is_current);
        }
    }
    rows
}

/// Rows from the current session's last pushed tree, used when no session
/// answered `list-tree`.  They carry no glyph: that tree is pushed only when
/// the layout changes, so its agent states can be arbitrarily old.
pub fn fallback_rows(last_tree: &[WinTree], current: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for w in last_tree {
        rows.push(row(true, w.id, 0, w.name.clone(), current));
        for p in &w.panes {
            rows.push(row(false, w.id, p.id, p.title.clone(), current));
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

/// The rows and selection after a re-poll, or `None` to keep the rows
/// already shown because the poll produced none.
pub fn refreshed_rows(
    old: &[TreeRow],
    selected: usize,
    sessions: &[SessionTree],
    current: &str,
) -> Option<(Vec<TreeRow>, usize)> {
    let rows = build_rows(sessions, current);
    if rows.is_empty() {
        return None;
    }
    let selected = reselect(old, selected, &rows);
    Some((rows, selected))
}

fn sort_sessions(sessions: &mut [SessionTree], current: &str) {
    sessions.sort_by(|a, b| (a.0 != current, &a.0).cmp(&(b.0 != current, &b.0)));
}

/// Merges a poll into the sessions shown.  A listed session that did not
/// answer keeps its shown windows until it has missed [`MAX_MISSES`] polls
/// in a row; a session whose port file is gone is dropped at once.
pub fn merge_poll(
    shown: &[SessionTree],
    misses: &mut HashMap<String, u32>,
    poll: SessionPoll,
    current: &str,
) -> Vec<SessionTree> {
    let SessionPoll {
        listed,
        mut answered,
    } = poll;
    misses.retain(|name, _| listed.contains(name));
    for (name, _) in &answered {
        misses.remove(name);
    }
    let missing: Vec<String> = listed
        .into_iter()
        .filter(|n| !answered.iter().any(|(a, _)| a == n))
        .collect();
    for name in missing {
        let missed = misses.entry(name.clone()).or_insert(0);
        *missed += 1;
        let kept = shown.iter().find(|(s, _)| *s == name);
        if let Some(tree) = kept.filter(|_| *missed < MAX_MISSES) {
            answered.push(tree.clone());
        }
    }
    sort_sessions(&mut answered, current);
    answered
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

fn query_session(base: String, port: u16) -> Option<SessionTree> {
    let key = crate::session::read_session_key(&base).unwrap_or_default();
    query_session_tree(port, &key).map(|wins| (base, wins))
}

fn session_port(path: &std::path::Path) -> Option<(String, u16)> {
    let (base, ext) = path.file_name()?.to_str()?.rsplit_once('.')?;
    if ext != "port" || crate::session::is_warm_session(base) {
        return None;
    }
    let port = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some((base.to_string(), port))
}

/// Asks every non-warm session's server for its `list-tree`, all at once,
/// so a poll takes as long as the slowest session rather than the sum of
/// them.  A session that does not answer, or answers JSON that does not
/// parse, is listed but not answered.
pub fn poll_sessions(current: &str) -> SessionPoll {
    let Ok(entries) = std::fs::read_dir(crate::paths::psmux_dir()) else {
        return SessionPoll::default();
    };
    let ports: Vec<(String, u16)> = entries
        .flatten()
        .filter_map(|e| session_port(&e.path()))
        .collect();
    let listed = ports.iter().map(|(base, _)| base.clone()).collect();
    let mut answered: Vec<SessionTree> = std::thread::scope(|scope| {
        let asks: Vec<_> = ports
            .into_iter()
            .map(|(base, port)| scope.spawn(move || query_session(base, port)))
            .collect();
        asks.into_iter()
            .filter_map(|ask| ask.join().ok().flatten())
            .collect()
    });
    sort_sessions(&mut answered, current);
    SessionPoll { listed, answered }
}

/// The sessions that answered [`poll_sessions`], current first.
pub fn collect_sessions(current: &str) -> Vec<SessionTree> {
    poll_sessions(current).answered
}

/// Re-polls `list-tree` off the render thread while the overlay is open, so
/// slow or stale sessions never stall drawing.  Polls start
/// [`REPOLL_INTERVAL`] apart, timed from the start of the previous one.
pub struct TreePoller {
    pending: Option<Receiver<SessionPoll>>,
    last: Instant,
    shown: Vec<SessionTree>,
    misses: HashMap<String, u32>,
}

impl Default for TreePoller {
    fn default() -> Self {
        Self {
            pending: None,
            last: Instant::now(),
            shown: Vec::new(),
            misses: HashMap::new(),
        }
    }
}

impl TreePoller {
    /// Call when the overlay opens, with the sessions it shows: drops any
    /// poll in flight and waits a full interval before the next.
    pub fn restart(&mut self, shown: Vec<SessionTree>) {
        self.pending = None;
        self.last = Instant::now();
        self.shown = shown;
        self.misses.clear();
    }

    /// Call every loop iteration.  When a poll has finished, returns the
    /// shown sessions with it merged in (see [`merge_poll`]); starts the
    /// next poll once the interval has passed.
    pub fn poll(&mut self, open: bool, current: &str) -> Option<Vec<SessionTree>> {
        if !open {
            self.pending = None;
            return None;
        }
        if let Some(rx) = &self.pending {
            return match rx.try_recv() {
                Ok(poll) => {
                    self.pending = None;
                    self.shown = merge_poll(&self.shown, &mut self.misses, poll, current);
                    Some(self.shown.clone())
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    None
                }
            };
        }
        if self.last.elapsed() >= REPOLL_INTERVAL {
            self.start(current);
        }
        None
    }

    fn start(&mut self, current: &str) {
        let (tx, rx) = mpsc::channel();
        let current = current.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(poll_sessions(&current));
        });
        self.pending = Some(rx);
        self.last = Instant::now();
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_tree_chooser.rs"]
mod tests;
