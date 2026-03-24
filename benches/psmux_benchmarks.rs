// benches/psmux_benchmarks.rs
//
// Criterion benchmark suite for psmux performance baselines.
// Place this file in the `benches/` directory of the psmux repo.
//
// Usage:
//   cargo bench --bench psmux_benchmarks
//   cargo bench --bench psmux_benchmarks -- --save-baseline v3.2.0
//   cargo bench --bench psmux_benchmarks -- --baseline v3.2.0  # compare
//
// Prerequisites:
//   [dev-dependencies]
//   criterion = { version = "0.5", features = ["html_reports"] }
//
//   [[bench]]
//   name = "psmux_benchmarks"
//   harness = false

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Helper: run a psmux command and return (success, duration)
fn psmux_cmd(args: &[&str]) -> (bool, Duration) {
    let start = Instant::now();
    let output = Command::new("psmux")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute psmux");
    (output.status.success(), start.elapsed())
}

/// Helper: ensure clean state
fn cleanup() {
    let _ = Command::new("psmux")
        .args(["kill-server"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    std::thread::sleep(Duration::from_millis(500));
}

// ─── Benchmark: Session Creation ───────────────────────────────────────────

fn bench_session_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("session_lifecycle");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("create_detached_session", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for i in 0..iters {
                let name = format!("bench-sess-{i}");
                let (success, elapsed) = psmux_cmd(&["new-session", "-d", "-s", &name]);
                assert!(success, "Session creation failed");
                total += elapsed;
                psmux_cmd(&["kill-session", "-t", &name]);
            }
            cleanup();
            total
        });
    });

    group.bench_function("create_and_kill_session", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for i in 0..iters {
                let name = format!("bench-ck-{i}");
                let start = Instant::now();
                psmux_cmd(&["new-session", "-d", "-s", &name]);
                psmux_cmd(&["kill-session", "-t", &name]);
                total += start.elapsed();
            }
            cleanup();
            total
        });
    });

    group.finish();
}

// ─── Benchmark: Window Operations ──────────────────────────────────────────

fn bench_window_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("window_operations");
    group.sample_size(15);

    group.bench_function("create_window", |b| {
        cleanup();
        psmux_cmd(&["new-session", "-d", "-s", "win-bench"]);
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let (_, elapsed) = psmux_cmd(&["new-window", "-t", "win-bench"]);
                total += elapsed;
            }
            total
        });
        cleanup();
    });

    // Benchmark: creating N windows in rapid succession
    for count in [5, 10, 15, 20] {
        group.bench_with_input(
            BenchmarkId::new("rapid_window_burst", count),
            &count,
            |b, &n| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for iter in 0..iters {
                        let name = format!("burst-{iter}");
                        psmux_cmd(&["new-session", "-d", "-s", &name]);
                        let start = Instant::now();
                        for _ in 0..n {
                            psmux_cmd(&["new-window", "-t", &name]);
                        }
                        total += start.elapsed();
                        psmux_cmd(&["kill-session", "-t", &name]);
                    }
                    cleanup();
                    total
                });
            },
        );
    }

    group.finish();
}

// ─── Benchmark: Pane Splitting ─────────────────────────────────────────────

fn bench_pane_splitting(c: &mut Criterion) {
    let mut group = c.benchmark_group("pane_splitting");
    group.sample_size(15);

    group.bench_function("split_horizontal", |b| {
        cleanup();
        psmux_cmd(&["new-session", "-d", "-s", "split-bench"]);
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let (_, elapsed) = psmux_cmd(&["split-window", "-h", "-t", "split-bench"]);
                total += elapsed;
            }
            total
        });
        cleanup();
    });

    group.bench_function("split_vertical", |b| {
        cleanup();
        psmux_cmd(&["new-session", "-d", "-s", "split-v-bench"]);
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let (_, elapsed) = psmux_cmd(&["split-window", "-v", "-t", "split-v-bench"]);
                total += elapsed;
            }
            total
        });
        cleanup();
    });

    group.finish();
}

// ─── Benchmark: List Operations (Metadata Query Speed) ─────────────────────

fn bench_list_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_operations");
    group.sample_size(30);

    // Setup: create varying session counts, then benchmark list speed
    for session_count in [1, 5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::new("list_sessions", session_count),
            &session_count,
            |b, &n| {
                cleanup();
                for i in 0..n {
                    psmux_cmd(&["new-session", "-d", "-s", &format!("list-{i}")]);
                }
                b.iter(|| {
                    let (success, _) = psmux_cmd(&["list-sessions"]);
                    assert!(success);
                });
                cleanup();
            },
        );
    }

    group.finish();
}

// ─── Benchmark: Command Dispatch Overhead ──────────────────────────────────

fn bench_command_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("command_dispatch");
    group.sample_size(50);

    cleanup();
    psmux_cmd(&["new-session", "-d", "-s", "dispatch"]);

    // Measure overhead of various no-op / lightweight commands
    for cmd in ["list-sessions", "list-windows", "list-panes", "display-message"] {
        group.bench_function(cmd, |b| {
            b.iter(|| {
                psmux_cmd(&[cmd, "-t", "dispatch"]);
            });
        });
    }

    cleanup();
    group.finish();
}

criterion_group!(
    benches,
    bench_session_creation,
    bench_window_operations,
    bench_pane_splitting,
    bench_list_operations,
    bench_command_dispatch,
);
criterion_main!(benches);
