//! `psmux-fleet-mcp` — stdio JSON-RPC server exposing psmux-fleet tools.

use anyhow::Result;
use psmux_fleet_mcp::tools::{list, status, ErrorEnvelope, ListParams, StatusParams};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Args {
    state_path: Option<PathBuf>,
    session: Option<String>,
}

fn parse_args() -> Result<Args> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut state_path: Option<PathBuf> = None;
    let mut session: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--state-path" => {
                i += 1;
                if i >= argv.len() {
                    anyhow::bail!("--state-path requires a value");
                }
                state_path = Some(PathBuf::from(&argv[i]));
            }
            "--session" => {
                i += 1;
                if i >= argv.len() {
                    anyhow::bail!("--session requires a value");
                }
                session = Some(argv[i].clone());
            }
            "-h" | "--help" => {
                println!(
                    "psmux-fleet-mcp — stdio JSON-RPC server\n\n\
                     USAGE: psmux-fleet-mcp [--state-path PATH] [--session NAME]\n\n\
                     METHODS:\n  \
                       fleet.list     List workspaces in the session\n  \
                       fleet.status   Detail for one workspace (params: {{\"id\": \"<id>\"}})"
                );
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
        i += 1;
    }
    Ok(Args {
        state_path,
        session,
    })
}

fn resolve_state_path(args: &Args) -> Result<PathBuf> {
    if let Some(p) = &args.state_path {
        return Ok(p.clone());
    }
    let session = args
        .session
        .clone()
        .or_else(|| std::env::var("PSMUX_SESSION").ok())
        .ok_or_else(|| anyhow::anyhow!("SESSION_NOT_FOUND: pass --session or set PSMUX_SESSION"))?;
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(home
        .join(".psmux")
        .join("orchestrate")
        .join(session)
        .join("state.json"))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let state_path = resolve_state_path(&args)?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = stdout;

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = handle(&state_path, &line);
        writer.write_all(resp.to_string().as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

fn handle(state_path: &std::path::Path, line: &str) -> serde_json::Value {
    let req: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({
                "jsonrpc": "2.0",
                "id": serde_json::Value::Null,
                "error": { "code": "PARSE_ERROR", "message": e.to_string() }
            });
        }
    };
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));

    match dispatch(state_path, &method, params) {
        Ok(result) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
        Err(envelope) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": envelope,
        }),
    }
}

fn dispatch(
    state_path: &std::path::Path,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, ErrorEnvelope> {
    match method {
        "fleet.list" => {
            let p: ListParams = serde_json::from_value(params).map_err(internal_err)?;
            let r = list(state_path, p).map_err(fleet_err)?;
            Ok(serde_json::to_value(r).map_err(internal_err)?)
        }
        "fleet.status" => {
            let p: StatusParams = serde_json::from_value(params).map_err(internal_err)?;
            let r = status(state_path, p).map_err(fleet_err)?;
            Ok(serde_json::to_value(r).map_err(internal_err)?)
        }
        other => Err(ErrorEnvelope {
            code: "METHOD_NOT_FOUND".into(),
            message: other.into(),
        }),
    }
}

fn internal_err(e: serde_json::Error) -> ErrorEnvelope {
    ErrorEnvelope {
        code: "INTERNAL".into(),
        message: e.to_string(),
    }
}

fn fleet_err(e: psmux_fleet::FleetError) -> ErrorEnvelope {
    ErrorEnvelope {
        code: e.code().into(),
        message: e.to_string(),
    }
}
