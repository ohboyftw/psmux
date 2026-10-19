//! Topic names, topic patterns, and the one matcher used for both live
//! delivery and replay.
//!
//! Rules (MQTT 3.1.1 section 4.7, plus one mycel compatibility rule):
//! - Levels are separated by `/`. Trailing `/`s are ignored, so `a/b/`
//!   names the same topic as `a/b` (the C++ server ignored one). Validation
//!   returns this canonical form, without trailing `/`s, and every buffer,
//!   sequence counter and subscription must be keyed by it, so `a/b/` and
//!   `a/b` share one history. A string of only `/`s is empty. A leading
//!   `/` gives an empty first level, so `/a` is not `a`.
//! - `+` matches exactly one level (an empty level too). `#` matches zero
//!   or more trailing levels, so `canopy/#` matches `canopy`.
//! - Wildcards must be whole levels and `#` must be the last level;
//!   anything else (`a+`, `a/#/b`) is rejected rather than reinterpreted.
//! - A wildcard in the first level never matches a topic that starts with
//!   `$`. This covers `+/...` as well as `#`, and applies to replay too.
//!   The C++ server only excluded a root `#`, and its replay excluded
//!   nothing, so `replay '#'` used to return `$mycel/*` events.

use std::fmt;

/// Longest accepted topic or pattern, in bytes (the MQTT limit).
pub const MAX_TOPIC_LEN: usize = 65_535;

/// Why a topic or pattern was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicError {
    /// The string is empty.
    Empty,
    /// The string is longer than [`MAX_TOPIC_LEN`] bytes.
    TooLong,
    /// The string contains a NUL character.
    Nul,
    /// A concrete topic contains `+` or `#`.
    WildcardInTopic,
    /// `+` or `#` shares a level with other characters.
    PartialWildcard,
    /// `#` is followed by more levels.
    HashNotLast,
}

impl fmt::Display for TopicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TopicError::Empty => "topic is empty",
            TopicError::TooLong => "topic is longer than 65535 bytes",
            TopicError::Nul => "topic contains NUL",
            TopicError::WildcardInTopic => "topic contains a wildcard",
            TopicError::PartialWildcard => "wildcard must be a whole level",
            TopicError::HashNotLast => "'#' must be the last level",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TopicError {}

/// The canonical form of a topic or pattern: `s` without trailing `/`s.
pub fn canonical(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// Levels of a canonical topic or pattern. Empty levels are kept.
fn levels(canonical: &str) -> std::str::Split<'_, char> {
    canonical.split('/')
}

/// True for `$`-prefixed topics such as `$mycel/clients/connected`.
pub fn is_system_topic(topic: &str) -> bool {
    topic.starts_with('$')
}

/// Check a concrete topic used in a publish and return its canonical form.
pub fn validate_topic(topic: &str) -> Result<&str, TopicError> {
    let topic = check_common(topic)?;
    if topic.contains(['+', '#']) {
        return Err(TopicError::WildcardInTopic);
    }
    Ok(topic)
}

/// Check a subscribe, unsubscribe or replay pattern and return its
/// canonical form.
pub fn validate_pattern(pattern: &str) -> Result<&str, TopicError> {
    let pattern = check_common(pattern)?;
    let mut rest = levels(pattern).peekable();
    while let Some(level) = rest.next() {
        let wild = level.contains(['+', '#']);
        if wild && level != "+" && level != "#" {
            return Err(TopicError::PartialWildcard);
        }
        if level == "#" && rest.peek().is_some() {
            return Err(TopicError::HashNotLast);
        }
    }
    Ok(pattern)
}

fn check_common(s: &str) -> Result<&str, TopicError> {
    if s.len() > MAX_TOPIC_LEN {
        return Err(TopicError::TooLong);
    }
    if s.contains('\0') {
        return Err(TopicError::Nul);
    }
    match canonical(s) {
        "" => Err(TopicError::Empty),
        c => Ok(c),
    }
}

/// Does `topic` match `pattern`? Both are assumed valid; trailing `/`s are
/// ignored. Iterative, so a topic with tens of thousands of levels cannot
/// overflow the stack.
pub fn matches(pattern: &str, topic: &str) -> bool {
    let (pattern, topic) = (canonical(pattern), canonical(topic));
    let first = levels(pattern).next();
    if is_system_topic(topic) && matches!(first, Some("+" | "#")) {
        return false;
    }
    let (mut p, mut t) = (levels(pattern), levels(topic));
    loop {
        match (p.next(), t.next()) {
            (Some("#"), _) | (None, None) => return true,
            (Some("+"), Some(_)) => {}
            (Some(pl), Some(tl)) if pl == tl => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_topic.rs"]
mod tests;
