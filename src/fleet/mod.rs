//! `psmux fleet` subcommand tree.
//!
//! Thin CLI surface over the `psmux-fleet` library. All aggregation logic
//! lives in the library; this module parses flat-style args (matching the
//! existing psmux binary pattern — no clap) and routes to `ls` / `status`.

pub mod ls;
pub mod status;

use std::io;

/// Dispatch `psmux fleet <sub> [args...]`.
///
/// `args` is the slice **after** the `fleet` token, e.g. for
/// `psmux fleet ls --json` it would be `["ls", "--json"]`.
pub fn dispatch(args: &[String]) -> io::Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("");
    match sub {
        "ls" => ls::run(&args[1..]),
        "status" => status::run(&args[1..]),
        "" => {
            eprintln!("psmux fleet: missing subcommand\nusage: psmux fleet <ls|status> [args]");
            std::process::exit(2);
        }
        other => {
            eprintln!(
                "psmux fleet: unknown subcommand '{other}'\nusage: psmux fleet <ls|status> [args]"
            );
            std::process::exit(2);
        }
    }
}
