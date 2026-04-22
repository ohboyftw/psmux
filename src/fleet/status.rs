//! `psmux fleet status <id>` — detail view. Placeholder; real handler in T11.

use std::io;

pub fn run(_args: &[String]) -> io::Result<()> {
    // TODO(T11): replace with real status handler — see
    // docs/superpowers/plans/2026-04-22-fleetview-c1.md Task 11.
    Err(io::Error::other(
        "psmux fleet status not yet implemented (T11)",
    ))
}
