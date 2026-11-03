// Issue #761: the server exited with the attached client after answering a
// pane query.
//
// Every console injection ends with FreeConsole and, when the process had a
// console before, AttachConsole(ATTACH_PARENT_PROCESS).  The server is started
// with CREATE_NEW_CONSOLE, so it always "had" one, and the first injection
// moved it into the console of its parent: the client that cold started it.
// Over ssh that console belongs to the channel, so closing the connection sent
// CTRL_CLOSE_EVENT to the server along with the client.
//
// The runtime proof is upstream's test_issue761_server_console_detach.ps1,
// not carried here.  These guards keep the two halves of the fix from coming
// apart: every re-attach in the injection code goes through the one helper
// that consults the server flag, and the server entry point sets that flag.

const PLATFORM: &str = include_str!("../src/platform.rs");
const SERVER: &str = include_str!("../src/server/mod.rs");

fn fn_body<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found"));
    let rest = &src[start..];
    let open = rest.find('{').expect("function has a body");
    let mut depth = 0usize;
    for (i, ch) in rest[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[..open + i + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after `{signature}`");
}

#[test]
fn every_parent_reattach_goes_through_the_server_aware_helper() {
    let direct = PLATFORM
        .matches("AttachConsole(ATTACH_PARENT_PROCESS)")
        .count();
    assert_eq!(
        direct, 1,
        "platform.rs calls AttachConsole(ATTACH_PARENT_PROCESS) {direct} times; a new \
         injection site must call reattach_parent_console() so the server stays detached (#761)"
    );
    let helper = fn_body(PLATFORM, "unsafe fn reattach_parent_console()");
    assert!(
        helper.contains("AttachConsole(ATTACH_PARENT_PROCESS)"),
        "the one direct parent re-attach must live in reattach_parent_console"
    );
    assert!(
        helper.contains("SERVER_PROCESS.load("),
        "reattach_parent_console must skip the re-attach in the server"
    );
}

#[test]
fn run_server_marks_the_process_as_the_server_before_anything_else() {
    let body = fn_body(SERVER, "pub fn run_server(");
    let flag = body
        .find("mouse_inject::SERVER_PROCESS")
        .expect("run_server must set mouse_inject::SERVER_PROCESS (#761)");
    assert!(
        body[flag..]
            .trim_start_matches("mouse_inject::SERVER_PROCESS")
            .trim_start()
            .starts_with(".store(true"),
        "run_server must store true into SERVER_PROCESS"
    );
    // It has to be set before the server can create a pane, because the first
    // injection can come from the very first pane's output.
    let pty_system = body
        .find("native_pty_system()")
        .expect("run_server opens the pty system");
    assert!(
        flag < pty_system,
        "SERVER_PROCESS must be set before any pane can exist"
    );
}
