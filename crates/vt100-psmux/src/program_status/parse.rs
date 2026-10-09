//! OSC 7501 report body parsing and validation.

use super::{BlockedKind, ProgramState, ProgramStatusRecord};

const MAX_KEY_LEN: usize = 16;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;
const MAX_APP_LEN: usize = 32;
const MAX_ID_LEN: usize = 128;
const MAX_SEGMENT_LEN: usize = 32;
const MAX_ID_DEPTH: usize = 8;

/// What one parsed report asks the store to do.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Set(ProgramStatusRecord),
    Clear,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) id: String,
    pub(super) action: Action,
}

/// Raw key values from one report body, before semantic validation.
#[derive(Default)]
struct Fields<'a> {
    state: Option<&'a [u8]>,
    id: Option<&'a [u8]>,
    kind: Option<&'a [u8]>,
    progress: Option<&'a [u8]>,
    app: Option<&'a [u8]>,
    title: Option<&'a [u8]>,
    msg: Option<&'a [u8]>,
}

/// Parses one report body.  `None` means the whole report is discarded.
pub(super) fn parse_report(body: &[u8]) -> Option<Report> {
    let fields = collect_fields(body)?;
    let id = parse_id(fields.id)?;
    let state = fields.state?;
    if state == b"clear" {
        // A clear is still one report: an invalid field discards it whole.
        build_record(ProgramState::Idle, &fields)?;
        return Some(Report {
            id,
            action: Action::Clear,
        });
    }
    let state = parse_state(state)?;
    let record = build_record(state, &fields)?;
    Some(Report {
        id,
        action: Action::Set(record),
    })
}

/// Splits the body into pairs, skipping malformed ones.  `None` when a key
/// exceeds the key length limit.
fn collect_fields(body: &[u8]) -> Option<Fields<'_>> {
    let mut fields = Fields::default();
    for pair in body.split(|&b| b == b':') {
        let Some(eq) = pair.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = trim(&pair[..eq]);
        let value = trim(&pair[eq + 1..]);
        if key.len() > MAX_KEY_LEN {
            return None;
        }
        if key.is_empty()
            || !key.iter().all(u8::is_ascii_lowercase)
            || !value.iter().all(|&b| is_value_byte(b))
        {
            continue;
        }
        store_field(&mut fields, key, value);
    }
    Some(fields)
}

fn store_field<'a>(fields: &mut Fields<'a>, key: &[u8], value: &'a [u8]) {
    let slot = match key {
        b"state" => &mut fields.state,
        b"id" => &mut fields.id,
        b"kind" => &mut fields.kind,
        b"progress" => &mut fields.progress,
        b"app" => &mut fields.app,
        b"title" => &mut fields.title,
        b"msg" => &mut fields.msg,
        _ => return,
    };
    *slot = Some(value);
}

fn trim(bytes: &[u8]) -> &[u8] {
    let is_ws = |b: &u8| b.is_ascii_whitespace();
    let start = bytes.iter().position(|b| !is_ws(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !is_ws(b))
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.+-".contains(&b)
}

/// Validates the id; absent means the root record (`""`).  `None` means the
/// report is discarded — an invalid id never falls back to the root.
fn parse_id(id: Option<&[u8]>) -> Option<String> {
    let Some(id) = id else {
        return Some(String::new());
    };
    if id.len() > MAX_ID_LEN || id.split(|&b| b == b'/').count() > MAX_ID_DEPTH {
        return None;
    }
    let segments_ok = id.split(|&b| b == b'/').all(|seg| {
        !seg.is_empty() && seg.len() <= MAX_SEGMENT_LEN && seg.iter().all(|&b| is_name_byte(b))
    });
    segments_ok.then(|| String::from_utf8_lossy(id).into_owned())
}

fn parse_state(state: &[u8]) -> Option<ProgramState> {
    match state {
        b"idle" => Some(ProgramState::Idle),
        b"working" => Some(ProgramState::Working),
        b"done" => Some(ProgramState::Done),
        b"blocked" => Some(ProgramState::Blocked),
        b"error" => Some(ProgramState::Error),
        _ => None,
    }
}

fn build_record(state: ProgramState, fields: &Fields<'_>) -> Option<ProgramStatusRecord> {
    let app = parse_app(fields.app).ok()?;
    let title = decode_text(fields.title, MAX_TITLE_ENCODED, MAX_TITLE_DECODED).ok()?;
    let msg = decode_text(fields.msg, MAX_MSG_ENCODED, MAX_MSG_DECODED).ok()?;
    let blocked = state == ProgramState::Blocked;
    let kind = fields.kind.filter(|_| blocked).and_then(parse_kind);
    let progress = fields
        .progress
        .filter(|_| blocked || state == ProgramState::Working)
        .and_then(parse_progress);
    Some(ProgramStatusRecord {
        state,
        kind,
        progress,
        app,
        title,
        msg,
        stamp: 0,
    })
}

/// A field violation that discards the whole report.
struct Discard;

/// `Ok(None)` when the app is absent or not a valid name; `Err` when it
/// exceeds the length limit.
fn parse_app(app: Option<&[u8]>) -> Result<Option<String>, Discard> {
    let Some(app) = app else {
        return Ok(None);
    };
    if app.len() > MAX_APP_LEN {
        return Err(Discard);
    }
    let valid = !app.is_empty() && app.iter().all(|&b| is_name_byte(b));
    Ok(valid.then(|| String::from_utf8_lossy(app).into_owned()))
}

fn parse_kind(kind: &[u8]) -> Option<BlockedKind> {
    match kind {
        b"permission" => Some(BlockedKind::Permission),
        b"question" => Some(BlockedKind::Question),
        b"auth" => Some(BlockedKind::Auth),
        _ => None,
    }
}

fn parse_progress(progress: &[u8]) -> Option<u8> {
    if progress.is_empty() || !progress.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(progress)
        .ok()?
        .parse::<u8>()
        .ok()
        .filter(|&p| p <= 100)
}

/// Decodes a base64 text value.  `Ok(None)` when absent or empty; `Err` on any
/// limit, base64, UTF-8 or control-char failure.
fn decode_text(
    value: Option<&[u8]>,
    max_encoded: usize,
    max_decoded: usize,
) -> Result<Option<String>, Discard> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > max_encoded {
        return Err(Discard);
    }
    let bytes = decode_base64(value).ok_or(Discard)?;
    if bytes.len() > max_decoded {
        return Err(Discard);
    }
    let text = String::from_utf8(bytes).map_err(|_| Discard)?;
    if text.chars().any(char::is_control) {
        return Err(Discard);
    }
    Ok((!text.is_empty()).then_some(text))
}

fn base64_digit(b: u8) -> Option<u32> {
    let digit = match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(digit))
}

/// Standard-alphabet base64 with optional `=` padding.
fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    let pad = input.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 || (pad > 0 && input.len() % 4 != 0) {
        return None;
    }
    let data = &input[..input.len() - pad];
    if data.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    for chunk in data.chunks(4) {
        let mut acc = 0u32;
        for &b in chunk {
            acc = (acc << 6) | base64_digit(b)?;
        }
        let bytes = (acc << (6 * (4 - chunk.len()))).to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_base64_when_padded_or_unpadded_then_decodes() {
        assert_eq!(decode_base64(b"aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64(b"aGk").unwrap(), b"hi");
        assert_eq!(decode_base64(b"aGVsbG8gd29ybGQ").unwrap(), b"hello world");
        assert_eq!(decode_base64(b"").unwrap(), b"");
    }

    #[test]
    fn decode_base64_when_malformed_then_rejects() {
        assert!(decode_base64(b"a").is_none());
        assert!(decode_base64(b"aG=k").is_none());
        assert!(decode_base64(b"aGk==").is_none());
        assert!(decode_base64(b"a,bc").is_none());
    }

    #[test]
    fn parse_report_when_pair_malformed_then_skips_only_that_pair() {
        let report = parse_report(b"bogus:state=working:X=1:app=pi").unwrap();
        let Action::Set(record) = report.action else {
            panic!("expected set");
        };
        assert_eq!(record.app(), Some("pi"));
    }

    #[test]
    fn parse_report_when_key_repeated_then_last_wins() {
        let report = parse_report(b"state=idle:state=done").unwrap();
        let Action::Set(record) = report.action else {
            panic!("expected set");
        };
        assert_eq!(record.state(), ProgramState::Done);
    }

    #[test]
    fn parse_report_when_whitespace_around_pairs_then_trimmed() {
        let report = parse_report(b" state = working : progress = 7 ").unwrap();
        let Action::Set(record) = report.action else {
            panic!("expected set");
        };
        assert_eq!(record.progress(), Some(7));
    }

    #[test]
    fn parse_report_when_state_missing_then_discarded() {
        assert!(parse_report(b"app=pi").is_none());
    }
}
