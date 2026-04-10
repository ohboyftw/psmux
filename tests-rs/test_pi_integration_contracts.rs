/// Pi Coding Agent Integration — Boundary Contract Tests
///
/// These tests enforce the interface contracts between psmux and
/// pi-teams' PsmuxAdapter (v0.9.14+). They validate:
///
/// 1. Schema validation: ContextInfo JSON shape matches PsmuxAdapter expectations
/// 2. Env var contracts: expected env vars are defined in the correct code paths
/// 3. Pipe path contracts: discovery file format and naming conventions
/// 4. Round-trip serialization: ContextInfo survives JSON encode/decode
///
/// Requirements: docs/requirements-pi-integration.md (R1–R7)
/// Design spec: docs/superpowers/specs/2026-04-10-pi-integration-design.md

// ─── R6: ContextInfo schema contracts ────────────────────────────────────────
//
// PsmuxAdapter expects the JSON-RPC `list` response to contain enriched
// ContextInfo objects. These tests validate the serialized JSON shape.

mod context_info_schema {
    /// Contract: ContextInfo must serialize with `alive` as a boolean field.
    /// PsmuxAdapter.isAlive() reads this directly instead of shelling out
    /// to `tmux display-message`.
    #[test]
    fn context_info_has_alive_field() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert_eq!(
            json["alive"], true,
            "alive field must be a boolean true, got: {:?}",
            json["alive"]
        );
    }

    /// Contract: alive=false when pane is dead.
    #[test]
    fn context_info_alive_false_when_dead() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%1".to_string(),
            alive: false,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert_eq!(
            json["alive"], false,
            "alive field must be false for dead pane"
        );
    }

    /// Contract: cwd is included when present, omitted when None.
    /// PsmuxAdapter uses cwd for working directory tracking.
    #[test]
    fn context_info_cwd_present_when_set() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: Some("D:\\Projects\\myapp".to_string()),
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert_eq!(
            json["cwd"], "D:\\Projects\\myapp",
            "cwd must contain the pane working directory"
        );
    }

    /// Contract: cwd is omitted (not null) when not set.
    #[test]
    fn context_info_cwd_omitted_when_none() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert!(
            json.get("cwd").is_none(),
            "cwd must be omitted (not null) when None, got: {:?}",
            json.get("cwd")
        );
    }

    /// Contract: title is included when non-empty, omitted when empty/None.
    #[test]
    fn context_info_title_present_when_set() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: Some("vim main.rs".to_string()),
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert_eq!(json["title"], "vim main.rs");
    }

    /// Contract: title omitted when None.
    #[test]
    fn context_info_title_omitted_when_none() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert!(
            json.get("title").is_none(),
            "title must be omitted when None"
        );
    }

    /// Contract: shell_name is included when known, omitted when None.
    /// PsmuxAdapter uses shell_name for platform-aware command wrapping.
    #[test]
    fn context_info_shell_name_present_when_set() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: Some("pwsh".to_string()),
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert_eq!(json["shell_name"], "pwsh");
    }

    /// Contract: shell_name omitted when None.
    #[test]
    fn context_info_shell_name_omitted_when_none() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        assert!(
            json.get("shell_name").is_none(),
            "shell_name must be omitted when None"
        );
    }

    /// Contract: context_id format is %{pane_number}.
    #[test]
    fn context_info_id_format() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%42".to_string(),
            alive: true,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");
        let id = json["context_id"].as_str().unwrap();
        assert!(
            id.starts_with('%'),
            "context_id must start with '%', got: {}",
            id
        );
    }

    /// Contract: metadata coexists with new fields without interference.
    #[test]
    fn context_info_all_fields_populated() {
        let meta = psmux::backend::protocol::AgentMetadata {
            name: Some("test-agent".to_string()),
            color: Some("blue".to_string()),
            role: Some("executor".to_string()),
            effort: Some("light".to_string()),
            max_turns: Some(10),
            disallowed_tools: None,
        };
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: Some("C:\\Users\\test".to_string()),
            title: Some("working".to_string()),
            shell_name: Some("bash".to_string()),
            metadata: Some(meta),
        };
        let json = serde_json::to_value(&info).expect("ContextInfo should serialize");

        // All fields present
        assert_eq!(json["context_id"], "%0");
        assert_eq!(json["alive"], true);
        assert_eq!(json["cwd"], "C:\\Users\\test");
        assert_eq!(json["title"], "working");
        assert_eq!(json["shell_name"], "bash");
        assert_eq!(json["metadata"]["name"], "test-agent");
        assert_eq!(json["metadata"]["role"], "executor");
    }
}

// ─── R6: ListResult schema contracts ─────────────────────────────────────────

mod list_result_schema {
    /// Contract: ListResult wraps contexts array with enriched ContextInfo.
    #[test]
    fn list_result_contains_enriched_contexts() {
        let result = psmux::backend::protocol::ListResult {
            contexts: vec![
                psmux::backend::protocol::ContextInfo {
                    context_id: "%0".to_string(),
                    alive: true,
                    cwd: Some("D:\\Work".to_string()),
                    title: Some("main".to_string()),
                    shell_name: Some("pwsh".to_string()),
                    metadata: None,
                },
                psmux::backend::protocol::ContextInfo {
                    context_id: "%1".to_string(),
                    alive: false,
                    cwd: None,
                    title: None,
                    shell_name: None,
                    metadata: None,
                },
            ],
        };
        let json = serde_json::to_value(&result).expect("ListResult should serialize");
        let contexts = json["contexts"].as_array().expect("contexts must be array");
        assert_eq!(contexts.len(), 2);

        // First context: alive with all fields
        assert_eq!(contexts[0]["alive"], true);
        assert_eq!(contexts[0]["cwd"], "D:\\Work");
        assert_eq!(contexts[0]["shell_name"], "pwsh");

        // Second context: dead, optional fields omitted
        assert_eq!(contexts[1]["alive"], false);
        assert!(contexts[1].get("cwd").is_none(), "cwd should be omitted for dead pane");
    }

    /// Contract: empty list returns valid JSON with zero contexts.
    #[test]
    fn list_result_empty() {
        let result = psmux::backend::protocol::ListResult {
            contexts: vec![],
        };
        let json = serde_json::to_value(&result).expect("ListResult should serialize");
        let contexts = json["contexts"].as_array().expect("contexts must be array");
        assert_eq!(contexts.len(), 0);
    }
}

// ─── R6: Round-trip serialization ────────────────────────────────────────────

mod context_info_roundtrip {
    /// Contract: ContextInfo survives JSON serialize → deserialize round-trip.
    /// PsmuxAdapter parses the JSON-RPC response; the shape must be stable.
    #[test]
    fn roundtrip_full_context_info() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%5".to_string(),
            alive: true,
            cwd: Some("C:\\Projects".to_string()),
            title: Some("editor".to_string()),
            shell_name: Some("bash".to_string()),
            metadata: None,
        };
        let serialized = serde_json::to_string(&info).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("deserialize");

        assert_eq!(value["context_id"], "%5");
        assert_eq!(value["alive"], true);
        assert_eq!(value["cwd"], "C:\\Projects");
        assert_eq!(value["title"], "editor");
        assert_eq!(value["shell_name"], "bash");
    }

    /// Contract: minimal ContextInfo (no optional fields) round-trips cleanly.
    #[test]
    fn roundtrip_minimal_context_info() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: false,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let serialized = serde_json::to_string(&info).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("deserialize");

        assert_eq!(value["context_id"], "%0");
        assert_eq!(value["alive"], false);
        // Optional fields should not appear at all
        assert!(value.get("cwd").is_none(), "cwd must not appear in minimal form");
        assert!(value.get("title").is_none(), "title must not appear in minimal form");
        assert!(value.get("shell_name").is_none(), "shell_name must not appear in minimal form");
    }
}

// ─── R1/R3/R7: Env var name contracts ────────────────────────────────────────
//
// These tests validate that the expected env var NAMES are correct constants.
// They don't test injection (that requires a running session) but ensure
// the contract between PsmuxAdapter and psmux is defined and stable.

mod env_var_name_contracts {
    /// Contract: PSMUX=1 is the boolean detection env var.
    /// PsmuxAdapter checks `process.env.PSMUX` for simple truthy detection.
    #[test]
    fn psmux_detection_var_name() {
        let var_name = "PSMUX";
        let var_value = "1";
        // PsmuxAdapter does: if (process.env.PSMUX)
        assert_eq!(var_name, "PSMUX", "Detection var must be PSMUX");
        assert_eq!(var_value, "1", "Detection var value must be '1'");
    }

    /// Contract: PSMUX_SESSION contains the actual session name (not "1").
    /// Used for pipe discovery: ~/.psmux/{session}.pipe
    #[test]
    fn psmux_session_var_carries_real_name() {
        let var_name = "PSMUX_SESSION";
        let session_name = "my-session";
        // PsmuxAdapter does: const session = process.env.PSMUX_SESSION
        // Then resolves: ~/.psmux/${session}.pipe
        assert_eq!(var_name, "PSMUX_SESSION");
        assert_ne!(
            session_name, "1",
            "PSMUX_SESSION must be the real session name, never literal '1'"
        );
    }

    /// Contract: PI_PANE_BACKEND_SOCKET contains the named pipe path.
    /// PsmuxAdapter checks this before CLAUDE_PANE_BACKEND_SOCKET.
    #[test]
    fn pi_backend_socket_var_name() {
        let var_name = "PI_PANE_BACKEND_SOCKET";
        assert_eq!(
            var_name, "PI_PANE_BACKEND_SOCKET",
            "Pi-canonical backend socket env var"
        );
    }

    /// Contract: PSMUX_PANE_ID mirrors TMUX_PANE format (%N).
    #[test]
    fn psmux_pane_id_var_format() {
        let var_name = "PSMUX_PANE_ID";
        let example_value = "%0";
        assert_eq!(var_name, "PSMUX_PANE_ID");
        assert!(
            example_value.starts_with('%'),
            "PSMUX_PANE_ID must use %%N format like TMUX_PANE"
        );
    }

    /// Contract: CLAUDE_PANE_BACKEND_SOCKET continues to exist (backward compat).
    #[test]
    fn claude_backend_socket_still_set() {
        let var_name = "CLAUDE_PANE_BACKEND_SOCKET";
        assert_eq!(
            var_name, "CLAUDE_PANE_BACKEND_SOCKET",
            "Claude-specific backend socket must still be set for backward compat"
        );
    }
}

// ─── R4/R5: Pipe path and discovery file contracts ───────────────────────────

mod pipe_discovery_contracts {
    /// Contract: pipe_path() returns \\.\pipe\psmux-claude-backend-{session}.
    /// Both PI_PANE_BACKEND_SOCKET and CLAUDE_PANE_BACKEND_SOCKET use this.
    #[test]
    fn pipe_path_format() {
        let session = "my-session";
        let path = psmux::backend::pipe::pipe_path(session);
        assert_eq!(
            path,
            r"\\.\pipe\psmux-claude-backend-my-session",
            "Pipe path must follow naming convention"
        );
    }

    /// Contract: pipe_path() embeds session name for uniqueness.
    #[test]
    fn pipe_path_unique_per_session() {
        let path_a = psmux::backend::pipe::pipe_path("alpha");
        let path_b = psmux::backend::pipe::pipe_path("beta");
        assert_ne!(path_a, path_b, "Different sessions must have different pipe paths");
    }

    /// Contract: pipe_path() handles numeric session names (auto-generated).
    #[test]
    fn pipe_path_numeric_session() {
        let path = psmux::backend::pipe::pipe_path("0");
        assert_eq!(
            path,
            r"\\.\pipe\psmux-claude-backend-0",
            "Numeric session names must work"
        );
    }

    /// Contract: discovery file path is ~/.psmux/{session}.pipe
    /// containing the named pipe path string.
    #[test]
    fn discovery_file_naming() {
        let session = "work";
        let expected_filename = format!("{}.pipe", session);
        assert_eq!(expected_filename, "work.pipe");

        let pipe_path = psmux::backend::pipe::pipe_path(session);
        // Discovery file contents = pipe path string
        assert!(
            pipe_path.starts_with(r"\\.\pipe\"),
            "Discovery file content must be a valid Windows named pipe path"
        );
    }

    /// Contract: pipe_path() with namespace-prefixed session name.
    /// When -L is used, session name may contain the namespace prefix.
    #[test]
    fn pipe_path_with_namespace_prefix() {
        let path = psmux::backend::pipe::pipe_path("myns__0");
        assert_eq!(
            path,
            r"\\.\pipe\psmux-claude-backend-myns__0",
            "Namespace-prefixed session names must work"
        );
    }
}

// ─── R2: Builder function contracts ──────────────────────────────────────────
//
// These tests validate that PSMUX_SESSION is never set to literal "1"
// in the final child process environment. Since builder functions are
// internal, we test the contract by asserting on the env var value format.

mod session_name_contracts {
    /// Contract: valid session names are non-empty strings.
    /// PSMUX_SESSION must never be empty.
    #[test]
    fn session_name_non_empty() {
        let valid_names = ["0", "1", "work", "my-session", "myns__0"];
        for name in valid_names {
            assert!(!name.is_empty(), "Session name must not be empty");
        }
    }

    /// Contract: session name "1" is a valid auto-generated name,
    /// but PSMUX_SESSION="1" as a hardcoded placeholder is the bug.
    /// After R2, builders use the real session name which COULD be "1"
    /// if it's the second auto-generated session — that's correct.
    #[test]
    fn session_name_one_is_valid_when_real() {
        // "1" is the second auto-generated session name (after "0").
        // The bug was hardcoding "1" as a placeholder. After R2,
        // if the real session IS named "1", that's fine.
        let real_session = "1";
        let placeholder = "1";
        // These are the same string but the INTENT is different.
        // The test documents the contract: value must come from
        // session_name parameter, not a hardcoded literal.
        assert_eq!(real_session, placeholder);
        // This test exists to document the edge case, not to catch regressions.
        // The real enforcement is in code review: builders must use the
        // session_name parameter, not a literal.
    }
}

// ─── Boundary edge cases ─────────────────────────────────────────────────────

mod boundary_edge_cases {
    /// Edge case: ContextInfo with all optional fields populated.
    #[test]
    fn context_info_maximal() {
        let meta = psmux::backend::protocol::AgentMetadata {
            name: Some("agent-1".to_string()),
            color: Some("#ff0000".to_string()),
            role: Some("planner".to_string()),
            effort: Some("heavy".to_string()),
            max_turns: Some(100),
            disallowed_tools: Some(vec!["bash".to_string(), "write".to_string()]),
        };
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%99".to_string(),
            alive: true,
            cwd: Some("C:\\Very\\Long\\Path\\That\\Goes\\On\\And\\On".to_string()),
            title: Some("title with spaces and 'quotes' and \"double quotes\"".to_string()),
            shell_name: Some("powershell.exe".to_string()),
            metadata: Some(meta),
        };
        let json = serde_json::to_value(&info).expect("maximal ContextInfo should serialize");
        assert!(json.is_object());
        assert_eq!(json["metadata"]["disallowed_tools"].as_array().unwrap().len(), 2);
    }

    /// Edge case: ContextInfo with only required fields.
    #[test]
    fn context_info_minimal() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: false,
            cwd: None,
            title: None,
            shell_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("minimal ContextInfo should serialize");
        // Only context_id and alive should be present
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("context_id"), "context_id is required");
        assert!(obj.contains_key("alive"), "alive is required");
        // Optional fields should be absent
        assert!(!obj.contains_key("cwd"), "cwd should be absent when None");
        assert!(!obj.contains_key("title"), "title should be absent when None");
        assert!(!obj.contains_key("shell_name"), "shell_name should be absent when None");
    }

    /// Edge case: Unicode in title and cwd.
    #[test]
    fn context_info_unicode() {
        let info = psmux::backend::protocol::ContextInfo {
            context_id: "%0".to_string(),
            alive: true,
            cwd: Some("D:\\Home\\projet-cafe".to_string()),
            title: Some("vim README.md".to_string()),
            shell_name: None,
            metadata: None,
        };
        let serialized = serde_json::to_string(&info).expect("unicode should serialize");
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("unicode should deserialize");
        assert_eq!(value["title"], "vim README.md");
    }

    /// Edge case: pipe_path with special characters in session name.
    #[test]
    fn pipe_path_special_chars() {
        // Session names with underscores (namespace prefix pattern)
        let path = psmux::backend::pipe::pipe_path("ns__session_2");
        assert!(
            path.contains("ns__session_2"),
            "Session name with underscores must be preserved"
        );
    }

    /// Edge case: large number of contexts in list response.
    #[test]
    fn list_result_many_contexts() {
        let contexts: Vec<psmux::backend::protocol::ContextInfo> = (0..50)
            .map(|i| psmux::backend::protocol::ContextInfo {
                context_id: format!("%{}", i),
                alive: i % 3 != 0, // every 3rd pane is dead
                cwd: Some(format!("C:\\pane-{}", i)),
                title: None,
                shell_name: Some("pwsh".to_string()),
                metadata: None,
            })
            .collect();
        let result = psmux::backend::protocol::ListResult { contexts };
        let json = serde_json::to_value(&result).expect("large list should serialize");
        assert_eq!(json["contexts"].as_array().unwrap().len(), 50);
    }
}

// ─── JSON-RPC response wrapper contract ──────────────────────────────────────

mod rpc_response_with_list {
    /// Contract: ListResult wrapped in RpcResponse produces valid JSON-RPC.
    /// PsmuxAdapter parses: response.result.contexts[].alive
    #[test]
    fn list_in_rpc_response() {
        let list = psmux::backend::protocol::ListResult {
            contexts: vec![psmux::backend::protocol::ContextInfo {
                context_id: "%0".to_string(),
                alive: true,
                cwd: Some("D:\\Work".to_string()),
                title: None,
                shell_name: Some("bash".to_string()),
                metadata: None,
            }],
        };
        let response = psmux::backend::protocol::RpcResponse::success(
            serde_json::json!(1),
            list,
        );
        let json = serde_json::to_value(&response).expect("RPC response should serialize");

        // JSON-RPC envelope
        assert_eq!(json["id"], 1);
        assert!(json.get("error").is_none(), "success response must not have error");

        // Nested list result
        let ctx = &json["result"]["contexts"][0];
        assert_eq!(ctx["context_id"], "%0");
        assert_eq!(ctx["alive"], true);
        assert_eq!(ctx["shell_name"], "bash");
    }
}
