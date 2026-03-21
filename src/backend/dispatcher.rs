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

    // Fire-and-forget: send text to the pane.
    tx.send(CtrlReq::BackendSendText {
        pane_id: p.context_id,
        text,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

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
