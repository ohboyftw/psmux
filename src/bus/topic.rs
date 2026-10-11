//! Topic names, topic patterns, and the one matcher used for both live
//! delivery and replay.
//!
//! Rules (MQTT 3.1.1 section 4.7, plus one mycel compatibility rule):
//! - Levels are separated by `/`. One trailing `/` is ignored, so `a/b/`
//!   names the same topic as `a/b` (as the C++ server did). A leading `/`
//!   gives an empty first level, so `/a` is not `a`.
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

/// Split into levels: one trailing `/` is dropped, empty levels elsewhere
/// are kept, and `""` has no levels.
pub fn split_levels(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    s.strip_suffix('/').unwrap_or(s).split('/').collect()
}

/// True for `$`-prefixed topics such as `$mycel/clients/connected`.
pub fn is_system_topic(topic: &str) -> bool {
    topic.starts_with('$')
}

/// Check a concrete topic used in a publish.
pub fn validate_topic(topic: &str) -> Result<(), TopicError> {
    check_common(topic)?;
    if topic.contains(['+', '#']) {
        return Err(TopicError::WildcardInTopic);
    }
    Ok(())
}

/// Check a subscribe, unsubscribe or replay pattern.
pub fn validate_pattern(pattern: &str) -> Result<(), TopicError> {
    check_common(pattern)?;
    let levels = split_levels(pattern);
    for (i, level) in levels.iter().enumerate() {
        let wild = level.contains(['+', '#']);
        if wild && *level != "+" && *level != "#" {
            return Err(TopicError::PartialWildcard);
        }
        if *level == "#" && i + 1 != levels.len() {
            return Err(TopicError::HashNotLast);
        }
    }
    Ok(())
}

fn check_common(s: &str) -> Result<(), TopicError> {
    if s.is_empty() {
        return Err(TopicError::Empty);
    }
    if s.len() > MAX_TOPIC_LEN {
        return Err(TopicError::TooLong);
    }
    if s.contains('\0') {
        return Err(TopicError::Nul);
    }
    Ok(())
}

/// Does `topic` match `pattern`? Both are assumed valid.
pub fn matches(pattern: &str, topic: &str) -> bool {
    let p = split_levels(pattern);
    if is_system_topic(topic) && matches!(p.first(), Some(&"+") | Some(&"#")) {
        return false;
    }
    levels_match(&p, &split_levels(topic))
}

fn levels_match(pattern: &[&str], topic: &[&str]) -> bool {
    match (pattern.split_first(), topic.split_first()) {
        (Some((&"#", _)), _) => true,
        (None, None) => true,
        (Some((&"+", p_rest)), Some((_, t_rest))) => levels_match(p_rest, t_rest),
        (Some((p, p_rest)), Some((t, t_rest))) => p == t && levels_match(p_rest, t_rest),
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_topic.rs"]
mod tests;
