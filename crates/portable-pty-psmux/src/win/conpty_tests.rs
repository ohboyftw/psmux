// These tests start no psmux server.  The tree test spawns `cmd /c ping`
// through a real ConPTY, exactly as a pane is spawned, and watches the cmd
// and its PING by pid.  It kills whatever it started before asserting.

use super::*;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::time::{Duration, Instant};

#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; 260],
}

extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
    fn Process32FirstW(snap: isize, pe: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snap: isize, pe: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    fn TerminateProcess(h: isize, code: u32) -> i32;
    fn GetHandleInformation(h: isize, flags: *mut u32) -> i32;
}

const SYNCHRONIZE_AND_TERMINATE: u32 = 0x0010_0000 | 0x0001;
const WAIT_TIMEOUT: u32 = 0x102;

fn child_named(parent: u32, name: &str) -> Option<u32> {
    // SAFETY: a TH32CS_SNAPPROCESS snapshot, walked with a correctly sized
    // entry and closed before returning.
    unsafe {
        let snap = CreateToolhelp32Snapshot(0x2, 0);
        if snap == -1 || snap == 0 {
            return None;
        }
        let mut pe: ProcessEntry32W = std::mem::zeroed();
        pe.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut pe);
        while ok != 0 && found.is_none() {
            let len = pe.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
            let exe = String::from_utf16_lossy(&pe.sz_exe_file[..len]);
            if pe.th32_parent_process_id == parent && exe.eq_ignore_ascii_case(name) {
                found = Some(pe.th32_process_id);
            }
            ok = Process32NextW(snap, &mut pe);
        }
        CloseHandle(snap);
        found
    }
}

fn is_alive(pid: u32) -> bool {
    // SAFETY: the handle is checked, used, and closed here.
    unsafe {
        let h = OpenProcess(SYNCHRONIZE_AND_TERMINATE, 0, pid);
        if h == 0 {
            return false;
        }
        let running = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
        CloseHandle(h);
        running
    }
}

fn kill(pid: u32) {
    // SAFETY: the handle is checked, used, and closed here.
    unsafe {
        let h = OpenProcess(SYNCHRONIZE_AND_TERMINATE, 0, pid);
        if h != 0 {
            TerminateProcess(h, 1);
            WaitForSingleObject(h, 2000);
            CloseHandle(h);
        }
    }
}

fn wait_until(ms: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

fn inherit_flag(h: std::os::windows::io::RawHandle) -> u32 {
    let mut flags = 0u32;
    // SAFETY: `h` is an open handle owned by the caller.
    assert_ne!(unsafe { GetHandleInformation(h as isize, &mut flags) }, 0);
    flags & 0x1
}

#[test]
fn pipe_ends_are_created_without_the_inherit_flag() {
    let (read, write) = create_pipe_with_buffer(64 * 1024).expect("pipe");
    assert_eq!(inherit_flag(read.as_raw_handle()), 0);
    assert_eq!(inherit_flag(write.as_raw_handle()), 0);
}

#[test]
fn dropping_the_child_ends_its_process_tree_while_the_pty_stays_open() {
    let pair = ConPtySystem::default()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.args(["/c", "ping -n 30 127.0.0.1"]);
    let child = pair.slave.spawn_command(cmd).expect("spawn cmd");
    // The host asks for the cursor position before it runs the client
    // (INHERIT_CURSOR) and blocks once its output pipe fills: answer the
    // query and drain the output, as a pane's reader thread does.
    let mut writer = pair.master.take_writer().expect("writer");
    writer.write_all(b"\x1b[1;1R").expect("cursor reply");
    let mut reader = pair.master.try_clone_reader().expect("reader");
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let cmd_pid = child.process_id().expect("cmd pid");
    let mut ping_pid = 0;
    let started = wait_until(5000, || {
        ping_pid = child_named(cmd_pid, "PING.EXE").unwrap_or(0);
        ping_pid != 0
    });
    drop(child);
    let gone = started && wait_until(3000, || !is_alive(cmd_pid) && !is_alive(ping_pid));
    kill(cmd_pid);
    if ping_pid != 0 {
        kill(ping_pid);
    }
    drop(writer);
    drop(pair);
    assert!(started, "cmd never started its ping");
    assert!(
        gone,
        "cmd {} or ping {} outlived the dropped child",
        cmd_pid, ping_pid
    );
}
