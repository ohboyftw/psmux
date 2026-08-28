//! An explicitly quoted empty argument has to survive tokenisation (#535).
//!
//! `set -g @foo ""` is how tmux clears an option. The parser dropped the empty
//! token along with the whitespace runs, so the server received a name with no
//! value, took the silent no-value branch, and kept the old value at exit 0.

use crate::commands::parse_command_line;

#[test]
fn a_quoted_empty_argument_survives_as_its_own_token() {
    assert_eq!(
        parse_command_line("set -g @foo \"\""),
        vec!["set", "-g", "@foo", ""]
    );
}

#[test]
fn a_single_quoted_empty_argument_survives_too() {
    assert_eq!(
        parse_command_line("set -g @foo ''"),
        vec!["set", "-g", "@foo", ""]
    );
}

#[test]
fn an_omitted_value_stays_omitted() {
    // The complement of the case above: no quotes means no token, so the
    // no-value error path still sees one positional rather than two.
    assert_eq!(parse_command_line("set -g @foo"), vec!["set", "-g", "@foo"]);
}

#[test]
fn trailing_whitespace_does_not_invent_an_empty_token() {
    assert_eq!(
        parse_command_line("set -g @foo bar   "),
        vec!["set", "-g", "@foo", "bar"]
    );
}

#[test]
fn a_quoted_value_with_spaces_is_still_one_token() {
    assert_eq!(
        parse_command_line("set -g @foo \"a b\""),
        vec!["set", "-g", "@foo", "a b"]
    );
}
