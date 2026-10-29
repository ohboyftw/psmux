use super::*;
use crate::agent_state::{
    parse_state, state_label, AgentStateCache, PaneReadiness, ReadinessSource,
};
use crate::wait_for::WaitCondition;
use std::time::{Duration, Instant};
use vt100::ProgramState::{Blocked, Done, Error, Idle, Working};

const ALL: [ProgramState; 5] = [Idle, Working, Done, Blocked, Error];

fn live(readiness: Option<ProgramState>, worst: Option<ProgramState>) -> Option<UntilView> {
    Some(UntilView {
        dead: false,
        readiness,
        worst,
    })
}

fn dead(worst: Option<ProgramState>) -> Option<UntilView> {
    Some(UntilView {
        dead: true,
        readiness: Some(Idle),
        worst,
    })
}

fn verdict(want: ProgramState, view: Option<UntilView>) -> Option<i32> {
    until_verdict(want, view, Instant::now(), None)
}

#[test]
fn parse_state_when_given_each_label_then_round_trips() {
    for s in ALL {
        assert_eq!(parse_state(state_label(Some(s))), Some(s));
    }
}

#[test]
fn parse_state_when_not_a_state_then_none() {
    for name in ["", "clear", "IDLE", "ready", "blocked "] {
        assert_eq!(parse_state(name), None, "{name:?}");
    }
}

#[test]
fn wait_condition_parse_when_state_blocked_then_state_condition() {
    let c = WaitCondition::parse("state", Some("blocked"), None).unwrap();
    assert!(matches!(c, WaitCondition::State { state: Blocked }));
}

#[test]
fn wait_condition_parse_when_state_unknown_then_error() {
    assert!(WaitCondition::parse("state", Some("clear"), None).is_err());
}

#[test]
fn wait_condition_parse_when_state_missing_then_error() {
    assert!(WaitCondition::parse("state", None, None).is_err());
}

#[test]
fn until_idle_when_pane_ready_then_met_at_once() {
    assert_eq!(verdict(Idle, live(Some(Idle), None)), Some(UNTIL_MET));
}

#[test]
fn until_idle_when_reported_working_then_keeps_waiting() {
    assert_eq!(verdict(Idle, live(Some(Working), Some(Working))), None);
}

#[test]
fn until_idle_matches_ready_for_every_readiness() {
    for state in [None, Some(Idle), Some(Working), Some(Blocked)] {
        let r = PaneReadiness {
            state,
            source: ReadinessSource::Reported,
        };
        let met = verdict(Idle, live(r.state, None)) == Some(UNTIL_MET);
        assert_eq!(met, r.is_ready(), "{state:?}");
    }
}

#[test]
fn until_working_when_only_output_is_fresh_then_keeps_waiting() {
    assert_eq!(verdict(Working, live(Some(Working), None)), None);
}

#[test]
fn until_working_when_reported_working_then_met() {
    assert_eq!(
        verdict(Working, live(Some(Working), Some(Working))),
        Some(UNTIL_MET)
    );
}

#[test]
fn until_done_when_reported_done_on_silent_pane_then_met() {
    assert_eq!(verdict(Done, live(Some(Idle), Some(Done))), Some(UNTIL_MET));
}

#[test]
fn until_error_when_worst_is_done_then_keeps_waiting() {
    assert_eq!(verdict(Error, live(Some(Idle), Some(Done))), None);
}

#[test]
fn until_any_when_pane_gone_then_gone() {
    for s in ALL {
        assert_eq!(verdict(s, None), Some(UNTIL_GONE), "{s:?}");
    }
}

#[test]
fn until_working_when_pane_dead_then_gone() {
    assert_eq!(verdict(Working, dead(None)), Some(UNTIL_GONE));
}

#[test]
fn until_idle_when_pane_dead_and_silent_then_gone() {
    assert_eq!(verdict(Idle, dead(None)), Some(UNTIL_GONE));
}

#[test]
fn until_working_when_dead_before_exit_rule_then_gone() {
    assert_eq!(verdict(Working, dead(Some(Working))), Some(UNTIL_GONE));
}

#[test]
fn until_done_when_dead_with_surviving_done_then_met() {
    assert_eq!(verdict(Done, dead(Some(Done))), Some(UNTIL_MET));
}

#[test]
fn until_error_when_dead_with_surviving_done_then_gone() {
    assert_eq!(verdict(Error, dead(Some(Done))), Some(UNTIL_GONE));
}

#[test]
fn until_blocked_when_deadline_passed_then_timeout() {
    let now = Instant::now();
    let v = until_verdict(Blocked, live(Some(Idle), None), now, Some(now));
    assert_eq!(v, Some(UNTIL_TIMEOUT));
}

#[test]
fn until_blocked_when_deadline_ahead_then_keeps_waiting() {
    let now = Instant::now();
    let dl = now + Duration::from_secs(5);
    assert_eq!(
        until_verdict(Blocked, live(Some(Idle), None), now, Some(dl)),
        None
    );
}

#[test]
fn until_working_when_met_at_deadline_then_met_not_timeout() {
    let now = Instant::now();
    let v = until_verdict(Working, live(Some(Working), Some(Working)), now, Some(now));
    assert_eq!(v, Some(UNTIL_MET));
}

#[test]
fn until_blocked_when_child_blocked_under_working_root_then_met() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(b"\x1b]7501;state=working\x1b\\\x1b]7501;id=t1:state=blocked\x1b\\");
    let mut cache = AgentStateCache::default();
    cache.observe(parser.screen());
    assert_eq!(
        verdict(Blocked, live(Some(Blocked), cache.worst())),
        Some(UNTIL_MET)
    );
}

#[test]
fn until_outcome_when_each_code_then_matching_kind() {
    assert_eq!(
        until_outcome(UNTIL_MET, 5),
        WaitOutcome::Success { elapsed_ms: 5 }
    );
    assert_eq!(
        until_outcome(UNTIL_TIMEOUT, 5),
        WaitOutcome::Timeout { elapsed_ms: 5 }
    );
    assert!(matches!(
        until_outcome(UNTIL_GONE, 5),
        WaitOutcome::Error { .. }
    ));
}

#[test]
fn wait_until_when_server_gone_then_gone() {
    let (tx, rx) = mpsc::channel();
    drop(rx);
    assert_eq!(wait_until(&tx, Some(1), Idle, Some(100)), UNTIL_GONE);
}

#[test]
fn wait_until_when_server_answers_then_returns_its_code() {
    let (tx, rx) = mpsc::channel::<CtrlReq>();
    let server = std::thread::spawn(move || {
        if let Ok(CtrlReq::WaitPaneUntil {
            pane_id,
            want,
            resp,
            ..
        }) = rx.recv()
        {
            assert_eq!((pane_id, want), (Some(7), Blocked));
            let _ = resp.send(UNTIL_TIMEOUT);
        }
    });
    assert_eq!(wait_until(&tx, Some(7), Blocked, Some(100)), UNTIL_TIMEOUT);
    server.join().unwrap();
}

fn rpc_wait_state(pane_id: &str) -> String {
    format!(
        r#"{{"id":"1","method":"wait_for","params":{{"condition":"state","arg":"blocked","pane_id":"{pane_id}","timeout_ms":100}}}}"#
    )
}

#[test]
fn rpc_wait_for_state_when_pane_id_not_percent_n_then_invalid_params() {
    let (tx, _rx) = mpsc::channel::<CtrlReq>();
    let resp = crate::backend::dispatcher::dispatch_rpc(&rpc_wait_state("3"), &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602);
}

#[test]
fn rpc_wait_for_state_when_server_answers_met_then_success_like_cli() {
    let (tx, rx) = mpsc::channel::<CtrlReq>();
    let server = std::thread::spawn(move || {
        if let Ok(CtrlReq::WaitPaneUntil {
            pane_id,
            want,
            resp,
            ..
        }) = rx.recv()
        {
            assert_eq!((pane_id, want), (Some(7), Blocked));
            let _ = resp.send(UNTIL_MET);
        }
    });
    let resp = crate::backend::dispatcher::dispatch_rpc(&rpc_wait_state("%7"), &tx).unwrap();
    server.join().unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["kind"], "success");
}

#[test]
fn wait_for_exit_code_when_server_closed_connection_then_2() {
    assert_eq!(wait_for_exit_code("", false), UNTIL_GONE);
    assert_eq!(wait_for_exit_code("", true), UNTIL_GONE);
}

#[test]
fn wait_for_exit_code_when_server_answers_then_mapped() {
    assert_eq!(wait_for_exit_code("OK", false), 0);
    assert_eq!(wait_for_exit_code("TIMEOUT", false), 1);
    assert_eq!(wait_for_exit_code("ERROR: pane is dead or gone", false), 2);
    assert_eq!(
        wait_for_exit_code(r#"{"kind":"success","elapsed_ms":3}"#, true),
        0
    );
    assert_eq!(
        wait_for_exit_code(r#"{"kind":"timeout","elapsed_ms":3}"#, true),
        1
    );
    assert_eq!(
        wait_for_exit_code(r#"{"kind":"error","reason":"x"}"#, true),
        2
    );
}

#[test]
fn wait_pane_exit_code_when_until_and_server_closed_connection_then_2() {
    assert_eq!(wait_pane_exit_code("", true), UNTIL_GONE);
}

#[test]
fn wait_pane_exit_code_when_server_answers_then_code_passed_through() {
    assert_eq!(wait_pane_exit_code("0", true), UNTIL_MET);
    assert_eq!(wait_pane_exit_code("1", true), UNTIL_TIMEOUT);
    assert_eq!(wait_pane_exit_code("2", true), UNTIL_GONE);
    assert_eq!(wait_pane_exit_code("", false), 1);
}
