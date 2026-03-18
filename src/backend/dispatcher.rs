//! JSON-RPC dispatcher: routes method calls to handler functions.
//!
//! Each handler validates its parameters, performs the operation (or stubs it
//! for Task 8 wiring), and returns a `serde_json::Value` result or an
//! `(error_code, message)` tuple.  The top-level `dispatch_rpc` function
//! parses incoming JSON, selects the handler, and serialises the response.

use std::sync::mpsc;

use crate::types::CtrlReq;

use super::protocol::*;

/// Dispatch a single newline-delimited JSON-RPC request.
///
/// Returns `Some(json_string)` with the response to send back to the client,
/// or `None` if the request is a notification (no `id`) — though the current
/// protocol doesn't use notifications.
pub fn dispatch_rpc(
    line: &str,
    _tx: &mpsc::Sender<CtrlReq>,
) -> Option<String> {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp = RpcResponse::error(
                serde_json::Value::Null,
                -32700,
                format!("Parse error: {e}"),
            );
            return Some(serde_json::to_string(&resp).unwrap());
        }
    };

    let id = req.id.clone().unwrap_or(serde_json::Value::Null);

    let result = match req.method.as_str() {
        "initialize" => handle_initialize(&req.params),
        "spawn_agent" => handle_spawn_agent(&req.params),
        "write" => handle_write(&req.params),
        "capture" => handle_capture(&req.params),
        "kill" => handle_kill(&req.params),
        "list" => handle_list(&req.params),
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
) -> Result<serde_json::Value, (i32, String)> {
    let _params: InitializeParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Task 8 will provide a real pane ID via CtrlReq; for now return a
    // static self_context_id so the protocol round-trip is testable.
    let result = InitializeResult {
        protocol_version: "1".into(),
        capabilities: vec!["events".into(), "capture".into()],
        self_context_id: "%0".into(),
    };

    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `spawn_agent` — validate params and return a placeholder context ID.
fn handle_spawn_agent(
    params: &serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    let p: SpawnAgentParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    if p.command.is_empty() {
        return Err((-32602, "command must not be empty".into()));
    }

    // Task 8 will wire this to actually spawn a pane via CtrlReq.
    let result = SpawnAgentResult { context_id: "%1".into() };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `write` — validate base64 payload and forward to pane (stubbed).
fn handle_write(
    params: &serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    let p: WriteParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Validate that the data field is valid base64.
    use base64::Engine;
    let _decoded = base64::engine::general_purpose::STANDARD
        .decode(&p.data)
        .map_err(|e| (-32602, format!("Invalid base64: {e}")))?;

    // Task 8 will wire this to send text to the pane via CtrlReq.
    Ok(serde_json::json!({}))
}

/// Handle `capture` — return pane content (stubbed as empty).
fn handle_capture(
    params: &serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    let _p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Task 8 will wire this to capture pane content via CtrlReq.
    let result = CaptureResult { text: String::new(), truncated: false };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

/// Handle `kill` — terminate a context (stubbed).
fn handle_kill(
    params: &serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    let _p: KillParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Task 8 will wire this to kill the pane via CtrlReq.
    Ok(serde_json::json!({}))
}

/// Handle `list` — return all active contexts (stubbed as empty).
fn handle_list(
    _params: &serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    // Task 8 will wire this to list panes via CtrlReq.
    let result = ListResult { contexts: vec![] };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> mpsc::Sender<CtrlReq> {
        let (tx, _rx) = mpsc::channel();
        tx
    }

    #[test]
    fn dispatch_initialize_returns_protocol_version() {
        let tx = make_tx();
        let input = r#"{"id":"1","method":"initialize","params":{"protocol_version":"1","capabilities":["events"]}}"#;
        let resp = dispatch_rpc(input, &tx).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocol_version"], "1");
        assert_eq!(v["result"]["self_context_id"], "%0");
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
        let input = r#"{"id":"1","method":"write","params":{"context_id":"%1","data":"!!!not-base64!!!"}}"#;
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
