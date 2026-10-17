use super::*;

#[test]
fn parse_window_line_when_state_present_then_read() {
    let row = parse_window_line("2:build:3:120x30:1:blocked").expect("row");
    assert_eq!(row.0, 2);
    assert_eq!(row.1, "build");
    assert!(row.4);
    assert_eq!(row.5, "blocked");
}

#[test]
fn parse_window_line_when_older_server_without_field_then_empty_state() {
    let row = parse_window_line("0:main:1:80x24:0").expect("row");
    assert_eq!(row.1, "main");
    assert_eq!(row.5, "");
}

#[test]
fn parse_window_line_when_state_outside_closed_set_then_empty() {
    let row = parse_window_line("0:main:1:80x24:0:#{window_agent_state}").expect("row");
    assert_eq!(row.5, "");
}

#[test]
fn parse_window_line_when_too_few_fields_then_none() {
    assert!(parse_window_line("0:main").is_none());
    assert!(parse_window_line("").is_none());
}
