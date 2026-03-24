---
name: psmux-perf-profiler
description: >
  Performance profiling, memory leak detection, and CPU stress testing for psmux.
  Use when investigating memory leaks, CPU spikes, ConPTY handle leaks, pane/session
  lifecycle issues, or building CI perf regression gates. Triggers on "profile",
  "memory leak", "CPU usage", "stress test", "benchmark", "perf regression",
  "handle leak", "ConPTY performance".
---

# psmux Performance & Memory Profile Test Skill

## Overview

Complete framework for benchmarks, memory leak detection, and CPU runoff stress tests
for psmux. Covers the full profiling lifecycle: instrumenting, triggering failure modes,
capturing metrics, and analyzing results.

## Architecture Context

| Component | What It Does | Leak/CPU Risk |
|-----------|-------------|---------------|
| **ConPTY spawn** | Creates Windows pseudo-terminals per pane | Handle leaks, process orphans |
| **VT100 parser** (`vt100-psmux` crate) | Parses terminal escape sequences | Unbounded buffer growth (capped at 10MB since c967cb1) |
| **AsyncPaneWriter** | Bounded channel + background drain thread | Channel backpressure, thread accumulation |
| **Pane reader threads** | 64KB buffer per pane, mutex-guarded | Thread accumulation, mutex contention |
| **Session server** | TCP server with adaptive polling (1-50ms debounced) | Socket exhaustion, CPU spin |
| **Format engine** | 126+ format variables, regex-based | Regex backtracking CPU spikes |
| **Snapshot writer** | Background thread with 100ms debounce | Disk I/O stalls (now async) |
| **Ratatui TUI renderer** | Terminal UI rendering loop | Render loop CPU saturation |

## Files

### Code (in project directories)
- `benches/psmux_benchmarks.rs` — Criterion benchmark suite (session/window/pane timing)
- `tests-rs/memory_leak_detector.rs` — RSS monitoring across create/destroy cycles
- `tests-rs/cpu_runoff_detector.rs` — CPU usage bounds under stress scenarios
- `scripts/psmux_stress_test.ps1` — PowerShell OS-level resource leak detection

### References (in this skill directory)
- `profiling-tools.md` — Rust profiling toolchain setup (flamegraph, DHAT, cargo-instruments)
- `stress-scenarios.md` — Specific failure mode triggers for psmux
- `ci-integration.md` — GitHub Actions workflow template for perf regression gates

## Quick Start

### Tier 1 — Quick Smoke (< 2 min)
```bash
cargo bench --bench psmux_benchmarks
```

### Tier 2 — Memory Leak Hunt (5-15 min)
```bash
cargo test --test memory_leak_detector -- --test-threads=1 --nocapture
```

### Tier 3 — CPU Runoff Stress (10-30 min)
```bash
cargo test --test cpu_runoff_detector -- --test-threads=1 --nocapture
```

### Tier 4 — OS-Level Resource Leaks
```powershell
pwsh scripts/psmux_stress_test.ps1 -TestSuite All -Cycles 100
pwsh scripts/psmux_stress_test.ps1 -TestSuite HandleLeak
pwsh scripts/psmux_stress_test.ps1 -TestSuite OrphanProcess
```

### Tier 5 — Full Profile (30+ min)
Combine all tiers with flamegraph generation and heap profiling.

## Known Issues (Fixed)

These were the bugs that motivated this profiling skill (all fixed in c967cb1):

| Bug | Root Cause | Fix |
|-----|-----------|-----|
| 28GB memory spike | Unbounded DCS buffer | 10MB cap in `perform.rs` |
| 5-10 min input freeze | Blocking `write_all()` on ConPTY pipe | AsyncPaneWriter |
| 58% CPU idle | 1ms perpetual poll | 5-tick debounce ramp |
| Snapshot I/O stall | Sync disk I/O on event loop | Background writer + debounce |

## Prerequisites

For benchmarks, add to `Cargo.toml`:
```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "psmux_benchmarks"
harness = false
```
