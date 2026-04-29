pub mod dispatcher;
pub mod pipe;
pub mod protocol;

// SPIKE: parallel implementation of pipe.rs built on the `interprocess` crate.
// Default OFF; behind `--features interprocess-pipe`. Original `pipe` module is
// untouched. Findings: .claude/internal/spike-interprocess-pipe.md
#[cfg(all(windows, feature = "interprocess-pipe"))]
pub mod pipe_interprocess;
