//! Tool handlers and wire types for the MCP server.

use psmux_fleet::{FleetError, Workspace, WorkspaceTable};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Parameters for `fleet.list`.
#[derive(Debug, Deserialize, Default)]
pub struct ListParams {}

/// Response for `fleet.list`: an array of workspaces and their count.
#[derive(Debug, Serialize)]
pub struct ListResult {
    pub workspaces: Vec<Workspace>,
    pub total: usize,
}

/// Parameters for `fleet.status`: the workspace id to fetch.
#[derive(Debug, Deserialize)]
pub struct StatusParams {
    pub id: String,
}

/// JSON-RPC error envelope. Uses `FleetError::code()` string codes rather
/// than JSON-RPC 2.0's integer codes — matches the taxonomy in spec §9.
#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
}

/// Execute `fleet.list`: hydrate the workspace table and return all rows.
pub fn list(state_path: &Path, _params: ListParams) -> Result<ListResult, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    Ok(ListResult {
        workspaces: table.workspaces().to_vec(),
        total: table.len(),
    })
}

/// Execute `fleet.status`: hydrate the workspace table and return one row.
/// Returns `FleetError::WorkspaceNotFound` if `params.id` doesn't match.
pub fn status(state_path: &Path, params: StatusParams) -> Result<Workspace, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    table
        .find(&params.id)
        .cloned()
        .ok_or(FleetError::WorkspaceNotFound(params.id))
}
