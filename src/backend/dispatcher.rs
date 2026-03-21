//! JSON-RPC dispatcher: routes method calls to handler functions.
//!
//! Each handler validates its parameters, sends the appropriate CtrlReq to
//! the server's main loop, waits for the response, and returns a
//! `serde_json::Value` result or an `RpcErr`.

use std::sync::mpsc;

use crate::types::CtrlReq;

use super::protocol::*;

/// Structured error type for handler return values, supporting optional `data`.
struct RpcErr {
    code: i32,
    message: String,
    data: Option<serde_json::Value>,
}

impl From<(i32, String)> for RpcErr {
    fn from((code, message): (i32, String)) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }
}

/// Parse a "%N" context_id string into a usize pane index.
/// Returns None if the format is invalid.
fn parse_pane_id(context_id: &str) -> Option<usize> {
    context_id.strip_prefix('%').and_then(|s| s.parse().ok())
}

/// Dispatch a single newline-delimited JSON-RPC request.
///
/// Returns `Some(json_string)` with the response to send back to the client,
/// or `None` if the request is a notification (no `id`) — though the current
/// protocol doesn't use notifications.
pub fn dispatch_rpc(line: &str, tx: &mpsc::Sender<CtrlReq>) -> Option<String> {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp =
                RpcResponse::error(serde_json::Value::Null, -32700, format!("Parse error: {e}"));
            return Some(serde_json::to_string(&resp).unwrap());
        }
    };

    let id = req.id.clone().unwrap_or(serde_json::Value::Null);

    let result = match req.method.as_str() {
        "initialize" => handle_initialize(&req.params, tx),
        "spawn_agent" => handle_spawn_agent(&req.params, tx),
        "write" => handle_write(&req.params, tx),
        "capture" => handle_capture(&req.params, tx),
        "kill" => handle_kill(&req.params, tx),
        "kill_all" => handle_kill_all(&req.params, tx),
        "list" => handle_list(&req.params, tx),
        "run_shell" => handle_run_shell(&req.params, tx),
        _ => Err(RpcErr::from((-32601, format!("Method not found: {}", req.method)))),
    };

    let resp = match result {
        Ok(value) => RpcResponse::success(id, value),
        Err(e) => match e.data {
            Some(data) => RpcResponse::error_with_data(id, e.code, e.message, data),
            None => RpcResponse::error(id, e.code, e.message),
        },
    };

    Some(serde_json::to_string(&resp).unwrap())
}

/// Handle `initialize` — return protocol version, capabilities, and context ID.
fn handle_initialize(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let _params: InitializeParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    // Ask the server for the active pane's context ID.
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendInitialize { resp: resp_tx })
        .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;
    let self_context_id = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    let result = InitializeResult {
        protocol_version: "2".into(),
        capabilities: vec!["events".into(), "capture".into(), "run_shell".into()],
        self_context_id,
    };

    serde_json::to_value(result).map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}

/// Handle `spawn_agent` — create a new pane with the given command.
fn handle_spawn_agent(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: SpawnAgentParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    if p.command.is_empty() {
        return Err(RpcErr::from((-32602, "command must not be empty".into())));
    }

    let metadata = p.metadata;
    let split_direction = match p.split_direction.as_deref() {
        Some("horizontal") => Some(crate::types::LayoutKind::Horizontal),
        Some("vertical") => Some(crate::types::LayoutKind::Vertical),
        Some(_) => {
            return Err(RpcErr::from((
                -32602,
                "split_direction must be \"horizontal\" or \"vertical\"".into(),
            )))
        }
        None => None,
    };

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendSpawnAgent {
        command: p.command,
        cwd: p.cwd,
        env: p.env,
        metadata,
        split_direction,
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let context_id = resp_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    if let Some(err_msg) = context_id.strip_prefix("ERROR:") {
        let code = if err_msg.contains("too small") {
            PANE_TOO_SMALL
        } else {
            SPAWN_FAILED
        };
        return Err(RpcErr::from((code, err_msg.to_string())));
    }

    // --- Readiness polling (runs in dispatcher thread, NOT server loop) ---
    let ready;
    let elapsed_ms;
    let data_version;

    if p.wait_ready {
        let timeout_ms = p.ready_timeout_ms.unwrap_or(15000) as u64;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();

        if let Some(pid) = parse_pane_id(&context_id) {
            // NOTE: We reuse one (qtx, qrx) channel across loop iterations.
            // If the server is slow, old responses may queue in qrx. This is
            // acceptable because data_version and last_output_time only ever
            // increase — a stale response just delays detection by one iteration.
            let (qtx, qrx) = mpsc::channel::<(u64, u64)>();
            loop {
                let qtx2 = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx2));
                if let Ok((dv, lot)) = qrx.recv_timeout(std::time::Duration::from_secs(2)) {
                    if dv > 0 && lot > 0 {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        if now_ms.saturating_sub(lot) >= 500 {
                            ready = true;
                            elapsed_ms = start.elapsed().as_millis() as u64;
                            data_version = dv;
                            break;
                        }
                    }
                }
                if std::time::Instant::now() >= deadline {
                    let qtx_final = qtx.clone();
                    let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx_final));
                    data_version = qrx
                        .recv_timeout(std::time::Duration::from_millis(500))
                        .map(|(dv, _)| dv)
                        .unwrap_or(0);
                    elapsed_ms = start.elapsed().as_millis() as u64;
                    ready = false;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        } else {
            ready = false;
            elapsed_ms = 0;
            data_version = 0;
        }

        if !ready {
            return Err(RpcErr {
                code: SPAWN_TIMEOUT,
                message: format!("Pane spawned but not ready within {}ms", timeout_ms),
                data: Some(serde_json::json!({ "context_id": context_id })),
            });
        }
    } else {
        ready = false;
        elapsed_ms = 0;
        data_version = 0;
    }

    let result = SpawnAgentResult {
        context_id,
        ready,
        elapsed_ms,
        data_version,
    };
    serde_json::to_value(result).map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}

/// Handle `write` — validate base64 payload and forward to pane.
fn handle_write(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: WriteParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    // Validate and decode base64.
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&p.data)
        .map_err(|e| RpcErr::from((-32602, format!("Invalid base64: {e}"))))?;

    let text = String::from_utf8(decoded)
        .map_err(|e| RpcErr::from((-32602, format!("Invalid UTF-8: {e}"))))?;

    // Send text to the pane and wait for confirmation.
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendSendText {
        pane_id: p.context_id.clone(),
        text,
        resp: Some(resp_tx),
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    if let Ok(false) = resp_rx.recv_timeout(std::time::Duration::from_secs(2)) {
        return Err(RpcErr {
            code: PANE_NOT_FOUND,
            message: format!("Pane not found: {}", p.context_id),
            data: Some(serde_json::json!({ "context_id": p.context_id })),
        });
    }

    Ok(serde_json::json!({}))
}

/// Handle `capture` — return pane content.
fn handle_capture(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    let context_id = p.context_id.clone();

    // --- Freshness polling (dispatcher thread) ---
    if p.wait_for_output || p.since_version.is_some() {
        if let Some(pid) = parse_pane_id(&context_id) {
            // Early pane existence check
            {
                let (check_tx, check_rx) = mpsc::channel();
                let _ = tx.send(CtrlReq::BackendCapturePane {
                    pane_id: context_id.clone(),
                    lines: Some(1),
                    clean: false,
                    resp: check_tx,
                });
                if let Ok(text) = check_rx.recv_timeout(std::time::Duration::from_secs(2)) {
                    if text == "__PANE_NOT_FOUND__" {
                        return Err(RpcErr {
                            code: PANE_NOT_FOUND,
                            message: format!("Pane not found: {}", context_id),
                            data: Some(serde_json::json!({ "context_id": context_id })),
                        });
                    }
                }
            }

            // Get baseline data_version
            let baseline = if let Some(sv) = p.since_version {
                sv
            } else {
                let (qtx, qrx) = mpsc::channel();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx));
                qrx.recv_timeout(std::time::Duration::from_millis(500))
                    .map(|(dv, _)| dv)
                    .unwrap_or(0)
            };

            let timeout_ms_val = p.timeout_ms.unwrap_or(5000) as u64;
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms_val);
            let (qtx, qrx) = mpsc::channel::<(u64, u64)>();
            let mut timed_out = true;

            loop {
                let qtx2 = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx2));
                if let Ok((dv, _)) = qrx.recv_timeout(std::time::Duration::from_secs(2)) {
                    if dv > baseline {
                        timed_out = false;
                        break;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            // Even on timeout, we still capture (stale content in error data)
            if timed_out {
                let (resp_tx, resp_rx) = mpsc::channel();
                let _ = tx.send(CtrlReq::BackendCapturePane {
                    pane_id: p.context_id,
                    lines: p.lines,
                    clean: p.clean.unwrap_or(false),
                    resp: resp_tx,
                });
                let text = resp_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or_default();

                let qtx_final = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx_final));
                let current_dv = qrx
                    .recv_timeout(std::time::Duration::from_millis(500))
                    .map(|(dv, _)| dv)
                    .unwrap_or(0);

                return Err(RpcErr {
                    code: CAPTURE_TIMEOUT,
                    message: format!("No new output within {}ms", timeout_ms_val),
                    data: Some(serde_json::json!({
                        "text": text,
                        "data_version": current_dv,
                        "context_id": context_id,
                    })),
                });
            }
        }
    }

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendCapturePane {
        pane_id: p.context_id,
        lines: p.lines,
        clean: p.clean.unwrap_or(false),
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let text = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    // Check for PANE_NOT_FOUND sentinel
    if text == "__PANE_NOT_FOUND__" {
        return Err(RpcErr::from((
            PANE_NOT_FOUND,
            format!("Pane not found: {}", context_id),
        )));
    }

    // Read data_version for the response
    let data_version = if let Some(pid) = parse_pane_id(&context_id) {
        let (qtx, qrx) = mpsc::channel();
        let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx));
        qrx.recv_timeout(std::time::Duration::from_millis(500))
            .map(|(dv, _)| dv)
            .unwrap_or(0)
    } else {
        0
    };

    let result = CaptureResult {
        text,
        data_version,
        context_id,
    };
    serde_json::to_value(result).map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}

/// Handle `kill` — terminate a context.
fn handle_kill(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: KillParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendKillPane {
        pane_id: p.context_id,
        grace_ms: p.grace_ms,
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    // Wait for acknowledgement.
    let _ = resp_rx.recv_timeout(std::time::Duration::from_secs(5));

    Ok(serde_json::json!({}))
}

/// Handle `kill_all` — kill all agent-spawned panes, optionally filtered by role.
fn handle_kill_all(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: KillAllParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendKillAll {
        role: p.role,
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let killed = resp_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    let result = KillAllResult { killed };
    serde_json::to_value(result).map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}

/// Handle `list` — return all active contexts.
fn handle_list(
    _params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendListPanes { resp: resp_tx })
        .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let json_str = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    // The server returns a pre-serialized JSON string; parse it back to Value.
    serde_json::from_str(&json_str)
        .map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}

/// Handle `run_shell` — spawn a process server-side and collect its output.
///
/// Resolves the working directory from: explicit `cwd` param > pane `spawn_cwd` > none.
/// Uses a poll-based timeout loop (try_wait) to keep the child handle available for
/// `kill()` on timeout, avoiding the ownership problem with `wait_with_output()`.
fn handle_run_shell(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: RunShellParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    if p.command.is_empty() {
        return Err(RpcErr::from((-32602, "command must not be empty".to_string())));
    }

    let timeout_ms = p.timeout_ms.unwrap_or(30000) as u64;

    // Resolve working directory: explicit cwd > pane spawn_cwd > none
    let cwd = if let Some(ref explicit_cwd) = p.cwd {
        Some(std::path::PathBuf::from(explicit_cwd))
    } else if p.context_id.is_some() {
        let (resp_tx, resp_rx) = mpsc::channel();
        tx.send(CtrlReq::BackendRunShell {
            context_id: p.context_id.clone(),
            resp: resp_tx,
        })
        .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

        resp_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?
    } else {
        None
    };

    // Build command
    let mut cmd = std::process::Command::new(&p.command[0]);
    if p.command.len() > 1 {
        cmd.args(&p.command[1..]);
    }
    if let Some(ref dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(ref env_vars) = p.env {
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Spawn
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().map_err(|e| RpcErr {
        code: COMMAND_FAILED,
        message: format!("Failed to spawn: {e}"),
        data: Some(serde_json::json!({ "command": p.command })),
    })?;

    // Take stdout/stderr pipes before poll loop
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    // Poll-based timeout with try_wait — keeps child handle for kill()
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();
                if let Some(ref mut pipe) = stdout_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut stdout_buf);
                }
                if let Some(ref mut pipe) = stderr_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut stderr_buf);
                }
                let result = RunShellResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
                    stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                };
                return serde_json::to_value(result)
                    .map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))));
            }
            Ok(None) => {
                if start.elapsed().as_millis() as u64 > timeout_ms {
                    let _ = child.kill();
                    let mut stdout_buf = Vec::new();
                    let mut stderr_buf = Vec::new();
                    if let Some(ref mut pipe) = stdout_pipe {
                        use std::io::Read;
                        let _ = pipe.read_to_end(&mut stdout_buf);
                    }
                    if let Some(ref mut pipe) = stderr_pipe {
                        use std::io::Read;
                        let _ = pipe.read_to_end(&mut stderr_buf);
                    }
                    return Err(RpcErr {
                        code: COMMAND_TIMEOUT,
                        message: format!("Command timed out after {}ms", timeout_ms),
                        data: Some(serde_json::json!({
                            "stdout": String::from_utf8_lossy(&stdout_buf),
                            "stderr": String::from_utf8_lossy(&stderr_buf),
                            "command": p.command,
                            "timeout_ms": timeout_ms,
                        })),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                return Err(RpcErr {
                    code: COMMAND_FAILED,
                    message: format!("Process error: {e}"),
                    data: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> mpsc::Sender<CtrlReq> {
        let (tx, _rx) = mpsc::channel();
        tx
    }

    #[test]
    fn dispatch_unknown_method_returns_error() {
        let tx = make_tx();
        let input = r#"{"id":"1","method":"nonexistent","params":{}}"#;
        let resp = dispatch_rpc(input, &tx).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn dispatch_malformed_json_returns_parse_error() {
        let tx = make_tx();
        let resp = dispatch_rpc("not json", &tx).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32700);
    }

    #[test]
    fn dispatch_write_rejects_bad_base64() {
        let tx = make_tx();
        let input =
            r#"{"id":"1","method":"write","params":{"context_id":"%1","data":"!!!not-base64!!!"}}"#;
        let resp = dispatch_rpc(input, &tx).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602);
    }

    #[test]
    fn dispatch_spawn_agent_rejects_empty_command() {
        let tx = make_tx();
        let input = r#"{"id":"1","method":"spawn_agent","params":{"command":[]}}"#;
        let resp = dispatch_rpc(input, &tx).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602);
    }
}
