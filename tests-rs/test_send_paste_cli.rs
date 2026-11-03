use super::build_send_paste_control;

#[test]
fn send_paste_cli_forwards_one_base64_frame() {
    let args = vec!["send-paste", "IGxpbmUgb25lXG5saW5lIHR3byA="];

    assert_eq!(
        build_send_paste_control(&args).unwrap(),
        "send-paste IGxpbmUgb25lXG5saW5lIHR3byA=\n",
    );
}

#[test]
fn send_paste_cli_rejects_missing_or_non_base64_payload() {
    assert!(build_send_paste_control(&["send-paste"]).is_err());
    assert!(build_send_paste_control(&["send-paste", "not base64"]).is_err());
    assert!(build_send_paste_control(&["send-paste", "YQ==", "Yg=="]).is_err());
    assert!(build_send_paste_control(&["send-paste", "-t"]).is_err());
}

#[test]
fn send_paste_cli_preserves_optional_target_syntax() {
    assert_eq!(
        build_send_paste_control(&["send-paste", "-t", "dev:0.1", "YQ=="]).unwrap(),
        "send-paste YQ==\n",
    );
}
