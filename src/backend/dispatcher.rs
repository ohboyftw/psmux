//! JSON-RPC dispatcher: routes method calls to handler functions.
//!
//! Each handler validates its parameters, sends the appropriate CtrlReq to
//! the server's main loop, waits for the response, and returns a
//! `serde_json::Value` result or an `(error_code, message)` tuple.

use std::sync::mpsc;

use crate::types::CtrlReq;

use super::protocol::*;

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
        "list" => handle_list(&req.params, tx),
        _ => Err((-32601, format!("Method not found: {}", req.method))),
    };

    let resp = match result {
        Ok(value) => RpcResponse::success(id, value),
        Err((code, msg)) => RpcResponse::error(id, code, msg),
    };

    Some(serde_json::to_string(&resp).unwrap())
}

/// Handle `initialize` — return protocol version, capabilities, and context ID.
fn handle_initialize(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let _params: InitializeParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Ask the server for the active pane's context ID.
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendInitialize { resp: resp_tx })
        .map_err(|_| (-32603, "Server channel closed".to_string()))?;
    let self_context_id = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Server response timeout".to_string()))?;

    let result = InitializeResult {
        protocol_version: "1".into(),
        capabilities: vec!["events".into(), "capture".into()],
        self_context_id,
    };

    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `spawn_agent` — create a new pane with the given command.
fn handle_spawn_agent(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: SpawnAgentParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    if p.command.is_empty() {
        return Err((-32602, "command must not be empty".into()));
    }

    let metadata = p.metadata.map(|m| (m.name, m.role));

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendSpawnAgent {
        command: p.command,
        cwd: p.cwd,
        env: p.env,
        metadata,
        resp: resp_tx,
    })
    .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    let context_id = resp_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| (-32603, "Server response timeout".to_string()))?;

    if let Some(err_msg) = context_id.strip_prefix("ERROR:") {
        return Err((-32603, err_msg.to_string()));
    }

    let result = SpawnAgentResult { context_id };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `write` — validate base64 payload and forward to pane.
fn handle_write(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: WriteParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Validate and decode base64.
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&p.data)
        .map_err(|e| (-32602, format!("Invalid base64: {e}")))?;

    let text = String::from_utf8(decoded).map_err(|e| (-32602, format!("Invalid UTF-8: {e}")))?;

    // Fire-and-forget: send text to the pane.
    tx.send(CtrlReq::BackendSendText {
        pane_id: p.context_id,
        text,
    })
    .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    Ok(serde_json::json!({}))
}

/// Handle `capture` — return pane content.
fn handle_capture(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendCapturePane {
        pane_id: p.context_id,
        lines: p.lines,
        clean: false,
        resp: resp_tx,
    })
    .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    let text = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Server response timeout".to_string()))?;

    let result = CaptureResult {
        truncated: false,
        text,
    };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `kill` — terminate a context.
fn handle_kill(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: KillParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendKillPane {
        pane_id: p.context_id,
        resp: resp_tx,
    })
    .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    // Wait for acknowledgement.
    let _ = resp_rx.recv_timeout(std::time::Duration::from_secs(5));

    Ok(serde_json::json!({}))
}

/// Handle `list` — return all active contexts.
fn handle_list(
    _params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendListPanes { resp: resp_tx })
        .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    let json_str = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Server response timeout".to_string()))?;

    // The server returns a pre-serialized JSON string; parse it back to Value.
    serde_json::from_str(&json_str).map_err(|e| (-32603, format!("Internal error: {e}")))
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
