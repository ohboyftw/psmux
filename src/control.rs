//! Control mode (`-C` / `-CC`) foundation.
//!
//! This module owns the *types* and the *single fan-out trigger* for tmux
//! control-mode protocol consumers (iTerm2, libtmux, tmuxinator, …).
//!
//! Stage 1A scope (this file): types + `emit_lifecycle()` + line formatters.
//! Stage 1B scope (separate dispatch): `dispatch_control_command`,
//! `format_list_sessions`, sentinel handling.
//!
//! ## Architectural invariants (enforced by `tests-rs/test_cc_emit_lifecycle_singleton.rs`)
//!
//! 1. `crate::mycel::publish_pane_event` is called from EXACTLY ONE place
//!    in the source tree: inside [`emit_lifecycle`]. Every other lifecycle
//!    notification site MUST go via `emit_lifecycle()`.
//! 2. `app.control_clients` is read from EXACTLY ONE place: inside
//!    [`emit_lifecycle`]. Stage 1B's `dispatch_control_command` writes to
//!    the field (Register/Deregister), but does not iterate it.
//!
//! These invariants are what makes the "parallel transports" coexistence
//! model in `.claude/internal/design-control-mode-vs-custompanebackend.md`
//! safe: if the helper is the only fan-out point, the relative ordering of
//! mycel publish vs control-client emission is determined inside the helper
//! and is consistent across all consumers.
//!
//! ## Fan-out ordering
//!
//! `emit_lifecycle()` writes to control-mode clients FIRST, then to the
//! mycel bus. Control-mode consumers (iTerm2 in particular) treat
//! `%notification` lines as authoritative state transitions and are sensitive
//! to ordering relative to subsequent commands. mycel subscribers are async
//! observers and tolerate small lag. Documented at design section 4.

use std::sync::{mpsc, Mutex, OnceLock};

/// Control mode flavour.
///
/// - `Echo` (`-C`): line-shaped, server echoes commands and prints responses.
///   Used by humans and basic libtmux clients.
/// - `NoEcho` (`-CC`): line-shaped, no echo, with `%begin`/`%end` framing
///   and `%notification` push events. Used by iTerm2 and tmuxinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    /// `-C` mode — server echoes commands.
    Echo,
    /// `-CC` mode — iTerm2 protocol, no echo, framed responses.
    NoEcho,
}

/// A line-shaped notification destined for one or more control-mode clients.
///
/// Stage 1B serializes [`LifecycleEvent`] into one of these and feeds it
/// to per-client writers. Variants mirror upstream tmux's
/// `%notification` set.
#[derive(Debug, Clone)]
pub enum ControlNotification {
    /// `%output %<pane_id> <bytes>` — pane stdout/stderr.
    Output { pane_id: usize, payload: String },
    /// `%window-add @<window_id>`
    WindowAdd { window_id: usize },
    /// `%window-close @<window_id>`
    WindowClose { window_id: usize },
    /// `%window-renamed @<window_id> <new_name>`
    WindowRenamed { window_id: usize, new_name: String },
    /// `%session-changed $<session_id> <name>`
    SessionChanged { session_id: usize, name: String },
    /// `%sessions-changed` — bulk hint that the session list moved.
    SessionsChanged,
    /// `%exit` (with optional reason for `%exit <reason>`).
    Exit { reason: Option<String> },
    /// `%error <text>`
    Error { text: String },
}

/// A control-mode client attached to the server.
///
/// Stage 1A holds only the bookkeeping fields. Stage 1B fills in the
/// writer-task wiring (per-client bounded `mpsc::Sender<String>` with
/// drop-the-oldest backpressure on `%output` payloads — see design section 4
/// risk table).
pub struct ControlClient {
    /// Server-allocated identifier; matches `CtrlReq::ControlRegister.client_id`.
    pub id: u64,
    /// `-C` vs `-CC` selected at attach time.
    pub mode: ControlMode,
    /// Handle to the per-client writer task. Stage 1B drains lines off this
    /// channel and writes them to the client socket / stdio bridge.
    /// Bounded capacity to apply backpressure (see design section 4).
    pub send_tx: mpsc::Sender<String>,
}

impl ControlClient {
    /// Construct a new client. Stage 1B will allocate the id from a server-side
    /// counter and pair `send_tx` with a writer task.
    pub fn new(id: u64, mode: ControlMode, send_tx: mpsc::Sender<String>) -> Self {
        Self { id, mode, send_tx }
    }

    /// Enqueue one already-formatted line for this client.
    ///
    /// Stage 1A: the helper [`emit_lifecycle`] calls this once per attached
    /// client. Failures (channel closed) mean the writer task is gone and the
    /// client has effectively disconnected; we silently drop — Stage 1B's
    /// `Deregister` path takes care of the cleanup.
    pub fn write_line(&self, line: &str) {
        let _ = self.send_tx.send(line.to_string());
    }
}

/// A canonical lifecycle event, emitted exactly once at the source-of-truth
/// trigger point. Both mycel and control-mode emitters consume it and
/// translate into their native shapes.
///
/// This type intentionally does NOT derive `Serialize` — mycel consumers
/// receive a hand-built JSON payload (preserving the wire shape backlog
/// row 46 callers depend on), and control-mode consumers receive an
/// `%notification` line. Adding a serde derive here would invite drift
/// between the wire formats.
#[derive(Debug, Clone)]
pub enum LifecycleEvent<'a> {
    /// A pane was created and assigned `pane_id`. `command` is the spawn argv
    /// joined with spaces, or empty for the default shell.
    PaneCreated { pane_id: usize, command: &'a str },
    /// A pane reached the readiness signal (idle prompt detected).
    PaneReady { pane_id: usize, elapsed_ms: u64 },
    /// A pane exited. `exit_code` is `None` if the child was killed.
    PaneExited {
        pane_id: usize,
        exit_code: Option<i32>,
    },
    /// A non-interactive `exec` finished. Distinct from `PaneExited` —
    /// the pane is still alive.
    ExecCompleted {
        context_id: &'a str,
        exit_code: i32,
        elapsed_ms: u64,
        command: &'a str,
    },
    /// A new session was created.
    SessionCreated { session_name: &'a str },
    /// A session was renamed.
    SessionRenamed {
        new_name: &'a str,
        old_name: &'a str,
    },
    /// A session was killed (pre-shutdown notification).
    SessionKilled { session_name: &'a str },
}

/// Cross-thread snapshot of control-client write channels.
///
/// `app.control_clients` is owned by the main server thread (single-threaded
/// `run_server` event loop). Threads that need to fan out lifecycle events
/// without holding the App lock (e.g. CustomPaneBackend's dispatcher thread,
/// mycel's tokio task) read this mirror.
///
/// Stage 1B's `CtrlReq::ControlRegister` / `ControlDeregister` handlers MUST
/// rebuild this snapshot inside the same critical section that mutates
/// `app.control_clients`, via [`refresh_global_clients`]. Until Stage 1B
/// wires that in, the global stays empty — backend-thread emits are mycel-only.
static GLOBAL_CONTROL_CLIENTS: OnceLock<Mutex<Vec<mpsc::Sender<String>>>> = OnceLock::new();

fn global_clients() -> &'static Mutex<Vec<mpsc::Sender<String>>> {
    GLOBAL_CONTROL_CLIENTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Replace the cross-thread mirror of control-client senders.
///
/// Called by Stage 1B's `ControlRegister` / `ControlDeregister` handlers
/// after mutating `app.control_clients`, to keep the snapshot consistent.
/// In Stage 1A this is exposed but not yet called (no register path exists).
pub fn refresh_global_clients(senders: Vec<mpsc::Sender<String>>) {
    if let Ok(mut guard) = global_clients().lock() {
        *guard = senders;
    }
}

/// Single fan-out trigger for lifecycle events.
///
/// Call this from the canonical site that today calls
/// `crate::mycel::publish_pane_event(...)`. Do NOT add new mycel publish
/// sites elsewhere — every lifecycle event flows through here.
///
/// Fan-out order: control-mode clients first (synchronous best-effort
/// `mpsc::send`), then mycel publish (also non-blocking). See the
/// "Fan-out ordering" rationale at the top of this module.
///
/// `clients` is borrowed from `app.control_clients` when called from the
/// main server thread; pass `&[]` from cross-thread sites (the global mirror
/// in [`refresh_global_clients`] picks them up).
pub fn emit_lifecycle(clients: &[ControlClient], evt: &LifecycleEvent<'_>) {
    let line = lifecycle_to_notification(evt).map(|n| format_notification(&n));

    // Emitter 1a — main-thread clients (the borrowed slice).
    if let Some(ref l) = line {
        for client in clients.iter() {
            client.write_line(l);
        }
    }

    // Emitter 1b — cross-thread mirror (other threads' view of the same
    // clients). We tolerate the brief window where main-thread mutation
    // hasn't refreshed the mirror yet — duplicates on the receiving end
    // are filtered by client_id at the writer task (Stage 1B).
    if let Some(ref l) = line {
        if !clients.is_empty() {
            // Main-thread already wrote — do NOT also iterate the mirror,
            // or we double-emit. The mirror is only for cross-thread sites
            // that pass an empty `clients` slice.
        } else if let Ok(guard) = global_clients().lock() {
            for tx in guard.iter() {
                let _ = tx.send(l.clone());
            }
        }
    }

    // Emitter 2 — mycel bus (JSON-shaped).
    #[cfg(feature = "mycel")]
    publish_lifecycle_to_mycel(evt);

    // Suppress unused-variable warning when mycel feature is off.
    #[cfg(not(feature = "mycel"))]
    let _ = evt;
}

/// Translate a [`LifecycleEvent`] to a [`ControlNotification`], or `None`
/// if there is no corresponding tmux-protocol line for this event.
///
/// Stage 1A maps the obvious cases; Stage 1B refines as it implements
/// `dispatch_control_command` and discovers what notifications iTerm2
/// actually consumes.
fn lifecycle_to_notification(evt: &LifecycleEvent<'_>) -> Option<ControlNotification> {
    match evt {
        // Pane creation surfaces to upstream as a window-add (pane-create
        // doesn't have a distinct `%notification` in tmux's CC protocol —
        // panes are addressed inside windows).
        LifecycleEvent::PaneCreated { .. } => None,
        LifecycleEvent::PaneReady { .. } => None,
        LifecycleEvent::PaneExited { .. } => None,
        LifecycleEvent::ExecCompleted { .. } => None,
        LifecycleEvent::SessionCreated { .. } => Some(ControlNotification::SessionsChanged),
        LifecycleEvent::SessionRenamed { .. } => Some(ControlNotification::SessionsChanged),
        LifecycleEvent::SessionKilled { .. } => Some(ControlNotification::SessionsChanged),
    }
}

/// Format a [`ControlNotification`] as the wire-shape line written to a CC
/// client's stdout (no trailing newline — the writer task adds CRLF).
pub fn format_notification(n: &ControlNotification) -> String {
    match n {
        ControlNotification::Output { pane_id, payload } => {
            format!("%output %{pane_id} {payload}")
        }
        ControlNotification::WindowAdd { window_id } => {
            format!("%window-add @{window_id}")
        }
        ControlNotification::WindowClose { window_id } => {
            format!("%window-close @{window_id}")
        }
        ControlNotification::WindowRenamed {
            window_id,
            new_name,
        } => format!("%window-renamed @{window_id} {new_name}"),
        ControlNotification::SessionChanged { session_id, name } => {
            format!("%session-changed ${session_id} {name}")
        }
        ControlNotification::SessionsChanged => "%sessions-changed".to_string(),
        ControlNotification::Exit { reason: Some(r) } => format!("%exit {r}"),
        ControlNotification::Exit { reason: None } => "%exit".to_string(),
        ControlNotification::Error { text } => format!("%error {text}"),
    }
}

/// Format the `%end <number> <client> <flags>` framing line that closes a
/// successful response block in `-CC` mode. Caller supplies the matching
/// `%begin` number and client id from the request.
pub fn format_end(number: u64, client_id: u64, flags: u32) -> String {
    format!("%end {number} {client_id} {flags}")
}

/// Format the `%error <number> <client> <flags>` framing line that closes a
/// failed response block in `-CC` mode. Same arity as [`format_end`] — the
/// error text is emitted as one or more body lines BEFORE this terminator.
pub fn format_error(number: u64, client_id: u64, flags: u32) -> String {
    format!("%error {number} {client_id} {flags}")
}

/// JSON-shaped emit to the mycel bus.
///
/// This is a thin private wrapper around the existing
/// `crate::mycel::publish_pane_event`. Centralising it here is what makes
/// the "exactly one mycel publish site" invariant enforceable.
#[cfg(feature = "mycel")]
fn publish_lifecycle_to_mycel(evt: &LifecycleEvent<'_>) {
    use crate::mycel::topics;
    match evt {
        LifecycleEvent::PaneCreated { pane_id, command } => {
            crate::mycel::publish_pane_event(
                topics::PANE_CREATED,
                &serde_json::json!({
                    "pane_id": format!("%{pane_id}"),
                    "command": command,
                }),
            );
        }
        LifecycleEvent::PaneReady {
            pane_id,
            elapsed_ms,
        } => {
            crate::mycel::publish_pane_event(
                topics::PANE_READY,
                &serde_json::json!({
                    "pane_id": format!("%{pane_id}"),
                    "elapsed_ms": *elapsed_ms,
                }),
            );
        }
        LifecycleEvent::PaneExited { pane_id, exit_code } => {
            let payload = serde_json::json!({
                "pane_id": format!("%{pane_id}"),
                "exit_code": exit_code,
            });
            crate::mycel::publish_pane_event(topics::PANE_EXITED, &payload);
            // Deprecated shim — kept so existing canopy subscribers don't
            // break. Will be removed in a future release.
            crate::mycel::publish_pane_event("psmux/pane/died", &payload);
        }
        LifecycleEvent::ExecCompleted {
            context_id,
            exit_code,
            elapsed_ms,
            command,
        } => {
            crate::mycel::publish_pane_event(
                topics::EXEC_COMPLETED,
                &serde_json::json!({
                    "pane_id": context_id,
                    "pid": 0u32,
                    "exit_code": exit_code,
                    "elapsed_ms": *elapsed_ms,
                    "command": command,
                }),
            );
        }
        LifecycleEvent::SessionCreated { session_name } => {
            crate::mycel::publish_pane_event(
                topics::SESSION_CREATED,
                &serde_json::json!({
                    "session_name": session_name,
                    "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
                }),
            );
        }
        LifecycleEvent::SessionRenamed { new_name, old_name } => {
            crate::mycel::publish_pane_event(
                topics::SESSION_RENAMED,
                &serde_json::json!({
                    "session_name": new_name,
                    "old_name": old_name,
                    "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
                }),
            );
        }
        LifecycleEvent::SessionKilled { session_name } => {
            crate::mycel::publish_pane_event(
                topics::SESSION_KILLED,
                &serde_json::json!({
                    "session_name": session_name,
                    "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
                }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn format_notification_output_includes_pane_id_and_payload() {
        let n = ControlNotification::Output {
            pane_id: 7,
            payload: "hello".into(),
        };
        assert_eq!(format_notification(&n), "%output %7 hello");
    }

    #[test]
    fn format_notification_window_add_uses_at_prefix() {
        let n = ControlNotification::WindowAdd { window_id: 3 };
        assert_eq!(format_notification(&n), "%window-add @3");
    }

    #[test]
    fn format_notification_session_changed_uses_dollar_prefix() {
        let n = ControlNotification::SessionChanged {
            session_id: 1,
            name: "main".into(),
        };
        assert_eq!(format_notification(&n), "%session-changed $1 main");
    }

    #[test]
    fn format_notification_sessions_changed_has_no_args() {
        let n = ControlNotification::SessionsChanged;
        assert_eq!(format_notification(&n), "%sessions-changed");
    }

    #[test]
    fn format_notification_exit_with_and_without_reason() {
        assert_eq!(
            format_notification(&ControlNotification::Exit { reason: None }),
            "%exit"
        );
        assert_eq!(
            format_notification(&ControlNotification::Exit {
                reason: Some("bye".into())
            }),
            "%exit bye"
        );
    }

    #[test]
    fn format_end_and_error_share_arity() {
        assert_eq!(format_end(42, 1, 0), "%end 42 1 0");
        assert_eq!(format_error(42, 1, 0), "%error 42 1 0");
    }

    #[test]
    fn emit_lifecycle_with_no_clients_is_a_noop_for_control_path() {
        // No clients attached — the helper still runs (mycel side is gated
        // by feature); the control-side loop must not panic on empty input.
        emit_lifecycle(
            &[],
            &LifecycleEvent::PaneCreated {
                pane_id: 1,
                command: "pwsh",
            },
        );
    }

    #[test]
    fn emit_lifecycle_writes_session_changes_to_attached_clients() {
        // Architectural check: a session-lifecycle event reaches every
        // attached client exactly once. We don't assert ordering vs mycel
        // here — that's a separate test gated on the mycel feature.
        let (tx_a, rx_a) = mpsc::channel::<String>();
        let (tx_b, rx_b) = mpsc::channel::<String>();
        let clients = vec![
            ControlClient::new(1, ControlMode::NoEcho, tx_a),
            ControlClient::new(2, ControlMode::NoEcho, tx_b),
        ];
        emit_lifecycle(
            &clients,
            &LifecycleEvent::SessionCreated {
                session_name: "alpha",
            },
        );
        assert_eq!(rx_a.try_recv().unwrap(), "%sessions-changed");
        assert_eq!(rx_b.try_recv().unwrap(), "%sessions-changed");
        assert!(rx_a.try_recv().is_err());
        assert!(rx_b.try_recv().is_err());
    }

    #[test]
    fn emit_lifecycle_pane_events_do_not_emit_cc_lines_in_stage_1a() {
        // Pane lifecycle events do not (yet) translate to a CC notification —
        // the upstream protocol uses %window-add/%window-close keyed on
        // window_id, which Stage 1A doesn't carry through. Stage 1B refines.
        let (tx, rx) = mpsc::channel::<String>();
        let clients = vec![ControlClient::new(1, ControlMode::NoEcho, tx)];
        emit_lifecycle(
            &clients,
            &LifecycleEvent::PaneReady {
                pane_id: 9,
                elapsed_ms: 12,
            },
        );
        assert!(
            rx.try_recv().is_err(),
            "Stage 1A: pane events do not emit CC lines yet"
        );
    }
}
