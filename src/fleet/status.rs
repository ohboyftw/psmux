//! `psmux fleet status <id>` — workspace detail view.

use psmux_fleet::WorkspaceTable;
use std::io;
use std::path::PathBuf;

pub fn run(args: &[String]) -> io::Result<()> {
    let parsed = match parse_args(args) {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(()),
        Err(msg) => {
            eprintln!("psmux fleet status: {msg}");
            std::process::exit(2);
        }
    };

    let path = match super::ls::resolve_state_path(parsed.state_path, parsed.session) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("psmux fleet status: {msg}");
            std::process::exit(2);
        }
    };

    let table = WorkspaceTable::hydrate_cold(&path).map_err(|e| {
        io::Error::other(format!(
            "psmux fleet status: loading {}: {e}",
            path.display()
        ))
    })?;

    let workspace = match table.find(&parsed.id) {
        Some(w) => w,
        None => {
            eprintln!("psmux fleet status: WORKSPACE_NOT_FOUND: {}", parsed.id);
            return Err(io::Error::other(format!(
                "WORKSPACE_NOT_FOUND: {}",
                parsed.id
            )));
        }
    };

    if parsed.json {
        let text = serde_json::to_string_pretty(workspace)
            .map_err(|e| io::Error::other(format!("psmux fleet status: serialize: {e}")))?;
        println!("{text}");
    } else {
        print_detail(workspace);
    }
    Ok(())
}

struct StatusArgs {
    id: String,
    state_path: Option<PathBuf>,
    session: Option<String>,
    json: bool,
}

fn parse_args(args: &[String]) -> Result<Option<StatusArgs>, String> {
    let mut state_path: Option<PathBuf> = None;
    let mut session: Option<String> = None;
    let mut json = false;
    let mut id: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--state-path" => {
                let v = args.get(i + 1).ok_or("--state-path requires a value")?;
                state_path = Some(PathBuf::from(v));
                i += 2;
                continue;
            }
            "--session" => {
                let v = args.get(i + 1).ok_or("--session requires a value")?;
                session = Some(v.clone());
                i += 2;
                continue;
            }
            "--json" => json = true,
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            other if other.starts_with('-') => {
                return Err(format!("unexpected flag '{other}'"));
            }
            other => {
                if id.is_some() {
                    return Err(format!("unexpected positional argument '{other}'"));
                }
                id = Some(other.to_string());
            }
        }
        i += 1;
    }

    let id = id.ok_or("missing <id> positional argument")?;
    Ok(Some(StatusArgs {
        id,
        state_path,
        session,
        json,
    }))
}

fn print_detail(w: &psmux_fleet::Workspace) {
    let status = format!("{:?}", w.status).to_lowercase();
    println!("id: {}", w.id);
    println!("session: {}", w.session);
    println!("status: {}", status);
    println!("pane: {}", w.pane_id.as_deref().unwrap_or("-"));
    match w.exit_code {
        Some(code) => println!("exit_code: {code}"),
        None => println!("exit_code: -"),
    }
    if let Some(p) = &w.crash_dump_path {
        println!("crash_dump: {}", p.display());
    }
}

fn print_help() {
    println!(
        "psmux fleet status — show workspace detail\n\
         \n\
         USAGE:\n    psmux fleet status <id> [--state-path PATH | --session NAME] [--json]\n\
         \n\
         OPTIONS:\n\
             --state-path PATH  Explicit state.json path (overrides session discovery)\n\
             --session NAME     Session name (default: $PSMUX_SESSION)\n\
             --json             Emit JSON instead of a human-readable detail block"
    );
}
