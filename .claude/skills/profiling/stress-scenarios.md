# psmux Stress Scenarios & Failure Mode Triggers

## Table of Contents
1. [Memory Leak Scenarios](#memory-leak-scenarios)
2. [CPU Runoff Scenarios](#cpu-runoff-scenarios)
3. [Handle/Resource Leak Scenarios](#handle-resource-leak-scenarios)
4. [Concurrency Stress Scenarios](#concurrency-stress-scenarios)
5. [Recovery & Cleanup Validation](#recovery-cleanup-validation)

---

## Memory Leak Scenarios

### ML-1: Pane Reader Buffer Accumulation
**What leaks:** Each pane spawns a reader thread with an 8KB buffer. If panes are created
but their reader threads aren't properly joined on close, buffers accumulate.

**How to trigger:**
```rust
/// Rapidly create and destroy panes without waiting for cleanup
fn trigger_reader_buffer_leak(iterations: usize) {
    for i in 0..iterations {
        // Create a session with multiple panes
        psmux_cmd(&["new-session", "-d", "-s", &format!("leak-{i}")]);
        for _ in 0..10 {
            psmux_cmd(&["split-window", "-t", &format!("leak-{i}")]);
        }
        // Kill immediately — race condition on thread cleanup
        psmux_cmd(&["kill-session", "-t", &format!("leak-{i}")]);
    }
    // Measure: RSS should return to baseline after all sessions killed
}
```

**Detection:** Compare RSS before and after 1000 iterations. Leak = RSS growth > 5MB
above baseline after all sessions destroyed and a 5-second cooldown.

### ML-2: VT100 Parser Screen Buffer Growth
**What leaks:** The `vt100-psmux` crate maintains a screen buffer per pane. If a process
floods the terminal with output (e.g., `yes` command, `cat /dev/urandom`), the parser
may accumulate history beyond the scrollback limit.

**How to trigger:**
```rust
/// Flood a pane with output and check if buffer is bounded
fn trigger_vt100_buffer_flood() {
    psmux_cmd(&["new-session", "-d", "-s", "flood"]);
    // Send a command that generates infinite output
    psmux_cmd(&["send-keys", "-t", "flood", "cmd /c \"for /L %i in (1,1,999999) do @echo ########################################\"", "Enter"]);
    // Let it run for 30 seconds
    std::thread::sleep(Duration::from_secs(30));
    // Sample RSS every second during the flood
    let rss_samples = sample_rss_over(Duration::from_secs(30), Duration::from_secs(1));
    // Kill
    psmux_cmd(&["kill-session", "-t", "flood"]);
    // Assert: RSS growth rate should plateau (bounded buffer)
    assert_rss_plateaus(&rss_samples, max_growth_rate_kb_per_sec: 100);
}
```

### ML-3: Session Metadata Accumulation
**What leaks:** Session/window/pane metadata stored in the server. If `kill-session`
doesn't fully clean up internal data structures, metadata accumulates.

**How to trigger:**
```rust
fn trigger_metadata_leak(cycles: usize) {
    let baseline_rss = get_rss_kb();
    for cycle in 0..cycles {
        // Create 5 sessions, each with 3 windows, each with 2 panes
        for s in 0..5 {
            let name = format!("meta-{cycle}-{s}");
            psmux_cmd(&["new-session", "-d", "-s", &name]);
            psmux_cmd(&["new-window", "-t", &name]);
            psmux_cmd(&["new-window", "-t", &name]);
            psmux_cmd(&["split-window", "-t", &format!("{name}:0")]);
            psmux_cmd(&["split-window", "-t", &format!("{name}:1")]);
            psmux_cmd(&["split-window", "-t", &format!("{name}:2")]);
        }
        // Destroy everything
        for s in 0..5 {
            psmux_cmd(&["kill-session", "-t", &format!("meta-{cycle}-{s}")]);
        }
    }
    std::thread::sleep(Duration::from_secs(5)); // cooldown
    let final_rss = get_rss_kb();
    let leak_kb = final_rss.saturating_sub(baseline_rss);
    assert!(leak_kb < 2048, "Metadata leak: {leak_kb}KB retained after {cycles} cycles");
}
```

### ML-4: Format Variable String Leak
**What leaks:** The format engine interpolates 126+ variables into status bar strings.
If intermediate `String` allocations during formatting aren't dropped, they leak.

**How to trigger:**
```rust
fn trigger_format_string_leak() {
    psmux_cmd(&["new-session", "-d", "-s", "fmt"]);
    // Set an absurdly complex status line that exercises all format variables
    let complex_format = "#{session_name} #{window_index}/#{window_name} \
        #{pane_index} #{pane_current_command} #{pane_width}x#{pane_height} \
        #{client_termtype} #{host} #{pid} #{window_flags} #{pane_title}";
    psmux_cmd(&["set-option", "-g", "status-right", complex_format]);
    psmux_cmd(&["set-option", "-g", "status-interval", "1"]);
    // Let it render 300+ status bar updates
    std::thread::sleep(Duration::from_secs(300));
    // Check RSS stability over last 60 seconds
}
```

---

## CPU Runoff Scenarios

### CPU-1: Regex Backtracking in Format Engine
**What spins:** The format engine uses `regex` crate for pattern matching. Pathological
input can cause exponential backtracking.

**How to trigger:**
```rust
fn trigger_regex_backtracking() {
    psmux_cmd(&["new-session", "-d", "-s", "regex"]);
    // Set window name to a pathological regex input
    // This targets the format engine's variable substitution
    let evil_name = "a".repeat(100) + "!";
    psmux_cmd(&["rename-window", "-t", "regex", &evil_name]);
    // Set status format that triggers regex processing on every render
    psmux_cmd(&["set-option", "-g", "status-right", "#{=100:window_name}"]);
    // Monitor CPU for 10 seconds
    let cpu_samples = sample_cpu_over(Duration::from_secs(10), Duration::from_millis(500));
    assert!(cpu_samples.iter().all(|&c| c < 50.0), "CPU runoff in format engine");
}
```

### CPU-2: Render Loop Saturation (TUI Busy-Loop)
**What spins:** The ratatui render loop redraws on every input event. A flood of
mouse/keyboard events can saturate the render thread.

**How to trigger:**
```rust
fn trigger_render_saturation() {
    psmux_cmd(&["new-session", "-d", "-s", "render"]);
    // Simulate rapid mouse movement events (if mouse mode enabled)
    for _ in 0..10000 {
        psmux_cmd(&["send-keys", "-t", "render", "-M", "MouseMove", "10", "10"]);
    }
    // Or: rapidly resize terminal
    for size in (20..200).chain((20..200).rev()) {
        // Trigger resize events via the API
        psmux_cmd(&["refresh-client", "-C", &format!("{size},{size}")]);
    }
}
```

### CPU-3: Session Discovery Polling Spin
**What spins:** Client-server discovery uses 10ms polling. If the server port file is
malformed or the server is slow to respond, the client can spin in a tight poll loop.

**How to trigger:**
```rust
fn trigger_discovery_spin() {
    // Start server
    psmux_cmd(&["new-session", "-d", "-s", "spin"]);
    // Corrupt the discovery port file
    let port_file = psmux_port_file_path();
    std::fs::write(&port_file, "not-a-port-number").unwrap();
    // Try to attach — client should timeout, not spin
    let start = Instant::now();
    let result = psmux_cmd_timeout(&["attach", "-t", "spin"], Duration::from_secs(5));
    let elapsed = start.elapsed();
    // If it took exactly 5s (timeout) with low CPU, good
    // If it consumed >200% CPU during those 5s, bad
    assert!(result.cpu_percent < 200.0, "Discovery polling spin detected");
}
```

### CPU-4: Concurrent Output Flood (All Panes Active)
**What spins:** When all panes are dumping output simultaneously, reader threads
compete for the render mutex, causing contention and CPU waste.

**How to trigger:**
```rust
fn trigger_concurrent_flood() {
    psmux_cmd(&["new-session", "-d", "-s", "flood"]);
    // Create 18 panes (documented stress test limit)
    for _ in 0..17 {
        psmux_cmd(&["split-window", "-t", "flood"]);
        psmux_cmd(&["select-layout", "-t", "flood", "tiled"]);
    }
    // Start output flood in ALL panes simultaneously
    for i in 0..18 {
        psmux_cmd(&["send-keys", "-t", &format!("flood:.{i}"),
            "cmd /c \"for /L %i in (1,1,99999) do @echo FLOOD\"", "Enter"]);
    }
    // Monitor CPU and throughput for 30 seconds
    let metrics = monitor_system(Duration::from_secs(30));
    // CPU should stay under 400% (4 cores) for an 18-pane flood
    assert!(metrics.avg_cpu < 400.0);
    // System should remain responsive (can still create new sessions)
    let responsive = psmux_cmd_timeout(&["new-session", "-d", "-s", "probe"],
        Duration::from_secs(2));
    assert!(responsive.success, "System unresponsive during flood");
}
```

### CPU-5: Infinite Hook Chain
**What spins:** psmux supports hooks (after-new-window, etc.). A hook that triggers
itself creates infinite recursion.

**How to trigger:**
```rust
fn trigger_hook_recursion() {
    psmux_cmd(&["new-session", "-d", "-s", "hooks"]);
    // Set a hook that creates a window, which fires the hook again
    psmux_cmd(&["set-hook", "-g", "after-new-window",
        "run-shell 'psmux new-window -t hooks'"]);
    // Now create a window — should trigger infinite chain
    // psmux SHOULD have recursion protection
    let start = Instant::now();
    psmux_cmd(&["new-window", "-t", "hooks"]);
    std::thread::sleep(Duration::from_secs(5));
    // Check: did it create infinite windows or did protection kick in?
    let window_count = psmux_cmd(&["list-windows", "-t", "hooks"])
        .stdout.lines().count();
    assert!(window_count < 50, "Hook recursion protection failed: {window_count} windows");
}
```

---

## Handle/Resource Leak Scenarios

### HL-1: ConPTY Handle Leak on Pane Close
**What leaks:** Each pane creates ConPTY handles via `portable-pty-psmux`. If handles
aren't closed on pane destruction, the process accumulates OS handles.

**How to trigger:**
```rust
fn trigger_conpty_handle_leak(iterations: usize) {
    let baseline_handles = get_handle_count();
    for _ in 0..iterations {
        psmux_cmd(&["new-session", "-d", "-s", "hl"]);
        psmux_cmd(&["split-window", "-t", "hl"]);
        psmux_cmd(&["split-window", "-t", "hl"]);
        std::thread::sleep(Duration::from_millis(100)); // let ConPTY initialize
        psmux_cmd(&["kill-session", "-t", "hl"]);
        std::thread::sleep(Duration::from_millis(200)); // let handles close
    }
    let final_handles = get_handle_count();
    let leaked = final_handles.saturating_sub(baseline_handles);
    assert!(leaked < 10, "ConPTY handle leak: {leaked} handles after {iterations} cycles");
}

#[cfg(windows)]
fn get_handle_count() -> u32 {
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::Foundation::*;
    unsafe {
        let process = GetCurrentProcess();
        let mut count: u32 = 0;
        GetProcessHandleCount(process, &mut count);
        count
    }
}
```

### HL-2: Orphaned Child Processes
**What leaks:** Each pane spawns a shell process. If psmux crashes or `kill-session`
doesn't terminate children, orphaned shell processes accumulate.

**How to trigger:**
```powershell
# PowerShell test script
$before = (Get-Process powershell -ErrorAction SilentlyContinue).Count
# Create and destroy 50 sessions
1..50 | ForEach-Object {
    psmux new-session -d -s "orphan-$_"
    psmux kill-session -t "orphan-$_"
}
Start-Sleep 5
$after = (Get-Process powershell -ErrorAction SilentlyContinue).Count
$orphans = $after - $before
if ($orphans -gt 2) { Write-Error "ORPHAN LEAK: $orphans extra shell processes" }
```

### HL-3: Socket/Port Exhaustion
**What leaks:** psmux server listens on a TCP port. Rapid server start/stop cycles
can exhaust ephemeral ports if sockets linger in TIME_WAIT.

**How to trigger:**
```rust
fn trigger_port_exhaustion() {
    for i in 0..100 {
        psmux_cmd(&["new-session", "-d", "-s", &format!("port-{i}")]);
        psmux_cmd(&["kill-server"]);
        std::thread::sleep(Duration::from_millis(50));
    }
    // Verify we can still start a server
    let result = psmux_cmd_timeout(&["new-session", "-d", "-s", "final"],
        Duration::from_secs(5));
    assert!(result.success, "Port exhaustion: cannot start server after 100 cycles");
}
```

---

## Concurrency Stress Scenarios

### CS-1: Parallel Session Blitz
**What breaks:** Creating many sessions in parallel can expose race conditions in
server initialization, port file writing, and session registry.

**How to trigger:**
```rust
fn trigger_parallel_session_blitz() {
    let handles: Vec<_> = (0..20).map(|i| {
        std::thread::spawn(move || {
            psmux_cmd(&["new-session", "-d", "-s", &format!("blitz-{i}")])
        })
    }).collect();
    
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let successes = results.iter().filter(|r| r.success).count();
    
    // All 20 should succeed
    assert!(successes >= 18, "Parallel blitz: only {successes}/20 sessions created");
    
    // Verify all are listable
    let listed = psmux_cmd(&["list-sessions"]).stdout.lines().count();
    assert_eq!(listed, successes);
    
    // Clean up
    psmux_cmd(&["kill-server"]);
}
```

### CS-2: Rapid Attach/Detach Cycling
**What breaks:** Client attachment involves socket handshake + terminal takeover.
Rapid cycling can expose races in client state management.

**How to trigger:**
```rust
fn trigger_attach_detach_cycling() {
    psmux_cmd(&["new-session", "-d", "-s", "cycle"]);
    for _ in 0..100 {
        // Attach and immediately detach
        let child = Command::new("psmux")
            .args(&["attach", "-t", "cycle"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // Send Ctrl-B d to detach
        // Or kill the client process
        child.kill().ok();
        std::thread::sleep(Duration::from_millis(50));
    }
    // Session should still be alive and functional
    let alive = psmux_cmd(&["has-session", "-t", "cycle"]);
    assert!(alive.success, "Session died after 100 attach/detach cycles");
}
```

---

## Recovery & Cleanup Validation

### RV-1: Post-Stress Baseline Recovery
After running ANY stress test, validate that the system returns to baseline:

```rust
fn validate_recovery(pre_stress_rss: u64, pre_stress_handles: u32) {
    // Kill everything
    psmux_cmd(&["kill-server"]);
    std::thread::sleep(Duration::from_secs(5));
    
    // Check no psmux processes remain
    assert!(find_process("psmux").is_none(), "Zombie psmux process found");
    
    // Start fresh and measure
    psmux_cmd(&["new-session", "-d", "-s", "recovery"]);
    let post_rss = get_rss_kb();
    let post_handles = get_handle_count();
    psmux_cmd(&["kill-server"]);
    
    let rss_drift = post_rss.saturating_sub(pre_stress_rss);
    let handle_drift = post_handles.saturating_sub(pre_stress_handles);
    
    assert!(rss_drift < 5120, "RSS didn't recover: +{rss_drift}KB");
    assert!(handle_drift < 5, "Handle count didn't recover: +{handle_drift}");
}
```

### RV-2: Graceful Degradation Under Load
psmux should degrade gracefully, not crash, under extreme load:

```rust
fn validate_graceful_degradation() {
    psmux_cmd(&["new-session", "-d", "-s", "degrade"]);
    // Try to create 100 panes (way beyond normal use)
    let mut created = 0;
    for _ in 0..100 {
        let result = psmux_cmd_timeout(
            &["split-window", "-t", "degrade"],
            Duration::from_secs(2)
        );
        if result.success { created += 1; } else { break; }
    }
    // Should create a reasonable number before refusing
    assert!(created >= 15, "Couldn't create minimum 15 panes: only {created}");
    // Session should still be alive
    assert!(psmux_cmd(&["has-session", "-t", "degrade"]).success);
    // Should be able to kill cleanly
    assert!(psmux_cmd(&["kill-session", "-t", "degrade"]).success);
}
```
