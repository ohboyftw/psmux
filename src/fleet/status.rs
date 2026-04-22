//! `psmux fleet status <id>` — detail view. Placeholder; real handler in T11.

use std::io;

pub fn run(_args: &[String]) -> io::Result<()> {
    Err(io::Error::other(
        "psmux fleet status not yet implemented (T11)",
    ))
}
