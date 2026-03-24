# CI Integration for psmux Performance Tests

## GitHub Actions Workflow

Add this to `.github/workflows/perf.yml`:

```yaml
name: Performance & Memory Tests

on:
  pull_request:
    branches: [master]
  push:
    branches: [master]
  schedule:
    - cron: '0 6 * * 1'  # Weekly Monday 6am UTC

env:
  CARGO_TERM_COLOR: always

jobs:
  benchmarks:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
      
      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-bench-${{ hashFiles('**/Cargo.lock') }}
      
      - name: Build release binary
        run: cargo build --release
      
      - name: Add to PATH
        run: echo "$env:GITHUB_WORKSPACE\target\release" | Out-File -Append $env:GITHUB_PATH
      
      - name: Run Criterion benchmarks
        run: cargo bench --bench psmux_benchmarks -- --output-format bencher | tee bench_output.txt
      
      - name: Store benchmark results
        uses: benchmark-action/github-action-benchmark@v1
        with:
          name: psmux Performance Benchmarks
          tool: cargo
          output-file-path: bench_output.txt
          github-token: ${{ secrets.GITHUB_TOKEN }}
          auto-push: true
          alert-threshold: '150%'
          comment-on-alert: true
          fail-on-alert: true

  stress-tests:
    runs-on: windows-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
      
      - name: Build release binary
        run: cargo build --release
      
      - name: Add to PATH
        run: echo "$env:GITHUB_WORKSPACE\target\release" | Out-File -Append $env:GITHUB_PATH

      - name: Run Memory Leak Tests
        run: cargo test --test memory_leak_detector -- --test-threads=1 --nocapture
      
      - name: Run CPU Runoff Tests
        run: cargo test --test cpu_runoff_detector -- --test-threads=1 --nocapture
      
      - name: Run PowerShell Stress Suite
        run: |
          .\scripts\psmux_stress_test.ps1 -TestSuite All -Cycles 50 -OutputJson stress_results.json
      
      - name: Upload stress test results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: stress-test-results
          path: stress_results.json
```

## Performance Regression Detection

The `benchmark-action/github-action-benchmark` action:
- Stores historical benchmark data in a `gh-pages` branch
- Generates trend charts at `https://<user>.github.io/<repo>/dev/bench/`
- Comments on PRs when performance regresses >50% (configurable)
- Fails the build if regression exceeds threshold

## Local Regression Testing

Compare against a saved baseline:
```bash
# Save baseline on master
cargo bench --bench psmux_benchmarks -- --save-baseline master

# Switch to feature branch, run comparison
git checkout feature-branch
cargo bench --bench psmux_benchmarks -- --baseline master
```

Criterion generates HTML reports in `target/criterion/` with violin plots
and statistical significance testing.

## Interpreting Results

### Benchmark Metrics
- **Session creation**: Should be < 100ms (documented target)
- **Window creation**: Should be < 80ms (documented target)
- **Pane split**: Should be < 80ms (documented target)
- **List operations**: Should scale linearly with session count

### Leak Test Thresholds
- **Memory**: < 5MB growth after 200 create/destroy cycles
- **Handles**: < 20 handle increase after 100 ConPTY cycles
- **Orphans**: < 2 extra shell processes after 100 session cycles
- **Buffer growth**: RSS growth rate < 100KB/s during sustained output flood

### CPU Thresholds
- **Idle session**: < 5% CPU
- **Format engine**: < 30% CPU with complex status line
- **10-pane flood**: < 400% CPU (4-core equivalent)
- **Responsiveness**: list-sessions < 2s during any stress test
