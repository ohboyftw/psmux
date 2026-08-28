// `-U` is tmux's unset alias for `-u`, and the config parser tested for it with
// a case-sensitive `contains('u')`. So `set -gU @x` in a config file did not
// unset: it fell through to the plain set path and wrote an EMPTY value, which
// reads identically through `show-options -v` and differs only in whether the
// option is still marked as user-set.

use super::*;
use crate::types::AppState;

fn mock_app() -> AppState {
    AppState::new("test_session".to_string())
}

#[test]
fn lowercase_u_in_a_cluster_unsets() {
    let mut app = mock_app();
    parse_set_option(&mut app, "set -g @x hello");
    assert!(app.user_set_options.contains("@x"));
    parse_set_option(&mut app, "set -gu @x");
    assert!(!app.user_set_options.contains("@x"));
}

#[test]
fn uppercase_u_is_an_unset_alias() {
    let mut app = mock_app();
    parse_set_option(&mut app, "set -g @x hello");
    parse_set_option(&mut app, "set -gU @x");
    assert!(
        !app.user_set_options.contains("@x"),
        "-U must unset, not write an empty value"
    );
}

#[test]
fn uppercase_u_ignores_a_trailing_value() {
    // tmux unsets and discards the operand. Falling through to the set path
    // instead WROTE it — the caller asked for an unset and got an assignment.
    let mut app = mock_app();
    parse_set_option(&mut app, "set -g @x hello");
    parse_set_option(&mut app, "set -gU @x XX");
    assert!(!app.user_set_options.contains("@x"));
    assert_ne!(app.user_options.get("@x").map(String::as_str), Some("XX"));
}

#[test]
fn an_ordinary_set_is_still_recorded_as_user_set() {
    let mut app = mock_app();
    parse_set_option(&mut app, "set -g @x hello");
    assert_eq!(app.user_options.get("@x").map(String::as_str), Some("hello"));
    assert!(app.user_set_options.contains("@x"));
}
