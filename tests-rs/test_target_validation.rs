// Targets-validation family (upstream 194fac0 + 772994a).
//
// `parse_target` used to return a silently-empty `ParsedTarget` for anything it
// could not parse: `%abc`, `@xyz`, `sess:nosuchwindow`. Every caller then read
// `window: None` / `pane: None` as "no target given" and ran the command against
// whatever was ACTIVE. So a typo in a `-t` did not fail, it hit a different pane
// in a different window and still exited 0.
//
// The fix is to distinguish "no target part was given" (None, legitimate — means
// current) from "a target part was given and it does not resolve" (None plus the
// unresolved flag, an error). An EMPTY part still means current, so the flag is
// only set for a non-empty part that failed to parse.

use super::*;

// ── 772994a: the `=` exact-match prefix ──────────────────────────────────

#[test]
fn equals_prefix_is_stripped_from_a_session_target() {
    // tmux spells "match this name exactly, no prefix matching" as `=name`.
    // psmux only ever matches exactly, so the marker is a no-op — but it has to
    // be REMOVED, or the session name keeps the '=' and matches nothing.
    let pt = parse_target("=sess:0");

    assert_eq!(pt.session.as_deref(), Some("sess"));
    assert_eq!(pt.window, Some(0));
    assert!(!pt.window_unresolved);
}

#[test]
fn equals_prefix_is_stripped_from_a_bare_session_target() {
    let pt = parse_target("=sess");

    assert_eq!(pt.session.as_deref(), Some("sess"));
}

// ── 194fac0: unparseable parts must be distinguishable from absent ones ──

#[test]
fn a_non_numeric_pane_id_is_marked_unresolved() {
    let pt = parse_target("%abc");

    assert_eq!(pt.pane, None);
    assert!(
        pt.pane_unresolved,
        "`%abc` names a pane that cannot exist; it must not read as 'no pane given'",
    );
}

#[test]
fn a_non_numeric_window_id_is_marked_unresolved() {
    let pt = parse_target("@xyz");

    assert_eq!(pt.window, None);
    assert!(pt.window_unresolved);
}

#[test]
fn a_named_window_is_marked_unresolved() {
    // psmux has no window-name resolution at all, so a name in the window slot
    // is not "a window we might find later" — it is unresolvable, today.
    let pt = parse_target("sess:nosuchwindow");

    assert_eq!(pt.window, None);
    assert!(
        pt.window_unresolved,
        "`sess:nosuchwindow` silently hit the ACTIVE window before this fix",
    );
}

#[test]
fn a_non_numeric_pane_in_the_pane_slot_is_marked_unresolved() {
    let pt = parse_target("sess:0.abc");

    assert_eq!(pt.window, Some(0));
    assert_eq!(pt.pane, None);
    assert!(pt.pane_unresolved);
}

#[test]
fn a_malformed_pane_id_in_the_pane_slot_is_marked_unresolved() {
    let pt = parse_target("sess:.%abc");

    assert_eq!(pt.pane, None);
    assert!(pt.pane_unresolved);
}

// ── the negative half: absent and empty parts stay legitimate ────────────

#[test]
fn a_bare_session_name_leaves_both_parts_resolved() {
    let pt = parse_target("sess");

    assert_eq!(pt.session.as_deref(), Some("sess"));
    assert!(!pt.window_unresolved, "no window part was given at all");
    assert!(!pt.pane_unresolved);
}

#[test]
fn a_session_name_containing_a_dot_is_not_a_pane_target() {
    let pt = parse_target("my.session");

    assert_eq!(pt.session.as_deref(), Some("my.session"));
    assert!(!pt.window_unresolved);
    assert!(!pt.pane_unresolved);
}

#[test]
fn an_empty_window_part_means_current_not_unresolved() {
    let pt = parse_target("sess:");

    assert_eq!(pt.window, None);
    assert!(
        !pt.window_unresolved,
        "an empty part means 'current', which is a legitimate target",
    );
}

#[test]
fn an_empty_pane_part_means_current_not_unresolved() {
    let pt = parse_target("sess:0.");

    assert_eq!(pt.window, Some(0));
    assert_eq!(pt.pane, None);
    assert!(!pt.pane_unresolved);
}

#[test]
fn well_formed_targets_are_never_marked_unresolved() {
    for t in [
        "%4",
        "@2",
        "sess:0",
        "sess:0.1",
        "sess:.%4",
        ":.1",
        "sess:1.%9",
    ] {
        let pt = parse_target(t);
        assert!(
            !pt.window_unresolved && !pt.pane_unresolved,
            "{t} is well-formed and must parse clean",
        );
    }
}
