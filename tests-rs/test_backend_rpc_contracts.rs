/// CustomPaneBackend JSON-RPC — boundary contract tests.
///
/// This is the one true module boundary in psmux: Claude Code's TeammateTool
/// and Pi's PsmuxAdapter speak to `dispatch_rpc` over a named pipe and never
/// link against this crate. Everything they rely on is wire shape, so wire
/// shape is what these tests pin.
///
/// Complements, and deliberately does not duplicate:
///   - `test_pi_integration_contracts.rs` — ContextInfo / ListResult shape,
///     env-var and pipe-path contracts
///   - the `#[cfg(test)]` module in `src/backend/dispatcher.rs` — unknown
///     method, parse error, bad base64, spawn handshake
///
/// Covered here: the three push events serialized from their real structs,
/// graceful rejection across the whole method surface, JSON-RPC envelope
/// invariants, and the metadata round-trip through pane storage.
use psmux::backend::protocol::{
    AgentMetadata, ContextExitedEvent, ContextExitedParams, ContextReadyEvent, ContextReadyParams,
    ExecCompletedEvent, ExecCompletedParams,
};
use std::collections::HashMap;

fn to_json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("event must serialize")
}

// ─── Push events ─────────────────────────────────────────────────────────────
//
// These are the asynchronous half of the boundary: the server pushes them
// unsolicited and a consumer dispatches on `method`. `test_feature_contracts.rs`
// checks these against hand-written `json!` literals, which cannot catch drift
// in the structs themselves — a renamed field there stays green. These build the
// real structs so a rename fails the build or the assertion.

#[test]
fn context_ready_event_wire_shape() {
    let json = to_json(&ContextReadyEvent {
        method: "context_ready".to_string(),
        params: ContextReadyParams {
            context_id: "%3".to_string(),
            ready_signal: "idle_prompt".to_string(),
            data_version: 7,
        },
    });

    assert_eq!(json["method"], "context_ready");
    assert_eq!(json["params"]["context_id"], "%3");
    assert_eq!(json["params"]["ready_signal"], "idle_prompt");
    assert_eq!(json["params"]["data_version"], 7);
}

#[test]
fn exec_completed_event_wire_shape() {
    let json = to_json(&ExecCompletedEvent {
        method: "exec_completed".to_string(),
        params: ExecCompletedParams {
            context_id: "%3".to_string(),
            exit_code: 1,
            command: "cargo build".to_string(),
            elapsed_ms: 1234,
        },
    });

    assert_eq!(json["method"], "exec_completed");
    assert_eq!(json["params"]["exit_code"], 1);
    assert_eq!(json["params"]["command"], "cargo build");
    assert_eq!(json["params"]["elapsed_ms"], 1234);
}

#[test]
fn context_exited_omits_the_enriched_fields_when_absent() {
    let json = to_json(&ContextExitedEvent {
        method: "context_exited".to_string(),
        params: ContextExitedParams {
            context_id: "%3".to_string(),
            exit_code: Some(0),
            elapsed_ms: None,
            command: None,
        },
    });

    let params = json["params"].as_object().expect("params is an object");
    assert!(!params.contains_key("elapsed_ms"), "absent means absent");
    assert!(!params.contains_key("command"), "absent means absent");
}

#[test]
fn context_exited_carries_the_enriched_fields_when_present() {
    let json = to_json(&ContextExitedEvent {
        method: "context_exited".to_string(),
        params: ContextExitedParams {
            context_id: "%3".to_string(),
            exit_code: Some(2),
            elapsed_ms: Some(500),
            command: Some("pytest".to_string()),
        },
    });

    assert_eq!(json["params"]["elapsed_ms"], 500);
    assert_eq!(json["params"]["command"], "pytest");
}

/// `exit_code` is the one Option on this event with no `skip_serializing_if`,
/// so a pane that died without an exit status sends an explicit `null` rather
/// than omitting the key. A consumer that treats "missing" and "null" alike is
/// fine; one that assumes the key is always an integer is not.
#[test]
fn context_exited_sends_a_null_exit_code_rather_than_omitting_it() {
    let json = to_json(&ContextExitedEvent {
        method: "context_exited".to_string(),
        params: ContextExitedParams {
            context_id: "%3".to_string(),
            exit_code: None,
            elapsed_ms: None,
            command: None,
        },
    });

    let params = json["params"].as_object().expect("params is an object");
    assert!(params.contains_key("exit_code"), "the key must be present");
    assert!(json["params"]["exit_code"].is_null());
}

// ─── Graceful rejection across the method surface ────────────────────────────
//
// The contract a client depends on is that every method answers. A panic in a
// handler takes down the pipe thread and the client hangs forever with no
// error to act on, which is strictly worse than any error code.
//
// Params that fail to deserialize are rejected before any send; the ones that
// reach a send are answered by a pump that never replies, so nothing blocks.

/// Every method the dispatcher routes (`dispatcher.rs`), with params that are
/// valid JSON but wrong for that method.
const EVERY_METHOD: [&str; 11] = [
    "initialize",
    "spawn_agent",
    "write",
    "capture",
    "kill",
    "kill_all",
    "set_metadata",
    "list",
    "run_shell",
    "exec",
    "wait_for",
];

/// Dispatch against a server that receives requests and answers none.
///
/// The pump drops each `CtrlReq`, which drops the reply sender inside it, so
/// every handler's `recv` fails as disconnected straight away. Simply dropping
/// the receiver instead would leave the handlers that ignore a failed `send`
/// waiting out their full 2–5s timeouts, which cost this file 50s of CI time
/// and — worse — let a rejection assertion pass on a timeout.
fn dispatch_against_silent_server(line: &str) -> serde_json::Value {
    dispatch_with_pump(line, |_req| {})
}

/// Dispatch against a server that answers a capture with the "no such pane"
/// sentinel, exactly as the real server loop does.
fn dispatch_against_pane_not_found(line: &str) -> serde_json::Value {
    dispatch_with_pump(line, |req| {
        if let psmux::types::CtrlReq::BackendCapturePane { resp, .. } = req {
            let _ = resp.send("__PANE_NOT_FOUND__".to_string());
        }
    })
}

fn dispatch_with_pump(
    line: &str,
    answer: impl Fn(psmux::types::CtrlReq) + Send + 'static,
) -> serde_json::Value {
    let (tx, rx) = std::sync::mpsc::channel();
    let pump = std::thread::spawn(move || {
        while let Ok(req) = rx.recv() {
            answer(req);
        }
    });

    let resp = psmux::backend::dispatcher::dispatch_rpc(line, &tx)
        .expect("a request carrying an id always gets a response");

    drop(tx);
    pump.join().expect("the pump thread must not panic");
    serde_json::from_str(&resp).expect("the response is always valid JSON")
}

#[test]
fn every_method_answers_an_empty_param_object_with_an_error() {
    for method in EVERY_METHOD {
        let line = format!(r#"{{"id":"1","method":"{method}","params":{{}}}}"#);
        let v = dispatch_against_silent_server(&line);

        assert!(
            v["error"]["code"].is_i64(),
            "{method} must answer with a numeric error code, got {v}"
        );
        assert!(
            v["result"].is_null(),
            "{method} must not report a result alongside an error, got {v}"
        );
        assert_eq!(v["id"], "1", "{method} must echo the request id");
    }
}

#[test]
fn every_method_answers_a_wrongly_typed_param_object_with_an_error() {
    for method in EVERY_METHOD {
        // Params as an array rather than an object: valid JSON, wrong shape.
        let line = format!(r#"{{"id":7,"method":"{method}","params":[1,2,3]}}"#);
        let v = dispatch_against_silent_server(&line);

        assert!(
            v["error"]["code"].is_i64(),
            "{method} must answer with a numeric error code, got {v}"
        );
        assert_eq!(v["id"], 7, "{method} must echo a numeric id unchanged");
    }
}

/// `context_id` is a `%N` string. Anything else must reach the server as an
/// unknown pane and come back as PANE_NOT_FOUND — never coerced into a pane
/// index, and never dressed up as an internal error.
///
/// Asserting the specific code matters: against a server that simply never
/// answers, every one of these "passes" on a timeout-shaped -32603, which
/// would stay green even if `%abc` started resolving to pane 0.
#[test]
fn malformed_context_ids_report_pane_not_found_rather_than_being_coerced() {
    for bad in ["", "1", "%", "%abc", "%-1", "%1.5", "% 1"] {
        let line = format!(r#"{{"id":"1","method":"capture","params":{{"context_id":"{bad}"}}}}"#);
        let v = dispatch_against_pane_not_found(&line);

        assert_eq!(
            v["error"]["code"],
            psmux::backend::protocol::PANE_NOT_FOUND,
            "context_id {bad:?} should be reported as a missing pane, got {v}"
        );
    }
}

// ─── JSON-RPC envelope invariants ────────────────────────────────────────────

/// A request that cannot be parsed has no id to echo, so the error must carry
/// a null id — a client correlating responses by id would otherwise never
/// resolve the promise it is holding.
#[test]
fn a_parse_error_answers_with_a_null_id() {
    let v = dispatch_against_silent_server("{ this is not json");

    assert_eq!(v["error"]["code"], -32700);
    assert!(v["id"].is_null(), "unparseable requests answer id: null");
}

#[test]
fn an_unknown_method_echoes_the_id_it_was_given() {
    let v =
        dispatch_against_silent_server(r#"{"id":"abc-123","method":"no_such_method","params":{}}"#);

    assert_eq!(v["error"]["code"], -32601);
    assert_eq!(v["id"], "abc-123");
}

/// The declared error codes are part of the published protocol
/// (`docs/custompane-backend.md`); a client switches on them.
#[test]
fn the_declared_error_codes_keep_their_values() {
    use psmux::backend::protocol as p;
    assert_eq!(p::PANE_NOT_FOUND, -32001);
    assert_eq!(p::SPAWN_FAILED, -32002);
    assert_eq!(p::PANE_TOO_SMALL, -32003);
    assert_eq!(p::SPAWN_TIMEOUT, -32004);
    assert_eq!(p::CAPTURE_TIMEOUT, -32005);
    assert_eq!(p::SESSION_NOT_FOUND, -32006);
    assert_eq!(p::COMMAND_TIMEOUT, -32007);
    assert_eq!(p::COMMAND_FAILED, -32008);
}

// ─── Metadata round-trip through pane storage ────────────────────────────────
//
// `AgentMetadata` does not reach a pane as JSON. It is flattened into the
// pane's `HashMap<String, String>` by `apply_to` and rebuilt by
// `from_metadata_map` when `list` reports the pane back. That flattening is a
// serialization boundary like any other, and it is lossy in ways JSON is not.

fn round_trip(meta: &AgentMetadata) -> Option<AgentMetadata> {
    let mut map = HashMap::new();
    meta.apply_to(&mut map);
    AgentMetadata::from_metadata_map(&map)
}

#[test]
fn metadata_survives_the_round_trip_through_pane_storage() {
    let original = AgentMetadata {
        name: Some("reviewer".to_string()),
        color: Some("cyan".to_string()),
        role: Some("critic".to_string()),
        effort: Some("high".to_string()),
        max_turns: Some(12),
        disallowed_tools: Some(vec!["Bash".to_string(), "Write".to_string()]),
    };

    let back = round_trip(&original).expect("a populated map rebuilds");

    assert_eq!(back.name, original.name);
    assert_eq!(back.color, original.color);
    assert_eq!(back.role, original.role);
    assert_eq!(back.effort, original.effort);
    assert_eq!(back.max_turns, original.max_turns);
    assert_eq!(back.disallowed_tools, original.disallowed_tools);
}

/// The empty-vs-missing distinction, which is where flattening to a joined
/// string is easiest to get wrong: `[].join(",")` is `""`, and `"".split(',')`
/// yields one empty element, so a naive round trip turns "no tools are
/// disallowed" into "the tool named empty-string is disallowed".
#[test]
fn an_empty_disallowed_tools_list_does_not_become_a_list_of_one_empty_name() {
    let original = AgentMetadata {
        name: Some("worker".to_string()),
        color: None,
        role: None,
        effort: None,
        max_turns: None,
        disallowed_tools: Some(vec![]),
    };

    let back = round_trip(&original).expect("a populated map rebuilds");

    assert_eq!(
        back.disallowed_tools,
        Some(vec![]),
        "an empty restriction list must stay empty"
    );
}

#[test]
fn metadata_with_no_fields_set_rebuilds_as_none() {
    let empty = AgentMetadata {
        name: None,
        color: None,
        role: None,
        effort: None,
        max_turns: None,
        disallowed_tools: None,
    };

    assert!(
        round_trip(&empty).is_none(),
        "a pane with no metadata reports None, not an all-None struct"
    );
}

/// `max_turns` is the only numeric field, so it is the only one that can fail
/// to parse back out of its string form.
#[test]
fn an_unparseable_max_turns_reads_back_as_none_rather_than_panicking() {
    let mut map = HashMap::new();
    map.insert("@agent".to_string(), "worker".to_string());
    map.insert("@max_turns".to_string(), "not-a-number".to_string());

    let back = AgentMetadata::from_metadata_map(&map).expect("a populated map rebuilds");

    assert_eq!(back.name.as_deref(), Some("worker"));
    assert_eq!(back.max_turns, None);
}
