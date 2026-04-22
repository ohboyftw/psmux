//! Tests for `SessionResolver`.
//!
//! NOTE: These tests mutate the process-wide `PSMUX_SESSION` env var.
//! Run with `cargo test -- --test-threads=1` (or `cargo test --test session
//! -- --test-threads=1`) to prevent races between tests. Do not add new
//! tests to this file without also `remove_var`ing PSMUX_SESSION at their
//! start.

use psmux_fleet::{FleetError, SessionResolver};

#[test]
fn explicit_arg_wins_over_env() {
    std::env::remove_var("PSMUX_SESSION");
    std::env::set_var("PSMUX_SESSION", "env-session");
    let r = SessionResolver::new_with_candidates(vec!["auto".into()]);
    let got = r.resolve(Some("explicit")).unwrap();
    assert_eq!(got, "explicit");
    std::env::remove_var("PSMUX_SESSION");
}

#[test]
fn env_wins_over_auto_detect() {
    std::env::remove_var("PSMUX_SESSION");
    std::env::set_var("PSMUX_SESSION", "env-session");
    let r = SessionResolver::new_with_candidates(vec!["auto-a".into(), "auto-b".into()]);
    let got = r.resolve(None).unwrap();
    assert_eq!(got, "env-session");
    std::env::remove_var("PSMUX_SESSION");
}

#[test]
fn single_candidate_auto_detects() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec!["only-one".into()]);
    let got = r.resolve(None).unwrap();
    assert_eq!(got, "only-one");
}

#[test]
fn zero_candidates_errors_not_found() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec![]);
    assert!(matches!(r.resolve(None), Err(FleetError::SessionNotFound)));
}

#[test]
fn multiple_candidates_errors_ambiguous() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec!["a".into(), "b".into()]);
    match r.resolve(None) {
        Err(FleetError::SessionAmbiguous(v)) => assert_eq!(v.len(), 2),
        other => panic!("expected SessionAmbiguous, got {other:?}"),
    }
}

#[test]
fn empty_env_falls_through_to_candidates() {
    std::env::remove_var("PSMUX_SESSION");
    std::env::set_var("PSMUX_SESSION", "");
    let r = SessionResolver::new_with_candidates(vec!["only-one".into()]);
    let got = r.resolve(None).unwrap();
    assert_eq!(got, "only-one");
    std::env::remove_var("PSMUX_SESSION");
}
