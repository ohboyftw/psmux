//! `psmux fleet ls` — list workspaces in a session.

use psmux_fleet::WorkspaceTable;
use std::io;
use std::path::PathBuf;

pub fn run(args: &[String]) -> io::Result<()> {
    let mut state_path: Option<PathBuf> = None;
    let mut session: Option<String> = None;
    let mut json_mode = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--state-path" => match args.get(i + 1) {
                Some(p) => {
                    state_path = Some(PathBuf::from(p));
                    i += 2;
                    continue;
                }
                None => {
                    eprintln!("psmux fleet ls: --state-path requires a value");
                    std::process::exit(2);
                }
            },
            "--session" => match args.get(i + 1) {
                Some(s) => {
                    session = Some(s.clone());
                    i += 2;
                    continue;
                }
                None => {
                    eprintln!("psmux fleet ls: --session requires a value");
                    std::process::exit(2);
                }
            },
            "--json" => json_mode = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("psmux fleet ls: unexpected argument '{other}'");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let path = match resolve_state_path(state_path, session) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("psmux fleet ls: {msg}");
            std::process::exit(2);
        }
    };

    let table = WorkspaceTable::hydrate_cold(&path).map_err(|e| {
        io::Error::other(format!("psmux fleet ls: loading {}: {e}", path.display()))
    })?;

    if json_mode {
        let out = serde_json::json!({
            "workspaces": table.workspaces(),
            "total": table.len(),
        });
        let text = serde_json::to_string_pretty(&out)
            .map_err(|e| io::Error::other(format!("psmux fleet ls: serialize: {e}")))?;
        println!("{text}");
    } else {
        print_table(&table);
    }
    Ok(())
}

fn resolve_state_path(
    explicit: Option<PathBuf>,
    session: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    let session = session
        .or_else(|| std::env::var("PSMUX_SESSION").ok())
        .ok_or_else(|| "no session specified; pass --session or set PSMUX_SESSION".to_string())?;
    let home = dirs::home_dir().ok_or_else(|| "no home dir".to_string())?;
    Ok(home
        .join(".psmux")
        .join("orchestrate")
        .join(session)
        .join("state.json"))
}

fn print_table(table: &WorkspaceTable) {
    println!("{:<25} {:<12} {:<10} TASK", "ID", "STATUS", "PANE");
    for w in table.workspaces() {
        let status = format!("{:?}", w.status).to_lowercase();
        let pane = w.pane_id.as_deref().unwrap_or("-");
        let task = w.task.as_deref().unwrap_or("");
        println!("{:<25} {:<12} {:<10} {}", w.id, status, pane, task);
    }
}

fn print_help() {
    println!(
        "psmux fleet ls — list workspaces in a session\n\
         \n\
         USAGE:\n    psmux fleet ls [--state-path PATH | --session NAME] [--json]\n\
         \n\
         OPTIONS:\n\
             --state-path PATH  Explicit state.json path (overrides session discovery)\n\
             --session NAME     Session name (default: $PSMUX_SESSION)\n\
             --json             Emit JSON instead of a human-readable table"
    );
}
