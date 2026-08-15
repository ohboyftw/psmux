// dc183e3 (upstream): a %id in the pane slot of "session:window.pane" was parsed
// with a bare parse::<usize>(), which fails on the '%'. The pane component came
// back None and every caller fell through to the ACTIVE pane — so the command did
// not fail, it acted on the WRONG pane, and still exited 0:
//
//     panes 0:%1 0:%2 1:%3 1:%4, active %2
//     psmux kill-pane -t wt:.%4   ->  exit 0, and %2 died. %4 survived.
//
// This matters more here than upstream: after pane ids collided and destroyed two
// live sessions (3b715f1), the standing guidance became "always use
// session-qualified targets -t <session>:<window>.<pane>, never a bare %N". That
// recommended-safe form is exactly the one with this hole.

use super::*;

#[test]
fn pane_id_in_the_pane_slot_is_parsed_as_a_pane_id() {
    let pt = parse_target("sess:0.%4");

    assert_eq!(pt.session.as_deref(), Some("sess"));
    assert_eq!(pt.window, Some(0));
    assert_eq!(
        pt.pane,
        Some(4),
        "a %id in the pane slot must resolve to that pane, not fall through to the active one",
    );
    assert!(pt.pane_is_id, "%4 names a pane by id, not by index");
}

#[test]
fn pane_id_in_the_pane_slot_without_a_window_is_parsed() {
    // The "sess:.%id" form — tmux accepts it, and it is the form the kill-pane
    // repro above used.
    let pt = parse_target("sess:.%4");

    assert_eq!(pt.session.as_deref(), Some("sess"));
    assert_eq!(pt.pane, Some(4));
    assert!(pt.pane_is_id, "%4 names a pane by id, not by index");
}

#[test]
fn numeric_index_in_the_pane_slot_stays_an_index() {
    // Guard on the fix, not on the bug: if the pane slot started treating every
    // value as an id, "sess:0.1" would mean pane %1 instead of the second pane
    // in window 0. Upstream pins this case for the same reason.
    let pt = parse_target("sess:0.1");

    assert_eq!(pt.window, Some(0));
    assert_eq!(pt.pane, Some(1));
    assert!(
        !pt.pane_is_id,
        "a bare number in the pane slot is an index, not a pane id",
    );
}

#[test]
fn bare_pane_id_target_is_unchanged() {
    // Regression guard for the path that already worked.
    let pt = parse_target("%4");

    assert_eq!(pt.pane, Some(4));
    assert!(pt.pane_is_id);
    assert_eq!(pt.session, None);
}
