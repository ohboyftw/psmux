//! `quote_windows_arg` must follow the CommandLineToArgvW / CRT rules, so the
//! server reads back exactly the argument `new-session` passed. Included from
//! `src/detached.rs`.

use super::*;

#[test]
fn quote_when_plain_or_unc_or_drive_root_then_unchanged() {
    for arg in [r"C:\", r"\\server\share\dir", "-n", "w"] {
        assert_eq!(quote_windows_arg(arg), arg);
    }
}

#[test]
fn quote_when_space_and_trailing_backslash_then_backslash_doubled_before_closing_quote() {
    assert_eq!(
        quote_windows_arg(r"C:\Program Files\x\"),
        r#""C:\Program Files\x\\""#
    );
    assert_eq!(
        quote_windows_arg(r"\\server\share dir\"),
        r#""\\server\share dir\\""#
    );
}

#[test]
fn quote_when_backslashes_before_a_quote_then_doubled_and_quote_escaped() {
    assert_eq!(quote_windows_arg(r#"a\"b"#), r#""a\\\"b""#);
    assert_eq!(quote_windows_arg(r#"a"b"#), r#""a\"b""#);
}

#[test]
fn quote_when_empty_then_empty_quotes() {
    assert_eq!(quote_windows_arg(""), r#""""#);
}

#[test]
fn command_line_when_cwd_ends_in_backslash_then_next_flag_is_its_own_argument() {
    let args = [
        "-d".to_string(),
        r"C:\a b\".to_string(),
        "-n".to_string(),
        "w".to_string(),
    ];
    let line: Vec<String> = args.iter().map(|a| quote_windows_arg(a)).collect();
    assert_eq!(line.join(" "), r#"-d "C:\a b\\" -n w"#);
}
