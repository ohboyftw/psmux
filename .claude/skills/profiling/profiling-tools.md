# Rust Profiling Tools for psmux

## Table of Contents
1. [CPU Profiling](#cpu-profiling)
2. [Memory Profiling](#memory-profiling)
3. [Allocation Tracking](#allocation-tracking)
4. [Flamegraph Generation](#flamegraph-generation)
5. [Windows-Specific Tools](#windows-specific-tools)
6. [Cargo Dependencies to Add](#cargo-dependencies)

---

## CPU Profiling

### cargo-flamegraph (Cross-platform, recommended)
```bash
cargo install flamegraph
# Build with debug symbols in release
cargo flamegraph --bin psmux -- new-session -d -s bench-session
```

Requires `CARGO_PROFILE_RELEASE_DEBUG=true` in Cargo.toml or env var.

### pprof-rs (In-process, programmatic)
Add to `Cargo.toml` under a `profiling` feature:
```toml
[features]
profiling = ["pprof"]

[dependencies]
pprof = { version = "0.14", features = ["flamegraph", "criterion"], optional = true }
```

Use in benchmarks:
```rust
#[cfg(feature = "profiling")]
fn profile_guard() -> pprof::ProfilerGuard<'static> {
    pprof::ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .unwrap()
}
```

### ETW Tracing (Windows-native)
For Windows-specific ConPTY profiling, use Event Tracing for Windows:
```rust
// In a test harness
use windows_sys::Win32::System::Diagnostics::Etw::*;
// Or use the `tracing-etw` crate for integration with Rust's tracing ecosystem
```

---

## Memory Profiling

### DHAT (Valgrind-based, Linux/WSL)
The `dhat` crate provides in-process heap profiling without Valgrind:
```toml
[dev-dependencies]
dhat = "0.3"

[features]
dhat-heap = []
```

```rust
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    
    // ... psmux logic
}
```

Run: `cargo run --features dhat-heap` → produces `dhat-heap.json`, view at https://nnethercote.github.io/dh_view/dh_view.html

### jemalloc Heap Profiling (Production-grade)
```toml
[dependencies]
tikv-jemallocator = { version = "0.6", features = ["profiling", "unprefixed_malloc_on_supported_platforms"] }
```

```rust
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

Note: jemalloc does NOT work with MSVC (psmux's default Windows target). Use DHAT
or the Windows-native tools instead for Windows profiling.

### Windows Process Memory Tracking (No external deps)
```rust
use std::process::Command;

fn get_rss_kb() -> u64 {
    // Use Windows API: GetProcessMemoryInfo
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::*;
        use windows_sys::Win32::System::Threading::*;
        unsafe {
            let process = GetCurrentProcess();
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            GetProcessMemoryInfo(process, &mut counters, counters.cb);
            counters.WorkingSetSize as u64 / 1024
        }
    }
    #[cfg(not(windows))]
    {
        // Fallback: read /proc/self/status
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        status.lines()
            .find(|l| l.starts_with("VmRSS:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}
```

---

## Allocation Tracking

### tracking_allocator (Count allocations per code path)
```toml
[dev-dependencies]
tracking-allocator = "0.4"
```

Use in benchmarks to count allocations during specific operations:
```rust
use tracking_allocator::{AllocationTracker, AllocTracker};

struct PsmuxTracker {
    alloc_count: AtomicU64,
    alloc_bytes: AtomicU64,
}

impl AllocationTracker for PsmuxTracker {
    fn allocated(&self, _addr: usize, _object_size: usize, wrapped_size: usize) {
        self.alloc_count.fetch_add(1, Ordering::Relaxed);
        self.alloc_bytes.fetch_add(wrapped_size as u64, Ordering::Relaxed);
    }
    fn deallocated(&self, _addr: usize, _object_size: usize, _wrapped_size: usize) {}
}
```

---

## Flamegraph Generation

### Quick flamegraph from cargo
```bash
# CPU flamegraph
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --bin psmux -- new-session -d

# With specific test scenario
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --bin psmux --test stress_tests -- --test-threads=1
```

### Differential flamegraph (before/after optimization)
```bash
# Capture baseline
cargo flamegraph --bin psmux -o baseline.svg -- new-session -d -s baseline
# Capture after changes
cargo flamegraph --bin psmux -o optimized.svg -- new-session -d -s optimized
# Diff (requires inferno)
cargo install inferno
inferno-diff-folded baseline.txt optimized.txt | inferno-flamegraph > diff.svg
```

---

## Windows-Specific Tools

### Windows Performance Recorder (WPR)
```powershell
# Start recording
wpr -start CPU
# Run psmux stress test
psmux new-session -d -s stress; Start-Sleep 30; psmux kill-server
# Stop and save
wpr -stop psmux_trace.etl
# Analyze with Windows Performance Analyzer (WPA)
wpa psmux_trace.etl
```

### Process Monitor (handle leak detection)
```powershell
# Use handle.exe from Sysinternals to monitor handle count
while ($true) {
    $h = (handle64 -p (Get-Process psmux).Id -s | Select-String "Handle count").ToString()
    "$([DateTime]::Now) $h" | Tee-Object -Append handles.log
    Start-Sleep 1
}
```

---

## Cargo Dependencies

Add these under feature flags to keep the main binary lean:

```toml
[features]
profiling = ["pprof"]
dhat-heap = ["dhat"]
stress-test = []

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
dhat = "0.3"
pprof = { version = "0.14", features = ["flamegraph", "criterion"] }
serde_json = "1.0"
tempfile = "3"

[[bench]]
name = "psmux_benchmarks"
harness = false
```
