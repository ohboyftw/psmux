// A claimed warm standby announces its real name (psmux/session/renamed
// from `__warm__`), the same way rename-session does.
use super::*;
use crate::control::{ControlClient, ControlMode};
use std::sync::mpsc;

fn app_with_client(name: &str) -> (AppState, mpsc::Receiver<String>) {
    let (tx, rx) = mpsc::channel();
    let mut app = AppState::new(name.to_string());
    app.control_clients
        .push(ControlClient::new(1, ControlMode::NoEcho, tx));
    (app, rx)
}

#[test]
fn renaming_a_warm_standby_announces_the_new_name() {
    let (mut app, rx) = app_with_client("__warm__");
    let old = rename_server_session(&mut app, "work".into());
    assert_eq!(
        (old.as_str(), app.session_name.as_str()),
        ("__warm__", "work")
    );
    assert_eq!(rx.try_recv().unwrap(), "%sessions-changed");
    assert!(rx.try_recv().is_err());
}

/// The arm of `CtrlReq` named `req` in the server loop's source.
fn ctrl_arm(req: &str) -> &'static str {
    let src = include_str!("../src/server/mod.rs");
    let start = src.find(&format!("CtrlReq::{req}(")).unwrap();
    let body = &src[start + 1..];
    &body[..body.find("CtrlReq::").unwrap()]
}

#[test]
fn claim_session_renames_through_the_announcing_helper() {
    assert!(ctrl_arm("ClaimSession").contains("rename_server_session(&mut app"));
}

#[test]
fn rename_session_renames_through_the_announcing_helper() {
    assert!(ctrl_arm("RenameSession").contains("rename_server_session(&mut app"));
}
