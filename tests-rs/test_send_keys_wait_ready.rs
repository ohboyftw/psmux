// send-keys --wait-ready: after polling, the -t temp focus must be put back
// on the target, because every poll fires the server's focus restore.

use super::*;
use crate::agent_state::{PaneReadiness, ReadinessSource};
use vt100::ProgramState;

fn readiness(state: ProgramState) -> PaneReadiness {
    PaneReadiness {
        state: Some(state),
        source: ReadinessSource::OutputStable,
    }
}

/// Answer requests like the server loop: the pane turns idle after
/// `busy_polls` polls, and the focus check reports `pane_exists`.
/// Returns the request kinds in arrival order.
fn fake_server(rx: mpsc::Receiver<CtrlReq>, busy_polls: usize, pane_exists: bool) -> Vec<String> {
    let mut seen = Vec::new();
    let mut polls = 0;
    while let Ok(req) = rx.recv() {
        match req {
            CtrlReq::QueryPaneState(pid, resp) => {
                seen.push(format!("poll %{pid}"));
                polls += 1;
                let state = if polls > busy_polls {
                    ProgramState::Idle
                } else {
                    ProgramState::Working
                };
                let _ = resp.send((1, readiness(state)));
            }
            CtrlReq::FocusPaneTempCheck(pid, resp) => {
                seen.push(format!("focus %{pid}"));
                let _ = resp.send(pane_exists);
            }
            CtrlReq::PaneExists(pid, resp) => {
                seen.push(format!("exists %{pid}"));
                let _ = resp.send(pane_exists);
            }
            _ => seen.push("other".to_string()),
        }
    }
    seen
}

fn run(busy_polls: usize, pane_exists: bool, limit: Duration) -> (bool, Vec<String>) {
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || fake_server(rx, busy_polls, pane_exists));
    let ok = wait_ready_then_refocus(&tx, 7, limit);
    drop(tx);
    (ok, server.join().expect("fake server panicked"))
}

#[test]
fn wait_ready_when_pane_turns_idle_then_refocuses_target_last() {
    let (ok, seen) = run(2, true, Duration::from_secs(5));
    assert!(ok);
    assert_eq!(seen, ["poll %7", "poll %7", "poll %7", "focus %7"]);
}

#[test]
fn wait_ready_when_pane_already_idle_then_still_refocuses() {
    let (ok, seen) = run(0, true, Duration::from_secs(5));
    assert!(ok);
    assert_eq!(seen, ["poll %7", "focus %7"]);
}

#[test]
fn wait_ready_when_deadline_passes_then_refocuses_and_delivers() {
    let (ok, seen) = run(usize::MAX, true, Duration::ZERO);
    assert!(ok);
    assert_eq!(seen.last().map(String::as_str), Some("focus %7"));
}

#[test]
fn wait_ready_when_pane_gone_then_reports_not_found() {
    let (ok, _) = run(0, false, Duration::from_secs(5));
    assert!(!ok);
}

/// A pane that closed: polls answer "no state" and the existence probe says no.
fn vanished_pane_server(rx: mpsc::Receiver<CtrlReq>) -> Vec<String> {
    let mut seen = Vec::new();
    while let Ok(req) = rx.recv() {
        match req {
            CtrlReq::QueryPaneState(pid, resp) => {
                seen.push(format!("poll %{pid}"));
                let _ = resp.send((0, crate::agent_state::readiness_of(0, 0, 0, None)));
            }
            CtrlReq::PaneExists(pid, resp) => {
                seen.push(format!("exists %{pid}"));
                let _ = resp.send(false);
            }
            _ => seen.push("other".to_string()),
        }
    }
    seen
}

#[test]
fn wait_ready_when_pane_closes_during_wait_then_stops_without_refocus() {
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || vanished_pane_server(rx));
    let started = std::time::Instant::now();
    let ok = wait_ready_then_refocus(&tx, 7, Duration::from_secs(30));
    drop(tx);
    let seen = server.join().expect("fake server panicked");
    assert!(!ok);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(seen, ["poll %7", "exists %7"]);
}
