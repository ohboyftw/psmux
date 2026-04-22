//! Tool handlers and wire types for the MCP server.

use psmux_fleet::{FleetError, Workspace, WorkspaceTable};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct ListParams {
    #[serde(default)]
    pub session: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub workspaces: Vec<Workspace>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct StatusParams {
    pub id: String,
    #[serde(default)]
    pub session: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
}

pub fn list(state_path: &Path, _params: ListParams) -> Result<ListResult, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    Ok(ListResult {
        workspaces: table.workspaces().to_vec(),
        total: table.len(),
    })
}

pub fn status(state_path: &Path, params: StatusParams) -> Result<Workspace, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    table
        .find(&params.id)
        .cloned()
        .ok_or(FleetError::WorkspaceNotFound(params.id))
}
