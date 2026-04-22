//! Workspace table — the cold-path query surface consumed by CLI and MCP.
//!
//! Cold hydration reads state.json only. Warm hydration (C2) will enrich
//! fields that require backend pipe, plan.json, and `@agent` metadata —
//! out of scope here.

use crate::{FleetError, StateReader, Workspace, WorkspaceStatus};
use std::path::Path;

/// Collection of workspaces with query helpers.
///
/// Tables are stateless snapshots — consumers call [`Self::hydrate_cold`] fresh
/// per invocation (spec §3). The table holds no reference to `state.json`
/// and will not observe on-disk updates.
///
/// Hydration modes:
/// - [`Self::hydrate_cold`] — state.json only.
/// - `hydrate_warm` (C2) — state.json + backend pipe + mycel.
#[derive(Debug)]
pub struct WorkspaceTable {
    workspaces: Vec<Workspace>,
}

impl WorkspaceTable {
    /// Cold hydration: reads state.json and projects each worker into a
    /// `Workspace`. Fields requiring backend/mycel data
    /// (branch, worktree_path, agent_kind, task, metadata) default here;
    /// warm hydration in C2 will enrich them.
    pub fn hydrate_cold(state_path: &Path) -> Result<Self, FleetError> {
        let reader = StateReader::load(state_path)?;
        Ok(Self {
            workspaces: reader.workspaces(),
        })
    }

    /// Number of workspaces in the table.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    /// `true` if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    /// All workspaces.
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Look up a workspace by id. `None` if not found.
    pub fn find(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// All workspaces matching the given status.
    pub fn filter_status(&self, status: WorkspaceStatus) -> Vec<&Workspace> {
        self.workspaces
            .iter()
            .filter(|w| w.status == status)
            .collect()
    }
}
