//! Flag parsing for control lines that spawn a pane (`new-window`,
//! `split-window`).
//!
//! The client quotes every value it sends, so only an unquoted argument can
//! be a flag.  A quoted window name such as `"-d"` or `"--"` (window names can
//! come from a pane's OSC title) therefore stays a value instead of turning
//! into a flag.

/// The flags, values and command of one spawn control line.
pub(crate) struct SpawnArgs<'a> {
    values: Vec<(&'a str, &'a str)>,
    switches: Vec<&'a str>,
    /// Everything after an unquoted `--`, joined, else the first positional.
    pub command: Option<String>,
}

impl<'a> SpawnArgs<'a> {
    /// The first value given for `flag`.
    pub fn value(&self, flag: &str) -> Option<&'a str> {
        self.values
            .iter()
            .find(|(f, _)| *f == flag)
            .map(|(_, v)| *v)
    }

    /// Every value given for `flag`, in order.
    pub fn values(&self, flag: &'a str) -> impl Iterator<Item = &'a str> + '_ {
        self.values
            .iter()
            .filter(move |(f, _)| *f == flag)
            .map(|(_, v)| *v)
    }

    /// Whether the switch `flag` was given.
    pub fn has(&self, flag: &str) -> bool {
        self.switches.contains(&flag)
    }
}

/// Parse `args` (with `quoted[i]` saying whether `args[i]` was quoted);
/// `value_flags` are the flags that take a value.
pub(crate) fn parse_spawn_args<'a>(
    args: &[&'a str],
    quoted: &[bool],
    value_flags: &[&str],
) -> SpawnArgs<'a> {
    let mut parsed = SpawnArgs {
        values: Vec::new(),
        switches: Vec::new(),
        command: None,
    };
    let mut i = 0;
    while i < args.len() {
        let (arg, is_flag) = (args[i], !quoted.get(i).copied().unwrap_or(false));
        if is_flag && arg == "--" {
            let rest: Vec<&str> = args[i + 1..].iter().map(|s| s.trim_matches('"')).collect();
            parsed.command = (!rest.is_empty()).then(|| rest.join(" "));
            break;
        }
        if is_flag && value_flags.contains(&arg) {
            if let Some(v) = args.get(i + 1) {
                parsed.values.push((arg, v));
            }
            i += 2;
            continue;
        }
        if is_flag && arg.starts_with('-') {
            parsed.switches.push(arg);
        } else if parsed.command.is_none() {
            parsed.command = Some(arg.trim_matches('"').to_string());
        }
        i += 1;
    }
    parsed
}

/// Positions of the `-t` flags in `args`: unquoted, before an unquoted `--`,
/// and not the value of an earlier `-t`.
pub(crate) fn target_flag_positions(args: &[&str], quoted: &[bool]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if !quoted.get(i).copied().unwrap_or(false) {
            if args[i] == "--" {
                break;
            }
            if args[i] == "-t" {
                positions.push(i);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    positions
}

#[cfg(test)]
#[path = "../../tests-rs/test_spawn_args.rs"]
mod tests;
