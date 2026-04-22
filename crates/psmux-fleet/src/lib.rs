pub mod detector;
pub mod error;
pub mod model;
pub mod session;
pub mod state;
pub mod table;

#[cfg(feature = "fleet-claude-transcript")]
pub use detector::ClaudeTranscriptDetector;
pub use detector::{AwaitingDetector, RegexDetector};
pub use error::{ConflictKind, FleetError};
pub use model::{AgentKind, Workspace, WorkspaceStatus};
pub use session::SessionResolver;
pub use state::{update_state, StateReader};
pub use table::WorkspaceTable;
