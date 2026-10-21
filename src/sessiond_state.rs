//! Cockpit view of the session states sessiond publishes (scale plan I17).
//!
//! sessiond sends `sessiond/session/state` and `sessiond/session/removed`
//! messages, specified in the sessiond-sidecar repo's `docs/state-topic.md`.
//! This module keeps the latest state of every session and exposes counts as
//! `#{sessiond_*}` format variables, so a status line can show which sessions
//! on the box are blocked on a human.
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
use std::time::Duration;

use serde::Deserialize;

/// The user option that turns the feed on.
pub const FEED_OPTION: &str = "@sessiond-feed";

const STATE_TOPIC: &str = "sessiond/session/state";
const REMOVED_TOPIC: &str = "sessiond/session/removed";

/// The v1 state set with its summary letter, most urgent first.
const STATES: [(&str, &str); 5] = [
    ("blocked", "B"),
    ("error", "E"),
    ("working", "W"),
    ("done", "D"),
    ("idle", "I"),
];

const MIN_BACKOFF: Duration = Duration::from_secs(5);
/// Every restart spawns a process, and some machines leak every process that
/// exits, so a feed that keeps dying is retried at most this often.
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// The latest known state of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub state: String,
    pub since: String,
    pub reason: String,
}

/// Latest state per session, keyed by sessiond's session id.
#[derive(Debug, Default)]
pub struct Board {
    entries: HashMap<String, Entry>,
}

#[derive(Deserialize)]
struct Envelope {
    topic: String,
    payload: Payload,
}

/// Unknown keys are ignored, as the contract requires of consumers.
#[derive(Deserialize)]
struct Payload {
    session: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    since: String,
    #[serde(default)]
    reason: String,
}

impl Board {
    /// Apply one feed line. Returns false when the line is not a v1 sessiond
    /// message: malformed JSON, another topic, or a state outside the v1 set.
    pub fn apply_line(&mut self, line: &str) -> bool {
        let Ok(envelope) = serde_json::from_str::<Envelope>(line.trim()) else {
            return false;
        };
        match envelope.topic.as_str() {
            STATE_TOPIC => self.apply_state(envelope.payload),
            REMOVED_TOPIC => {
                self.entries.remove(&envelope.payload.session);
                true
            }
            _ => false,
        }
    }

    fn apply_state(&mut self, payload: Payload) -> bool {
        let Some(state) = payload
            .state
            .filter(|s| STATES.iter().any(|(name, _)| name == s))
        else {
            return false;
        };
        let entry = Entry {
            state,
            since: payload.since,
            reason: payload.reason,
        };
        self.entries.insert(payload.session, entry);
        true
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

    fn clear(&mut self) {
        self.entries.clear();
    }
}

struct Feed {
    command: String,
    board: Mutex<Board>,
    live: AtomicBool,
}

static FEED: OnceLock<Feed> = OnceLock::new();

fn lock_board(feed: &Feed) -> MutexGuard<'_, Board> {
    feed.board.lock().unwrap_or_else(|e| e.into_inner())
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
            live: AtomicBool::new(false),
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
        feed.live.store(false, Ordering::Relaxed);
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
    feed.live.store(true, Ordering::Relaxed);
    let mut applied = false;
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        applied |= lock_board(feed).apply_line(&line);
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
/// `sessiond_feed` is `off`, `live` or `down`. `sessiond_summary` is the
/// [`Board::summary`] line, `sessiond_total` the session count, and
/// `sessiond_<state>` the count of one state. All are empty while off.
pub fn format_var(var: &str, user_options: &HashMap<String, String>) -> Option<String> {
    let key = var.strip_prefix("sessiond_")?;
    let command = user_options
        .get(FEED_OPTION)
        .filter(|c| !c.trim().is_empty());
    let Some(command) = command else {
        return Some(if key == "feed" {
            "off".into()
        } else {
            String::new()
        });
    };
    let feed = feed(command);
    if key == "feed" {
        let live = feed.live.load(Ordering::Relaxed);
        return Some(if live { "live" } else { "down" }.into());
    }
    let board = lock_board(feed);
    Some(match key {
        "summary" => board.summary(),
        "total" => board.len().to_string(),
        state if STATES.iter().any(|(name, _)| *name == state) => board.count(state).to_string(),
        _ => String::new(),
    })
}

#[cfg(test)]
#[path = "../tests-rs/test_sessiond_state.rs"]
mod tests;
