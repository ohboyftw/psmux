// tests/cpu_runoff_detector.rs
//
// Integration tests that detect CPU runaway conditions in psmux.
// Each test triggers a known CPU-intensive scenario and asserts that
// CPU usage stays within bounds.
//
// Usage:
//   cargo test --test cpu_runoff_detector -- --test-threads=1 --nocapture

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ─── Configuration ─────────────────────────────────────────────────────────

const MAX_SINGLE_CORE_CPU_PCT: f64 = 90.0; // one core = 100%
const MAX_TOTAL_CPU_PCT: f64 = 400.0; // 4-core system
const CPU_SAMPLE_INTERVAL_MS: u64 = 200;
const STRESS_DURATION_SECS: u64 = 15;

// ─── Helpers ───────────────────────────────────────────────────────────────

fn psmux(args: &[&str]) -> bool {
    Command::new("psmux")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cleanup() {
    let _ = Command::new("psmux")
        .args(["kill-server"])
        .stdin(Stdio::null())
        .output();
    std::thread::sleep(Duration::from_secs(1));
}

/// Get CPU usage percentage of the psmux server process.
/// Returns (user_cpu_percent, total_cpu_percent)
#[cfg(windows)]
fn get_server_cpu_pct() -> Option<f64> {
    let output = Command::new("powershell")
        .args([
            "-Command",
            "(Get-Process psmux -ErrorAction SilentlyContinue | \
             Select-Object -First 1).CPU",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<f64>().ok()
}

#[cfg(not(windows))]
fn get_server_cpu_pct() -> Option<f64> {
    let output = Command::new("sh")
        .args([
            "-c",
            "ps -o %cpu= -p $(pgrep -f 'psmux' | head -1) 2>/dev/null",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<f64>().ok()
}

/// Sample CPU over duration, return (timestamp_ms, cpu_pct) pairs
fn sample_cpu(duration: Duration) -> Vec<(u64, f64)> {
    let start = Instant::now();
    let mut samples = Vec::new();
    while start.elapsed() < duration {
        if let Some(cpu) = get_server_cpu_pct() {
            samples.push((start.elapsed().as_millis() as u64, cpu));
        }
        std::thread::sleep(Duration::from_millis(CPU_SAMPLE_INTERVAL_MS));
    }
    samples
}

fn avg_cpu(samples: &[(u64, f64)]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|(_, c)| c).sum::<f64>() / samples.len() as f64
}

fn max_cpu(samples: &[(u64, f64)]) -> f64 {
    samples.iter().map(|(_, c)| *c).fold(0.0f64, f64::max)
}

struct CpuTestResult {
    test_name: String,
    avg_cpu_pct: f64,
    max_cpu_pct: f64,
    sample_count: usize,
    duration_secs: f64,
    passed: bool,
}

impl CpuTestResult {
    fn report(&self) {
        let status = if self.passed { "PASS" } else { "FAIL" };
        eprintln!(
            "[{status}] {name}: avg_cpu={avg:.1}% max_cpu={max:.1}% \
             samples={n} duration={dur:.1}s",
            status = status,
            name = self.test_name,
            avg = self.avg_cpu_pct,
            max = self.max_cpu_pct,
            n = self.sample_count,
            dur = self.duration_secs,
        );
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[test]
fn cpu_test_idle_session_baseline() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "idle"]);
    std::thread::sleep(Duration::from_secs(2)); // let it settle

    let samples = sample_cpu(Duration::from_secs(10));
    let avg = avg_cpu(&samples);
    let max = max_cpu(&samples);

    let result = CpuTestResult {
        test_name: "idle_session_baseline".into(),
        avg_cpu_pct: avg,
        max_cpu_pct: max,
        sample_count: samples.len(),
        duration_secs: 10.0,
        passed: avg < 5.0, // idle should be near-zero CPU
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "Idle session consuming too much CPU: {avg:.1}%"
    );
}

#[test]
fn cpu_test_rapid_window_creation() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "rapid-win"]);

    // Start monitoring
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let monitor = std::thread::spawn(move || {
        let mut samples = Vec::new();
        while !stop_clone.load(Ordering::Relaxed) {
            if let Some(cpu) = get_server_cpu_pct() {
                samples.push((0u64, cpu));
            }
            std::thread::sleep(Duration::from_millis(CPU_SAMPLE_INTERVAL_MS));
        }
        samples
    });

    // Rapid-fire create 20 windows
    let start = Instant::now();
    for _ in 0..20 {
        psmux(&["new-window", "-t", "rapid-win"]);
    }
    let creation_time = start.elapsed();
    eprintln!(
        "  Created 20 windows in {:.2}s",
        creation_time.as_secs_f64()
    );

    // Let CPU settle
    std::thread::sleep(Duration::from_secs(3));
    stop.store(true, Ordering::Relaxed);
    let samples = monitor.join().unwrap();

    let avg = avg_cpu(&samples);
    let max = max_cpu(&samples);

    let result = CpuTestResult {
        test_name: "rapid_window_creation".into(),
        avg_cpu_pct: avg,
        max_cpu_pct: max,
        sample_count: samples.len(),
        duration_secs: creation_time.as_secs_f64() + 3.0,
        passed: max < MAX_SINGLE_CORE_CPU_PCT && creation_time < Duration::from_secs(10),
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "CPU runoff during rapid window creation: max={max:.1}%"
    );
}

#[test]
fn cpu_test_concurrent_output_flood() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "flood-cpu"]);

    // Create 10 panes
    for _ in 0..9 {
        psmux(&["split-window", "-t", "flood-cpu"]);
        psmux(&["select-layout", "-t", "flood-cpu", "tiled"]);
    }

    // Start flooding all panes
    for i in 0..10 {
        #[cfg(windows)]
        psmux(&[
            "send-keys",
            "-t",
            &format!("flood-cpu:.{i}"),
            "cmd /c \"for /L %x in (1,1,99999) do @echo FLOOD_LINE\"",
            "Enter",
        ]);
        #[cfg(not(windows))]
        psmux(&[
            "send-keys",
            "-t",
            &format!("flood-cpu:.{i}"),
            &format!("yes 'FLOOD_LINE_{i}'"),
            "Enter",
        ]);
    }

    // Monitor CPU during flood
    let samples = sample_cpu(Duration::from_secs(STRESS_DURATION_SECS));
    let avg = avg_cpu(&samples);
    let max = max_cpu(&samples);

    // Stop flood
    for i in 0..10 {
        psmux(&["send-keys", "-t", &format!("flood-cpu:.{i}"), "C-c", ""]);
    }

    let result = CpuTestResult {
        test_name: "concurrent_output_flood_10_panes".into(),
        avg_cpu_pct: avg,
        max_cpu_pct: max,
        sample_count: samples.len(),
        duration_secs: STRESS_DURATION_SECS as f64,
        passed: avg < MAX_TOTAL_CPU_PCT,
    };
    result.report();

    // Also check: system should remain responsive
    let start = Instant::now();
    let responsive = psmux(&["list-sessions"]);
    let latency = start.elapsed();
    eprintln!(
        "  Responsiveness check: list-sessions in {:?} (success={})",
        latency, responsive
    );

    cleanup();
    assert!(result.passed, "CPU runoff during flood: avg={avg:.1}%");
    assert!(
        latency < Duration::from_secs(2),
        "System unresponsive during flood"
    );
}

#[test]
fn cpu_test_format_engine_stress() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "fmt-cpu"]);

    // Set a complex status line with many format variables
    let complex_fmt = "#{session_name}|#{window_index}/#{window_name}|\
        #{pane_index}|#{pane_current_command}|#{pane_width}x#{pane_height}|\
        #{client_termtype}|#{host}|#{pid}";
    psmux(&["set-option", "-g", "status-right", complex_fmt]);
    psmux(&["set-option", "-g", "status-left", complex_fmt]);
    psmux(&["set-option", "-g", "status-interval", "1"]); // update every second

    // Create multiple windows to increase format engine work
    for _ in 0..10 {
        psmux(&["new-window", "-t", "fmt-cpu"]);
    }

    // Monitor CPU for 30 seconds of format engine churning
    let samples = sample_cpu(Duration::from_secs(30));
    let avg = avg_cpu(&samples);
    let max = max_cpu(&samples);

    let result = CpuTestResult {
        test_name: "format_engine_stress".into(),
        avg_cpu_pct: avg,
        max_cpu_pct: max,
        sample_count: samples.len(),
        duration_secs: 30.0,
        passed: avg < 30.0, // format engine shouldn't dominate CPU
    };
    result.report();
    cleanup();
    assert!(result.passed, "Format engine CPU runoff: avg={avg:.1}%");
}

#[test]
fn cpu_test_rapid_resize_events() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "resize-cpu"]);

    // Create several panes (triggers lazy resize logic)
    for _ in 0..5 {
        psmux(&["split-window", "-t", "resize-cpu"]);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let monitor = std::thread::spawn(move || {
        let mut samples = Vec::new();
        while !stop_clone.load(Ordering::Relaxed) {
            if let Some(cpu) = get_server_cpu_pct() {
                samples.push((0u64, cpu));
            }
            std::thread::sleep(Duration::from_millis(CPU_SAMPLE_INTERVAL_MS));
        }
        samples
    });

    // Rapid resize events
    for size in (30..200).chain((30..200).rev()) {
        psmux(&["refresh-client", "-C", &format!("{size},{size}")]);
    }

    std::thread::sleep(Duration::from_secs(2)); // settling time
    stop.store(true, Ordering::Relaxed);
    let samples = monitor.join().unwrap();

    let avg = avg_cpu(&samples);
    let max = max_cpu(&samples);

    let result = CpuTestResult {
        test_name: "rapid_resize_events".into(),
        avg_cpu_pct: avg,
        max_cpu_pct: max,
        sample_count: samples.len(),
        duration_secs: 0.0,
        passed: avg < MAX_SINGLE_CORE_CPU_PCT,
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "Resize CPU runoff: avg={avg:.1}%, max={max:.1}%"
    );
}

#[test]
fn cpu_test_session_discovery_malformed_port() {
    cleanup();
    psmux(&["new-session", "-d", "-s", "disc-cpu"]);
    std::thread::sleep(Duration::from_secs(1));

    // This test verifies that a malformed port file doesn't cause
    // the client to spin in a tight polling loop.
    // NOTE: This requires knowledge of the port file location.
    // On Windows it's typically in %TEMP%/psmux-<uid>/
    // The test is best run manually with the port file corrupted.

    // Instead, we test the attach timeout behavior:
    // Try to attach to a non-existent session — client should timeout gracefully
    let start = Instant::now();
    let output = Command::new("psmux")
        .args(["attach", "-t", "nonexistent-session-xyz"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("psmux attach failed to execute");
    let elapsed = start.elapsed();

    eprintln!(
        "  attach to nonexistent session: exit={} elapsed={:.2}s",
        output.status.code().unwrap_or(-1),
        elapsed.as_secs_f64()
    );

    // Should fail quickly (< 3s), not spin for a long time
    assert!(
        elapsed < Duration::from_secs(3),
        "Client spent too long trying to attach to nonexistent session: {:.2}s",
        elapsed.as_secs_f64()
    );
    cleanup();
}
