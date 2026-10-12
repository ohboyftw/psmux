// kill-server's last-resort process sweep and the shared desktop bus. The
// sweep decides by each process's own command line, never by a discovery
// file the caller's environment names.
use super::*;

const ME: u32 = 100;

fn line(s: &str) -> impl FnOnce() -> Option<String> + '_ {
    move || Some(s.to_string())
}

#[test]
fn sweep_targets_a_psmux_server_process() {
    let server = r#""C:\bin\psmux.exe" server -s work"#;
    assert!(is_sweep_target("psmux.exe", 300, ME, line(server)));
    assert!(is_sweep_target(r"C:\bin\TMUX.EXE", 300, ME, line(server)));
}

#[test]
fn sweep_spares_a_bus_whatever_discovery_file_it_names() {
    let bus = r#""C:\Program Files\psmux\psmux.exe" bus --port 4321 --discovery C:\Users\me\.mycel\server"#;
    assert!(!is_sweep_target("psmux.exe", 300, ME, line(bus)));
    assert!(!is_sweep_target("pmux.exe", 300, ME, line("pmux bus")));
}

#[test]
fn sweep_spares_a_process_whose_command_line_cannot_be_read() {
    assert!(!is_sweep_target("psmux.exe", 300, ME, || None));
}

#[test]
fn sweep_spares_itself_and_other_programs() {
    let server = "psmux server -s work";
    assert!(!is_sweep_target("psmux.exe", ME, ME, line(server)));
    assert!(!is_sweep_target("mycel.exe", 300, ME, line("mycel start")));
    assert!(!is_sweep_target("notpsmux.exe", 300, ME, line(server)));
}

#[test]
fn bus_command_line_is_the_bus_subcommand_only() {
    assert!(crate::bus::host::is_bus_command_line(
        r#""C:\a b\psmux.exe" bus"#
    ));
    assert!(crate::bus::host::is_bus_command_line(
        "psmux.exe  bus --port 0"
    ));
    assert!(!crate::bus::host::is_bus_command_line(
        "psmux server -s bus"
    ));
    assert!(!crate::bus::host::is_bus_command_line(
        r#""C:\bus\psmux.exe" new-session"#
    ));
    assert!(!crate::bus::host::is_bus_command_line("psmux"));
}

#[test]
fn process_command_line_reads_this_process() {
    let cmd = crate::warm_reap::process_command_line(std::process::id()).unwrap();
    assert!(cmd.contains("psmux-"), "command line was {cmd}");
}
