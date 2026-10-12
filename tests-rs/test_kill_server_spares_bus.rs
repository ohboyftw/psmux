// kill-server's last-resort process sweep and the shared desktop bus.
use super::*;

const ME: u32 = 100;
const BUS: u32 = 200;

#[test]
fn sweep_targets_a_psmux_server_process() {
    assert!(is_sweep_target("psmux.exe", 300, ME, Some(BUS)));
    assert!(is_sweep_target(r"C:\bin\TMUX.EXE", 300, ME, None));
}

#[test]
fn sweep_spares_the_shared_bus() {
    assert!(!is_sweep_target("psmux.exe", BUS, ME, Some(BUS)));
}

#[test]
fn sweep_spares_itself_and_other_programs() {
    assert!(!is_sweep_target("psmux.exe", ME, ME, None));
    assert!(!is_sweep_target("mycel.exe", 300, ME, None));
    assert!(!is_sweep_target("notpsmux.exe", 300, ME, None));
}
