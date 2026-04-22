use crate::FleetError;

/// Resolves the target psmux session per spec §7.
///
/// Priority order:
/// 1. Explicit argument
/// 2. `$PSMUX_SESSION` environment variable
/// 3. Auto-detect via candidate list (from `psmux ls --json`)
pub struct SessionResolver {
    candidates: Vec<String>,
}

impl SessionResolver {
    /// Constructs a resolver with a pre-fetched candidate list.
    /// Callers performing real auto-detection should invoke `psmux ls --json`
    /// and pass the session names here.
    pub fn new_with_candidates(candidates: Vec<String>) -> Self {
        Self { candidates }
    }

    /// Resolves to a single session name or returns a typed error.
    pub fn resolve(&self, explicit: Option<&str>) -> Result<String, FleetError> {
        if let Some(s) = explicit {
            return Ok(s.to_string());
        }
        if let Ok(env) = std::env::var("PSMUX_SESSION") {
            if !env.is_empty() {
                return Ok(env);
            }
        }
        match self.candidates.len() {
            0 => Err(FleetError::SessionNotFound),
            1 => Ok(self.candidates[0].clone()),
            _ => Err(FleetError::SessionAmbiguous(self.candidates.clone())),
        }
    }
}
