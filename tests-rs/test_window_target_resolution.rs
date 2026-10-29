// psmux/psmux#627: window targets resolved to the wrong window.
//
// Measured on ohboy-builds 727a8ab5 with windows @1 "w1" (index 0) and
// @2 "w2" (index 1) in session alpha:
//
//     display-message -t @2          -> alpha/@1/w1   exit 0  (wrong window)
//     display-message -t alpha:@2    -> alpha/@1/w1   exit 0  (wrong window)
//     display-message -t alpha:w2    -> can't find window, exit 1
//     display-message -t alpha:@2.%N -> can't find window, exit 1
//     split-window -d -t alpha:w2    -> split w1, exit 0
//
// Two faults. parse_target set `window_is_id` and nothing read it, so the
// server took @2 as window INDEX 2. With two windows that index does not exist,
// FocusWindowTemp ignored it, and the command ran on the active window. And a
// window name, or an id followed by `.pane`, was marked unresolved because
// psmux had no name lookup and the `@` branch never looked for a pane slot.
//
// Now parse_target hands the server a WindowSpec (index, id or name), and the
// server resolves it against its real windows. A spec that matches nothing is
// an error.

use super::*;
use crate::types::WindowSpec;

// ── parse_target: every window form survives parsing ────────────────────

#[test]
fn a_bare_window_id_is_an_id_not_an_index() {
    let pt = parse_target("@2");

    assert_eq!(pt.window_spec(), Some(WindowSpec::Id(2)));
}

#[test]
fn a_session_qualified_window_id_is_an_id_not_an_index() {
    let pt = parse_target("alpha:@2");

    assert_eq!(pt.session.as_deref(), Some("alpha"));
    assert_eq!(pt.window_spec(), Some(WindowSpec::Id(2)));
}

#[test]
fn a_window_id_followed_by_a_pane_id_parses_both() {
    let pt = parse_target("alpha:@2.%15020002");

    assert_eq!(pt.session.as_deref(), Some("alpha"));
    assert_eq!(pt.window_spec(), Some(WindowSpec::Id(2)));
    assert_eq!(pt.pane, Some(15020002));
    assert!(pt.pane_is_id);
    assert!(!pt.window_unresolved && !pt.pane_unresolved);
}

#[test]
fn a_bare_window_id_followed_by_a_pane_index_parses_both() {
    let pt = parse_target("@2.1");

    assert_eq!(pt.session, None);
    assert_eq!(pt.window_spec(), Some(WindowSpec::Id(2)));
    assert_eq!(pt.pane, Some(1));
    assert!(!pt.pane_is_id);
}

#[test]
fn a_window_name_is_kept_for_the_server_to_resolve() {
    let pt = parse_target("alpha:w2");

    assert_eq!(pt.session.as_deref(), Some("alpha"));
    assert_eq!(pt.window_spec(), Some(WindowSpec::Name("w2".to_string())));
    assert!(
        !pt.window_unresolved,
        "only the server knows its window names, so a name is not a parse error",
    );
}

#[test]
fn a_window_name_followed_by_a_pane_parses_both() {
    let pt = parse_target("alpha:w2.%7");

    assert_eq!(pt.window_spec(), Some(WindowSpec::Name("w2".to_string())));
    assert_eq!(pt.pane, Some(7));
    assert!(pt.pane_is_id);
}

#[test]
fn a_window_index_stays_an_index() {
    let pt = parse_target("alpha:1");

    assert_eq!(pt.window_spec(), Some(WindowSpec::Index(1)));
}

#[test]
fn no_window_part_means_no_window_spec() {
    for t in ["alpha", "alpha:", "%4", "alpha:.1", ""] {
        assert_eq!(parse_target(t).window_spec(), None, "{t:?}");
    }
}

#[test]
fn an_overflowing_numeric_window_is_unresolved_not_a_name() {
    let pt = parse_target("sess:9999999999999999999999");

    assert_eq!(pt.window_spec(), None);
    assert!(pt.window_unresolved);
}

#[test]
fn a_malformed_window_id_is_still_unresolved() {
    for t in ["@xyz", "@", "alpha:@", "alpha:@x.1", "@-1"] {
        let pt = parse_target(t);
        assert!(pt.window_unresolved, "{t:?} must stay unresolved");
        assert_eq!(pt.window_spec(), None, "{t:?}");
    }
}

// ── WindowSpec::resolve: the server-side lookup ─────────────────────────

/// Windows as (id, name), in index order — the shape the server passes in.
const WINDOWS: [(usize, &str); 3] = [(1, "w1"), (2, "w2"), (5, "logs")];

#[test]
fn an_id_resolves_to_the_window_that_has_it() {
    // @2 is the window at internal index 1, not internal index 2.
    assert_eq!(WindowSpec::Id(2).resolve(WINDOWS, 0), Some(1));
    assert_eq!(WindowSpec::Id(5).resolve(WINDOWS, 0), Some(2));
}

#[test]
fn an_id_ignores_base_index() {
    assert_eq!(WindowSpec::Id(2).resolve(WINDOWS, 1), Some(1));
}

#[test]
fn an_unknown_id_resolves_to_nothing() {
    assert_eq!(WindowSpec::Id(3).resolve(WINDOWS, 0), None);
}

#[test]
fn a_name_resolves_to_the_window_that_has_it() {
    assert_eq!(
        WindowSpec::Name("w2".to_string()).resolve(WINDOWS, 0),
        Some(1)
    );
}

#[test]
fn an_unknown_name_resolves_to_nothing() {
    assert_eq!(WindowSpec::Name("w9".to_string()).resolve(WINDOWS, 0), None);
}

#[test]
fn a_name_is_matched_exactly() {
    // No prefix matching: "w" must not pick w1 or w2.
    assert_eq!(WindowSpec::Name("w".to_string()).resolve(WINDOWS, 0), None);
}

#[test]
fn a_name_shared_by_two_windows_is_ambiguous() {
    // tmux refuses this too; picking one would act on a window by luck.
    let dup = [(1, "sh"), (2, "sh")];
    assert_eq!(WindowSpec::Name("sh".to_string()).resolve(dup, 0), None);
}

#[test]
fn an_index_honours_base_index() {
    assert_eq!(WindowSpec::Index(0).resolve(WINDOWS, 0), Some(0));
    assert_eq!(WindowSpec::Index(1).resolve(WINDOWS, 1), Some(0));
    assert_eq!(WindowSpec::Index(3).resolve(WINDOWS, 1), Some(2));
}

#[test]
fn an_index_outside_the_window_list_resolves_to_nothing() {
    assert_eq!(WindowSpec::Index(3).resolve(WINDOWS, 0), None);
    // Below base-index.
    assert_eq!(WindowSpec::Index(0).resolve(WINDOWS, 1), None);
}
