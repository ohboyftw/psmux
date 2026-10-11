//! Per-pane cache of the OSC 7501 program status a pane's agent reports.
//!
//! The server tick samples each pane's parser once per 100 ms and copies the
//! result here, so format variables never lock the parser.  A state held for
//! less than one tick can be missed.

use vt100::{BlockedKind, ProgramState, ProgramStatusRecord, Screen};

/// Longest `title`/`msg` kept for display outside the grid, in characters.
pub const MAX_TEXT_CHARS: usize = 200;

/// A change of a pane's rolled-up (worst-of) agent state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTransition {
    /// State before the change; `None` = no record.
    pub old: Option<ProgramState>,
    /// State after the change; `None` = no record.
    pub new: Option<ProgramState>,
    /// Id of the record that now carries the worst state (`None` = root).
    pub record_id: Option<String>,
    /// Fields of that record when the change was seen; empty when `new` is `None`.
    pub details: AgentRecordDetails,
}

/// The fields of the record that carries a pane's worst-of state.  Text is
/// sanitised for the status line, `display-message` and toasts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRecordDetails {
    /// Why the record is blocked; only set on blocked records.
    pub kind: Option<BlockedKind>,
    /// Progress 0-100.
    pub progress: Option<u8>,
    /// App name, inherited from the nearest ancestor record.
    pub app: String,
    /// Sanitised title.
    pub title: String,
    /// Sanitised one-line message.
    pub msg: String,
}

/// What the server last read from a pane's OSC 7501 records.
#[derive(Debug, Default)]
pub struct AgentStateCache {
    generation: u64,
    observed: Option<(u64, u64)>,
    root: Option<ProgramState>,
    worst: Option<ProgramState>,
    worst_id: Option<String>,
    details: AgentRecordDetails,
    prev_worst: Option<ProgramState>,
}

impl AgentStateCache {
    /// State of the root record (reported without an `id`).
    pub fn root(&self) -> Option<ProgramState> {
        self.root
    }

    /// Worst state over all records: blocked > error > working > done > idle.
    pub fn worst(&self) -> Option<ProgramState> {
        self.worst
    }

    /// Id of the child record that carries the worst state; `None` for the root.
    pub fn worst_id(&self) -> Option<&str> {
        self.worst_id.as_deref()
    }

    /// Fields of the worst-of record; all empty when there is no record.
    pub fn details(&self) -> &AgentRecordDetails {
        &self.details
    }

    /// Call when the pane's parser is replaced (respawn, clear-history).
    /// A new parser restarts its sequence counter at 0, which can climb back
    /// to the cached value, so the generation makes the next sample differ.
    pub fn parser_replaced(&mut self) {
        self.generation += 1;
        self.root = None;
        self.worst = None;
        self.worst_id = None;
        self.details = AgentRecordDetails::default();
    }

    /// Samples `screen` if its records changed since the last call and
    /// returns the worst-of transition, if any.  Progress-only updates
    /// change the sequence but not the state, so they are not transitions.
    pub fn observe(&mut self, screen: &Screen) -> Option<AgentTransition> {
        if self.observed != Some((self.generation, screen.program_status_seq())) {
            self.refresh(screen);
        }
        if self.worst == self.prev_worst {
            return None;
        }
        let old = std::mem::replace(&mut self.prev_worst, self.worst);
        Some(AgentTransition {
            old,
            new: self.worst,
            record_id: self.worst_id.clone(),
            details: self.details.clone(),
        })
    }

    /// Copies `screen`'s records even when the sequence did not change, as
    /// after the process-exit lifetime rule.  Does not touch transition
    /// detection: the next [`observe`](Self::observe) reports the change.
    pub fn refresh(&mut self, screen: &Screen) {
        self.observed = Some((self.generation, screen.program_status_seq()));
        self.root = screen.program_status().map(ProgramStatusRecord::state);
        let worst = worst_record(screen);
        self.worst = worst.map(|(_, r)| r.state());
        self.worst_id = worst.and_then(|(id, _)| id.map(str::to_owned));
        self.details = worst
            .map(|(id, r)| record_details(screen, id, r))
            .unwrap_or_default();
    }
}

fn worst_record(screen: &Screen) -> Option<(Option<&str>, &ProgramStatusRecord)> {
    let mut worst: Option<(Option<&str>, &ProgramStatusRecord)> = None;
    for (id, record) in screen.program_status_records() {
        if worst.is_none_or(|(_, w)| rank(record.state()) > rank(w.state())) {
            worst = Some((id, record));
        }
    }
    worst
}

fn record_details(
    screen: &Screen,
    id: Option<&str>,
    record: &ProgramStatusRecord,
) -> AgentRecordDetails {
    AgentRecordDetails {
        kind: record.kind(),
        progress: record.progress(),
        app: sanitize_text(screen.program_status_app(id).unwrap_or_default()),
        title: sanitize_text(record.title().unwrap_or_default()),
        msg: sanitize_text(record.msg().unwrap_or_default()),
    }
}

fn rank(state: ProgramState) -> u8 {
    match state {
        ProgramState::Idle => 0,
        ProgramState::Done => 1,
        ProgramState::Working => 2,
        ProgramState::Error => 3,
        ProgramState::Blocked => 4,
    }
}

/// Removes bidi overrides and invisible format characters and caps the
/// length at [`MAX_TEXT_CHARS`].  The parser already rejects control
/// characters, so text shown outside the grid cannot reorder or hide itself.
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_hidden_format_char(c))
        .take(MAX_TEXT_CHARS)
        .collect()
}

fn is_hidden_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// The `#{pane_agent_state}` text for a state; empty when there is no record.
pub fn state_label(state: Option<ProgramState>) -> &'static str {
    match state {
        None => "",
        Some(ProgramState::Idle) => "idle",
        Some(ProgramState::Working) => "working",
        Some(ProgramState::Done) => "done",
        Some(ProgramState::Blocked) => "blocked",
        Some(ProgramState::Error) => "error",
    }
}

/// Parses a state name as `wait-pane --until` takes it; the inverse of
/// [`state_label`] over `idle|working|done|blocked|error`.
pub fn parse_state(name: &str) -> Option<ProgramState> {
    match name {
        "idle" => Some(ProgramState::Idle),
        "working" => Some(ProgramState::Working),
        "done" => Some(ProgramState::Done),
        "blocked" => Some(ProgramState::Blocked),
        "error" => Some(ProgramState::Error),
        _ => None,
    }
}

/// The `#{pane_agent_kind}` text; empty unless the record is blocked with a kind.
pub fn kind_label(kind: Option<BlockedKind>) -> &'static str {
    match kind {
        None => "",
        Some(BlockedKind::Permission) => "permission",
        Some(BlockedKind::Question) => "question",
        Some(BlockedKind::Auth) => "auth",
    }
}

/// Worst of `states` (blocked > error > working > done > idle); `None` when
/// no state is set.  choose-tree rolls panes up into windows and sessions
/// with it.
pub fn worst_of(states: impl IntoIterator<Item = Option<ProgramState>>) -> Option<ProgramState> {
    states.into_iter().flatten().max_by_key(|&s| rank(s))
}

/// Worst of state labels as `#{pane_agent_state}` prints them.  A label
/// outside the closed set, such as an older server's empty value, counts as
/// no record.
pub fn worst_label<'a>(labels: impl IntoIterator<Item = &'a str>) -> &'static str {
    state_label(worst_of(labels.into_iter().map(parse_state)))
}

/// The one-cell glyph choose-tree shows for a state; empty when there is
/// no record.
pub fn state_glyph(state: Option<ProgramState>) -> &'static str {
    match state {
        None => "",
        Some(ProgramState::Idle) => "○",
        Some(ProgramState::Working) => "●",
        Some(ProgramState::Done) => "✓",
        Some(ProgramState::Blocked) => "!",
        Some(ProgramState::Error) => "✗",
    }
}

/// Milliseconds of output silence after which a non-reporting pane is idle.
pub const OUTPUT_STABLE_MS: u64 = 500;

/// Where a pane's [`PaneReadiness`] state came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessSource {
    /// The program reported its state with OSC 7501.
    Reported,
    /// Inferred from output silence.
    OutputStable,
}

/// A pane's state as every readiness check sees it: `#{pane_ready}`,
/// `#{pane_agent_readiness}`, `#{pane_agent_source}`, `wait-pane --ready`, `send-keys --wait-ready`,
/// the spawn `wait_ready` RPC option and the `context_ready` push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneReadiness {
    /// `None` = unknown (no output yet and nothing reported).
    pub state: Option<ProgramState>,
    /// Where `state` came from.
    pub source: ReadinessSource,
}

impl PaneReadiness {
    /// Ready means idle; a `blocked` or `working` report, or output within
    /// the last [`OUTPUT_STABLE_MS`], is not ready.
    pub fn is_ready(&self) -> bool {
        self.state == Some(ProgramState::Idle)
    }
}

/// The single readiness predicate.  A reported `idle`, `working` or
/// `blocked` record wins; otherwise no output yet is unknown,
/// [`OUTPUT_STABLE_MS`] of silence is idle and anything more recent is
/// working.
///
/// `reported` is the worst-of state over all records, so a `working` or
/// `blocked` child record (a background task) keeps the pane not ready under
/// an `idle` root.  `done` and `error` are verdicts, not activity: they
/// survive OSC 133 A and process exit, so letting them win would leave a pane
/// not ready forever after its agent stopped.  With only such records left,
/// silence decides, as it does with no record.  A `working` record left by a
/// crashed agent still wins until OSC 133 A, process exit, RIS or
/// `clear-history`.
///
/// Keyed on a cached record rather than the parser's
/// `program_status_seen()`, which stays true after `state=clear`, so a shell
/// left behind by an agent falls back to silence.
pub fn readiness_of(
    data_version: u64,
    last_output_ms: u64,
    now_ms: u64,
    reported: Option<ProgramState>,
) -> PaneReadiness {
    if let Some(state @ (ProgramState::Idle | ProgramState::Working | ProgramState::Blocked)) =
        reported
    {
        return PaneReadiness {
            state: Some(state),
            source: ReadinessSource::Reported,
        };
    }
    PaneReadiness {
        state: silence_state(data_version, last_output_ms, now_ms),
        source: ReadinessSource::OutputStable,
    }
}

fn silence_state(data_version: u64, last_output_ms: u64, now_ms: u64) -> Option<ProgramState> {
    if data_version == 0 || last_output_ms == 0 {
        None
    } else if now_ms.saturating_sub(last_output_ms) >= OUTPUT_STABLE_MS {
        Some(ProgramState::Idle)
    } else {
        Some(ProgramState::Working)
    }
}

/// The `#{pane_agent_source}` text and the `context_ready` `ready_signal`.
/// `output_stable` alone does not mean silent: with an unknown state (no
/// output yet) `#{pane_agent_readiness}` is empty and `#{pane_ready}` is 0.
pub fn source_label(source: ReadinessSource) -> &'static str {
    match source {
        ReadinessSource::Reported => "reported",
        ReadinessSource::OutputStable => "output_stable",
    }
}

/// Wall-clock milliseconds since the Unix epoch, the clock the pane reader
/// threads stamp `last_output_time` with.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl crate::types::Pane {
    /// This pane's [`PaneReadiness`] at `now_ms`.  Reads only atomics and
    /// the agent-state cache, never the parser, so it is safe from format
    /// expansion.
    pub fn readiness(&self, now_ms: u64) -> PaneReadiness {
        use std::sync::atomic::Ordering::Acquire;
        readiness_of(
            self.data_version.load(Acquire),
            self.last_output_time.load(Acquire),
            now_ms,
            self.agent_state.worst(),
        )
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_agent_state.rs"]
mod tests;
