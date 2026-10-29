//! Cockpit view of the session states sessiond publishes (scale plan I17).
//!
//! sessiond sends `sessiond/session/state`, `sessiond/session/removed`,
//! `sessiond/sidecar/started` and `sessiond/sidecar/alive` messages, specified
//! in the sessiond-sidecar repo's `docs/state-topic.md`. This module keeps the
//! latest state of every session and exposes it as `#{sessiond_*}` format
//! variables, so a status line can show which sessions on the box are blocked
//! on a human.
//!
//! Off unless the user option `@sessiond-feed` names a command. The command
//! runs the first time a `sessiond_*` variable is expanded, and must print one
//! JSONL envelope per message on stdout, replaying current state when it
//! starts, e.g. `ssh sessiond-box tail -n +1 -F ~/.sessiond/state.jsonl`.
//! The mycel client is not used: it is an optional path dependency that CI
//! cannot build.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// The user option that turns the feed on.
pub const FEED_OPTION: &str = "@sessiond-feed";

const STATE_TOPIC: &str = "sessiond/session/state";
const REMOVED_TOPIC: &str = "sessiond/session/removed";
const STARTED_TOPIC: &str = "sessiond/sidecar/started";
const ALIVE_TOPIC: &str = "sessiond/sidecar/alive";

/// The v1 state set with its summary letter, most urgent first.
const STATES: [(&str, &str); 5] = [
    ("blocked", "B"),
    ("error", "E"),
    ("working", "W"),
    ("done", "D"),
    ("idle", "I"),
];

/// The sidecar heartbeats every 30 s; three missed beats means it is gone.
const STALE_AFTER: Duration = Duration::from_secs(90);
const MIN_BACKOFF: Duration = Duration::from_secs(5);
/// Every restart spawns a process, and some machines leak every process that
/// exits, so a feed that keeps dying is retried at most this often.
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Title length in `#{sessiond_attention}`, which has to fit a status line.
const ATTENTION_TITLE_CHARS: usize = 32;
const LIST_TITLE_CHARS: usize = 60;

/// The latest known state of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub state: String,
    pub since: String,
    pub reason: String,
    pub title: Option<String>,
    pub project: Option<String>,
}

impl Entry {
    /// `project: title`, falling back to the session id for a missing title.
    fn label(&self, session: &str, max_title: usize) -> String {
        let title = truncate(self.title.as_deref().unwrap_or(session), max_title);
        match &self.project {
            Some(project) => format!("{project}: {title}"),
            None => title,
        }
    }
}

/// What one feed line did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Not a v1 sessiond message: malformed JSON, another topic, or a state
    /// outside the v1 set.
    Ignored,
    State,
    Removed,
    /// A sidecar (re)started; the board was cleared for its snapshot.
    Started,
    Alive,
}

/// Latest state per session, keyed by sessiond's opaque session id.
#[derive(Debug, Default)]
pub struct Board {
    entries: HashMap<String, Entry>,
}

#[derive(Deserialize)]
struct Envelope {
    topic: String,
    payload: serde_json::Value,
}

/// Unknown keys are ignored, as the contract requires of consumers.
#[derive(Deserialize)]
struct StatePayload {
    session: String,
    state: String,
    #[serde(default)]
    since: String,
    #[serde(default)]
    reason: String,
    title: Option<String>,
    project: Option<String>,
}

#[derive(Deserialize)]
struct RemovedPayload {
    session: String,
}

impl Board {
    /// Apply one feed line.
    pub fn apply_line(&mut self, line: &str) -> Applied {
        let Ok(envelope) = serde_json::from_str::<Envelope>(line.trim()) else {
            return Applied::Ignored;
        };
        match envelope.topic.as_str() {
            STATE_TOPIC => self.apply_state(envelope.payload),
            REMOVED_TOPIC => self.apply_removed(envelope.payload),
            STARTED_TOPIC => {
                // Removals missed while the sidecar was down would linger.
                self.entries.clear();
                Applied::Started
            }
            ALIVE_TOPIC => Applied::Alive,
            _ => Applied::Ignored,
        }
    }

    fn apply_state(&mut self, payload: serde_json::Value) -> Applied {
        let Ok(p) = serde_json::from_value::<StatePayload>(payload) else {
            return Applied::Ignored;
        };
        if rank(&p.state).is_none() {
            return Applied::Ignored;
        }
        let entry = Entry {
            state: p.state,
            since: p.since,
            reason: p.reason,
            title: p.title,
            project: p.project,
        };
        self.entries.insert(p.session, entry);
        Applied::State
    }

    fn apply_removed(&mut self, payload: serde_json::Value) -> Applied {
        let Ok(p) = serde_json::from_value::<RemovedPayload>(payload) else {
            return Applied::Ignored;
        };
        self.entries.remove(&p.session);
        Applied::Removed
    }

    /// The latest state of `session`, if it is on the board.
    pub fn get(&self, session: &str) -> Option<&Entry> {
        self.entries.get(session)
    }

    /// Number of sessions on the board.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Number of sessions currently in `state`.
    pub fn count(&self, state: &str) -> usize {
        self.entries.values().filter(|e| e.state == state).count()
    }

    /// Non-zero counts, most urgent first, e.g. `B1 W2 D3`. Empty when no
    /// session is known.
    pub fn summary(&self) -> String {
        STATES
            .iter()
            .filter_map(|(name, letter)| {
                let n = self.count(name);
                (n > 0).then(|| format!("{letter}{n}"))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The sessions that need a human (blocked, then error), as
    /// `project: title` joined by ` | `.
    pub fn attention(&self) -> String {
        self.sorted()
            .into_iter()
            .filter(|(_, e)| matches!(e.state.as_str(), "blocked" | "error"))
            .map(|(id, e)| e.label(id, ATTENTION_TITLE_CHARS))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Every session, one per line: state letter, then `project: title`.
    pub fn sessions(&self) -> String {
        self.sorted()
            .into_iter()
            .map(|(id, e)| format!("{} {}", letter(&e.state), e.label(id, LIST_TITLE_CHARS)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Entries by urgency, then project, then title, so output is stable.
    fn sorted(&self) -> Vec<(&String, &Entry)> {
        let mut rows: Vec<_> = self.entries.iter().collect();
        rows.sort_by(|(a_id, a), (b_id, b)| {
            (rank(&a.state), &a.project, &a.title, a_id).cmp(&(
                rank(&b.state),
                &b.project,
                &b.title,
                b_id,
            ))
        });
        rows
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Position of `state` in the urgency order, or `None` outside the v1 set.
fn rank(state: &str) -> Option<usize> {
    STATES.iter().position(|(name, _)| *name == state)
}

fn letter(state: &str) -> &'static str {
    rank(state).map_or("?", |i| STATES[i].1)
}

/// At most `max` characters, ending in `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `#{sessiond_feed}` for a feed: `down` while its command is not running,
/// `stale` when the sidecar has been silent for too long, else `live`.
pub fn feed_status(running: bool, since_heard: Duration) -> &'static str {
    if !running {
        "down"
    } else if since_heard > STALE_AFTER {
        "stale"
    } else {
        "live"
    }
}

struct Feed {
    command: String,
    board: Mutex<Board>,
    running: AtomicBool,
    /// Last `started` or `alive` message, or when the command last started.
    last_heard: Mutex<Instant>,
}

static FEED: OnceLock<Feed> = OnceLock::new();

fn lock_board(feed: &Feed) -> MutexGuard<'_, Board> {
    feed.board.lock().unwrap_or_else(|e| e.into_inner())
}

fn mark_heard(feed: &Feed) {
    *feed.last_heard.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
}

fn since_heard(feed: &Feed) -> Duration {
    feed.last_heard
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .elapsed()
}

/// The process-wide feed, started on first use. The first command wins: a
/// changed `@sessiond-feed` takes effect when the server restarts.
fn feed(command: &str) -> &'static Feed {
    let mut started_here = false;
    let feed = FEED.get_or_init(|| {
        started_here = true;
        Feed {
            command: command.to_string(),
            board: Mutex::new(Board::default()),
            running: AtomicBool::new(false),
            last_heard: Mutex::new(Instant::now()),
        }
    });
    if started_here {
        let spawned = std::thread::Builder::new()
            .name("sessiond-feed".into())
            .spawn(move || run_feed(feed));
        if let Err(e) = spawned {
            crate::debug_log::server_log("sessiond", &format!("feed thread: {e}"));
        }
    }
    feed
}

/// Run the feed command for the life of the server, restarting it with
/// backoff whenever it exits.
fn run_feed(feed: &'static Feed) {
    let mut backoff = MIN_BACKOFF;
    loop {
        match read_feed_once(feed) {
            Ok(true) => backoff = MIN_BACKOFF,
            Ok(false) => {}
            Err(e) => crate::debug_log::server_log("sessiond", &format!("feed: {e}")),
        }
        feed.running.store(false, Ordering::Relaxed);
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Run the feed command once and apply its lines until it exits. Returns
/// whether it produced at least one sessiond message.
fn read_feed_once(feed: &Feed) -> io::Result<bool> {
    let mut child = shell_command(&feed.command).spawn()?;
    crate::proc_stats::record_spawn(crate::proc_stats::SpawnKind::Other);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("feed command has no stdout"))?;
    // A started feed replays current state, so the old board is stale.
    lock_board(feed).clear();
    mark_heard(feed);
    feed.running.store(true, Ordering::Relaxed);
    let mut applied = false;
    for line in BufReader::new(stdout).lines() {
        let outcome = lock_board(feed).apply_line(&line?);
        if matches!(outcome, Applied::Started | Applied::Alive) {
            mark_heard(feed);
        }
        applied |= outcome != Applied::Ignored;
    }
    child.wait()?;
    Ok(applied)
}

fn shell_command(command: &str) -> Command {
    let mut cmd = if cfg!(windows) {
        use crate::platform::HideWindowCommandExt;
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c.hide_window();
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

/// Resolve a `sessiond_*` format variable, or `None` when `var` is not one.
///
/// `sessiond_feed` is `off`, `down`, `stale` or `live`. `sessiond_summary`,
/// `sessiond_attention` and `sessiond_sessions` are the [`Board`] views of the
/// same names, `sessiond_total` the session count, and `sessiond_<state>` the
/// count of one state. All but `sessiond_feed` are empty while off.
pub fn format_var(var: &str, user_options: &HashMap<String, String>) -> Option<String> {
    let key = var.strip_prefix("sessiond_")?;
    let command = user_options
        .get(FEED_OPTION)
        .filter(|c| !c.trim().is_empty());
    let Some(command) = command else {
        return Some(if key == "feed" { "off" } else { "" }.into());
    };
    let feed = feed(command);
    if key == "feed" {
        let running = feed.running.load(Ordering::Relaxed);
        return Some(feed_status(running, since_heard(feed)).into());
    }
    let board = lock_board(feed);
    Some(match key {
        "summary" => board.summary(),
        "attention" => board.attention(),
        "sessions" => board.sessions(),
        "total" => board.len().to_string(),
        state if rank(state).is_some() => board.count(state).to_string(),
        _ => String::new(),
    })
}

#[cfg(test)]
#[path = "../tests-rs/test_sessiond_state.rs"]
mod tests;
