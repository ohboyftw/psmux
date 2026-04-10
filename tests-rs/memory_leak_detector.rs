// tests/memory_leak_detector.rs
//
// Integration test suite that detects memory leaks by monitoring RSS
// across create/destroy cycles of sessions, windows, and panes.
//
// Usage:
//   cargo test --test memory_leak_detector -- --test-threads=1 --nocapture
//
// IMPORTANT: Run with --test-threads=1 to avoid interference between tests.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ─── Configuration ─────────────────────────────────────────────────────────

const LEAK_THRESHOLD_KB: u64 = 5120; // 5MB max leak tolerated
const CYCLE_COOLDOWN_MS: u64 = 200; // wait between create/destroy
const POST_STRESS_COOLDOWN_S: u64 = 5; // wait after all cycles
const RSS_SAMPLE_INTERVAL_MS: u64 = 500; // sampling frequency

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

fn psmux_output(args: &[&str]) -> String {
    Command::new("psmux")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn cleanup() {
    // Kill only test sessions, NOT the server — avoids destroying user's real sessions
    for prefix in &[
        "baseline",
        "leak-sess-",
        "pane-leak",
        "flood-leak",
        "lifecycle-anchor",
        "life-",
        "renamed-",
        "copy-flood",
    ] {
        let output = psmux_output(&["list-sessions", "-F", "#{session_name}"]);
        for line in output.lines() {
            if line.starts_with(prefix) {
                let _ = psmux(&["kill-session", "-t", line]);
            }
        }
    }
    std::thread::sleep(Duration::from_millis(500));
}

/// Get RSS of the psmux server process in KB.
/// Works on both Windows and Linux.
fn get_server_rss_kb() -> Option<u64> {
    #[cfg(windows)]
    {
        // Use PowerShell to get WorkingSet of psmux process
        let output = Command::new("powershell")
            .args([
                "-Command",
                "(Get-Process psmux -ErrorAction SilentlyContinue | \
                Select-Object -First 1).WorkingSet64 / 1KB",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.trim().parse::<f64>().ok().map(|v| v as u64)
    }
    #[cfg(not(windows))]
    {
        // Use /proc for Linux (useful in WSL testing)
        let output = Command::new("sh")
            .args([
                "-c",
                "ps -o rss= -p $(pgrep -f 'psmux.*server' | head -1) 2>/dev/null",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.trim().parse::<u64>().ok()
    }
}

/// Sample RSS over a duration, returning (timestamp_ms, rss_kb) pairs
fn sample_rss(duration: Duration, interval: Duration) -> Vec<(u64, u64)> {
    let start = Instant::now();
    let mut samples = Vec::new();
    while start.elapsed() < duration {
        if let Some(rss) = get_server_rss_kb() {
            samples.push((start.elapsed().as_millis() as u64, rss));
        }
        std::thread::sleep(interval);
    }
    samples
}

/// Compute linear regression slope on RSS samples (KB per millisecond)
fn rss_growth_rate(samples: &[(u64, u64)]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let n = samples.len() as f64;
    let sum_x: f64 = samples.iter().map(|(t, _)| *t as f64).sum();
    let sum_y: f64 = samples.iter().map(|(_, r)| *r as f64).sum();
    let sum_xy: f64 = samples.iter().map(|(t, r)| *t as f64 * *r as f64).sum();
    let sum_xx: f64 = samples.iter().map(|(t, _)| (*t as f64).powi(2)).sum();
    (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x)
}

struct LeakTestResult {
    test_name: String,
    baseline_rss_kb: u64,
    final_rss_kb: u64,
    delta_kb: i64,
    growth_rate_kb_per_sec: f64,
    cycles: usize,
    passed: bool,
}

impl LeakTestResult {
    fn report(&self) {
        let status = if self.passed { "PASS" } else { "FAIL" };
        eprintln!(
            "[{status}] {name}: baseline={base}KB final={final_}KB delta={delta:+}KB \
             rate={rate:.2}KB/s cycles={cycles}",
            status = status,
            name = self.test_name,
            base = self.baseline_rss_kb,
            final_ = self.final_rss_kb,
            delta = self.delta_kb,
            rate = self.growth_rate_kb_per_sec,
            cycles = self.cycles,
        );
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[test]
fn leak_test_session_create_destroy() {
    cleanup();
    let cycles = 200;

    // Establish baseline
    psmux(&["new-session", "-d", "-s", "baseline"]);
    std::thread::sleep(Duration::from_secs(2));
    let baseline = get_server_rss_kb().unwrap_or(0);
    psmux(&["kill-session", "-t", "baseline"]);

    // Run cycles
    for i in 0..cycles {
        psmux(&["new-session", "-d", "-s", &format!("leak-sess-{i}")]);
        psmux(&["kill-session", "-t", &format!("leak-sess-{i}")]);
        if i % 50 == 0 {
            std::thread::sleep(Duration::from_millis(CYCLE_COOLDOWN_MS));
        }
    }

    std::thread::sleep(Duration::from_secs(POST_STRESS_COOLDOWN_S));
    let final_rss = get_server_rss_kb().unwrap_or(0);
    let delta = final_rss as i64 - baseline as i64;

    let result = LeakTestResult {
        test_name: "session_create_destroy".into(),
        baseline_rss_kb: baseline,
        final_rss_kb: final_rss,
        delta_kb: delta,
        growth_rate_kb_per_sec: delta as f64 / (cycles as f64 * CYCLE_COOLDOWN_MS as f64 / 1000.0),
        cycles,
        passed: delta < LEAK_THRESHOLD_KB as i64,
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "Memory leak detected: {delta}KB over {cycles} cycles"
    );
}

#[test]
fn leak_test_pane_create_destroy() {
    cleanup();
    let cycles = 100;

    psmux(&["new-session", "-d", "-s", "pane-leak"]);
    std::thread::sleep(Duration::from_secs(2));
    let baseline = get_server_rss_kb().unwrap_or(0);

    for i in 0..cycles {
        // Create 10 panes
        for _ in 0..10 {
            psmux(&["split-window", "-t", "pane-leak"]);
        }
        // Kill all except the first pane by killing and recreating the window
        psmux(&["kill-window", "-t", "pane-leak:0"]);
        psmux(&["new-window", "-t", "pane-leak"]);
        if i % 20 == 0 {
            std::thread::sleep(Duration::from_millis(CYCLE_COOLDOWN_MS));
        }
    }

    std::thread::sleep(Duration::from_secs(POST_STRESS_COOLDOWN_S));
    let final_rss = get_server_rss_kb().unwrap_or(0);
    let delta = final_rss as i64 - baseline as i64;

    let result = LeakTestResult {
        test_name: "pane_create_destroy".into(),
        baseline_rss_kb: baseline,
        final_rss_kb: final_rss,
        delta_kb: delta,
        growth_rate_kb_per_sec: 0.0,
        cycles,
        passed: delta < LEAK_THRESHOLD_KB as i64,
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "Pane leak detected: {delta}KB over {cycles}×10 panes"
    );
}

#[test]
fn leak_test_output_flood_bounded_buffer() {
    cleanup();

    psmux(&["new-session", "-d", "-s", "flood-leak"]);
    std::thread::sleep(Duration::from_secs(1));

    // Start flooding output
    #[cfg(windows)]
    psmux(&[
        "send-keys",
        "-t",
        "flood-leak",
        "cmd /c \"for /L %i in (1,1,999999) do @echo ########################################\"",
        "Enter",
    ]);
    #[cfg(not(windows))]
    psmux(&[
        "send-keys",
        "-t",
        "flood-leak",
        "yes '########################################'",
        "Enter",
    ]);

    // Sample RSS for 60 seconds
    let samples = sample_rss(
        Duration::from_secs(60),
        Duration::from_millis(RSS_SAMPLE_INTERVAL_MS),
    );

    // Kill the flood
    psmux(&["send-keys", "-t", "flood-leak", "C-c", ""]);
    std::thread::sleep(Duration::from_secs(2));

    let rate = rss_growth_rate(&samples);
    let rate_kb_per_sec = rate * 1000.0; // convert from KB/ms to KB/s

    eprintln!(
        "[{}] output_flood_buffer: {} samples, growth_rate={:.2}KB/s",
        if rate_kb_per_sec < 100.0 {
            "PASS"
        } else {
            "FAIL"
        },
        samples.len(),
        rate_kb_per_sec
    );

    // Print RSS timeline for debugging
    eprintln!("  RSS timeline (first 10 / last 10 samples):");
    for s in samples.iter().take(10) {
        eprintln!("    t={:6}ms rss={}KB", s.0, s.1);
    }
    if samples.len() > 20 {
        eprintln!("    ...");
        for s in samples.iter().rev().take(10).rev() {
            eprintln!("    t={:6}ms rss={}KB", s.0, s.1);
        }
    }

    cleanup();
    assert!(
        rate_kb_per_sec < 100.0,
        "VT100 buffer unbounded: RSS growing at {rate_kb_per_sec:.1}KB/s during flood"
    );
}

#[test]
fn leak_test_full_lifecycle_stress() {
    cleanup();
    let cycles = 50;

    psmux(&["new-session", "-d", "-s", "lifecycle-anchor"]);
    std::thread::sleep(Duration::from_secs(2));
    let baseline = get_server_rss_kb().unwrap_or(0);

    for i in 0..cycles {
        let sess = format!("life-{i}");
        psmux(&["new-session", "-d", "-s", &sess]);
        psmux(&["new-window", "-t", &sess]);
        psmux(&["new-window", "-t", &sess]);
        psmux(&["split-window", "-t", &format!("{sess}:0")]);
        psmux(&["split-window", "-t", &format!("{sess}:1")]);
        psmux(&["split-window", "-t", &format!("{sess}:2")]);

        // Send some output to exercise the VT100 parser
        for w in 0..3 {
            psmux(&[
                "send-keys",
                "-t",
                &format!("{sess}:{w}"),
                "echo hello",
                "Enter",
            ]);
        }
        std::thread::sleep(Duration::from_millis(100));

        // Rename, resize, exercise format engine
        psmux(&["rename-session", "-t", &sess, &format!("renamed-{i}")]);
        psmux(&[
            "rename-window",
            "-t",
            &format!("renamed-{i}:0"),
            &format!("win-{i}"),
        ]);

        // Destroy
        psmux(&["kill-session", "-t", &format!("renamed-{i}")]);
    }

    std::thread::sleep(Duration::from_secs(POST_STRESS_COOLDOWN_S));
    let final_rss = get_server_rss_kb().unwrap_or(0);
    let delta = final_rss as i64 - baseline as i64;

    let result = LeakTestResult {
        test_name: "full_lifecycle_stress".into(),
        baseline_rss_kb: baseline,
        final_rss_kb: final_rss,
        delta_kb: delta,
        growth_rate_kb_per_sec: 0.0,
        cycles,
        passed: delta < LEAK_THRESHOLD_KB as i64,
    };
    result.report();
    cleanup();
    assert!(
        result.passed,
        "Lifecycle leak: {delta}KB after {cycles} full cycles"
    );
}

/// Reproduces reported 11GB memory leak: flood output into a pane, then enter
/// copy mode and scroll up/down repeatedly while output continues flowing.
/// The reader thread keeps feeding the parser while copy mode serializes the
/// scrollback — this is the exact scenario that triggers the leak.
#[test]
fn leak_test_copy_mode_scroll_during_flood() {
    cleanup();

    psmux(&["new-session", "-d", "-s", "copy-flood"]);
    std::thread::sleep(Duration::from_secs(2));

    // Fill the scrollback buffer with output
    #[cfg(windows)]
    psmux(&["send-keys", "-t", "copy-flood",
        "cmd /c \"for /L %i in (1,1,999999) do @echo LINE_%i_########################################\"",
        "Enter"]);
    #[cfg(not(windows))]
    psmux(&[
        "send-keys",
        "-t",
        "copy-flood",
        "yes 'LINE_########################################'",
        "Enter",
    ]);

    // Let output accumulate for 10 seconds
    std::thread::sleep(Duration::from_secs(10));

    let baseline = get_server_rss_kb().unwrap_or(0);
    eprintln!("[INFO] copy_mode_scroll_flood: baseline RSS = {baseline}KB (after 10s flood)");

    // Enter copy mode (Ctrl-b [)
    psmux(&["send-keys", "-t", "copy-flood", "C-b", "["]);
    std::thread::sleep(Duration::from_millis(500));

    // Repeatedly scroll up and down for 30 seconds while output keeps flowing
    let scroll_start = Instant::now();
    let mut scroll_cycles = 0u32;
    let mut rss_samples: Vec<(u64, u64)> = Vec::new();

    while scroll_start.elapsed() < Duration::from_secs(30) {
        // Page up 5 times
        for _ in 0..5 {
            psmux(&["send-keys", "-t", "copy-flood", "C-u", ""]);
        }
        std::thread::sleep(Duration::from_millis(200));

        // Page down 3 times
        for _ in 0..3 {
            psmux(&["send-keys", "-t", "copy-flood", "C-d", ""]);
        }
        std::thread::sleep(Duration::from_millis(200));

        scroll_cycles += 1;

        // Sample RSS every cycle
        if let Some(rss) = get_server_rss_kb() {
            rss_samples.push((scroll_start.elapsed().as_millis() as u64, rss));
        }
    }

    // Exit copy mode
    psmux(&["send-keys", "-t", "copy-flood", "q", ""]);
    std::thread::sleep(Duration::from_millis(500));

    // Stop the flood
    psmux(&["send-keys", "-t", "copy-flood", "C-c", ""]);
    std::thread::sleep(Duration::from_secs(POST_STRESS_COOLDOWN_S));

    let final_rss = get_server_rss_kb().unwrap_or(0);
    let delta = final_rss as i64 - baseline as i64;
    let rate = rss_growth_rate(&rss_samples);
    let rate_kb_per_sec = rate * 1000.0;

    eprintln!(
        "[{}] copy_mode_scroll_flood: baseline={baseline}KB final={final_rss}KB \
         delta={delta:+}KB rate={rate_kb_per_sec:.2}KB/s scroll_cycles={scroll_cycles}",
        if delta < (LEAK_THRESHOLD_KB as i64 * 10) {
            "PASS"
        } else {
            "FAIL"
        },
    );

    // Print RSS timeline
    eprintln!("  RSS timeline:");
    for s in rss_samples.iter().take(5) {
        eprintln!("    t={:6}ms rss={}KB", s.0, s.1);
    }
    if rss_samples.len() > 10 {
        eprintln!("    ...");
        for s in rss_samples.iter().rev().take(5).rev() {
            eprintln!("    t={:6}ms rss={}KB", s.0, s.1);
        }
    }

    cleanup();
    // Allow 50MB growth (10x normal threshold) since we're flooding + scrolling simultaneously
    assert!(
        delta < (LEAK_THRESHOLD_KB as i64 * 10),
        "Copy mode scroll leak: {delta}KB growth over {scroll_cycles} scroll cycles ({rate_kb_per_sec:.1}KB/s)"
    );
}
