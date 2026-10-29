// Framing and wire-format tests for src/bus/codec.rs and src/bus/proto.rs.
// Ported from mycel tests/test_framing.cpp, plus golden bytes (spec 12.6 #15).

use super::*;
use crate::bus::proto::*;
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// Yields at most `chunk` bytes per read, to exercise partial reads.
struct Chunked {
    data: Vec<u8>,
    pos: usize,
    chunk: usize,
}

impl Read for Chunked {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.chunk.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn env(msg: Msg) -> Envelope {
    Envelope { msg: Some(msg) }
}

fn round_trip(e: &Envelope) -> Envelope {
    let bytes = encode_frame(e).unwrap();
    read_frame(&mut Cursor::new(bytes)).unwrap().unwrap()
}

fn assert_round_trip(msg: Msg) {
    let e = env(msg);
    assert_eq!(round_trip(&e), e);
}

fn sample_event() -> Event {
    Event {
        topic: "psmux/pane/created".into(),
        payload: b"{\"pane_id\":\"%1\"}".to_vec(),
        ts_unix_ms: 1_700_000_000_123,
        metadata: HashMap::from([("k".to_string(), "v".to_string())]),
        sequence: 42,
        publisher: "psmux@HOST".into(),
    }
}

#[test]
fn hello_round_trip() {
    assert_round_trip(Msg::Hello(Hello {
        client_id: "psmux@HOST".into(),
        client_type: "both".into(),
    }));
}

#[test]
fn subscribe_round_trip() {
    assert_round_trip(Msg::Subscribe(Subscribe {
        topic_pattern: "canopy/#".into(),
    }));
}

#[test]
fn unsubscribe_round_trip() {
    assert_round_trip(Msg::Unsubscribe(Unsubscribe {
        topic_pattern: "canopy/+/x".into(),
    }));
}

#[test]
fn publish_round_trip() {
    assert_round_trip(Msg::Publish(Publish {
        topic: "a/b".into(),
        payload: vec![0, 1, 2, 255],
        ts_unix_ms: 99,
        metadata: HashMap::new(),
    }));
}

#[test]
fn publish_with_metadata_round_trip() {
    let metadata = HashMap::from([
        ("a".to_string(), "1".to_string()),
        ("b".to_string(), "2".to_string()),
    ]);
    assert_round_trip(Msg::Publish(Publish {
        topic: "a/b".into(),
        payload: b"x".to_vec(),
        ts_unix_ms: 1,
        metadata,
    }));
}

#[test]
fn replay_round_trip() {
    assert_round_trip(Msg::Replay(Replay {
        topic_pattern: "#".into(),
        since_unix_ms: 12345,
        max_events: -1,
    }));
}

#[test]
fn status_req_round_trip() {
    assert_round_trip(Msg::StatusReq(StatusReq {}));
}

#[test]
fn event_round_trip() {
    assert_round_trip(Msg::Event(sample_event()));
}

#[test]
fn ack_round_trip() {
    assert_round_trip(Msg::Ack(Ack {
        r#ref: "replay_complete".into(),
    }));
}

#[test]
fn error_round_trip() {
    assert_round_trip(Msg::Error(Error {
        message: "Server at max connections".into(),
    }));
}

#[test]
fn shutdown_round_trip() {
    assert_round_trip(Msg::Shutdown(Shutdown {
        reason: "CLI stop command".into(),
    }));
}

#[test]
fn status_resp_round_trip() {
    let client = ClientInfo {
        client_id: "c".into(),
        client_type: "both".into(),
        connected_at: 5,
        subscriptions: vec!["a/#".into(), "$mycel/#".into()],
        events_published: 1,
        events_received: 2,
        events_dropped: 3,
    };
    let topic = TopicInfo {
        topic: "a/b".into(),
        event_count: 7,
        subscriber_count: 1,
        oldest_event_ts: 10,
        newest_event_ts: 20,
    };
    assert_round_trip(Msg::StatusResp(StatusResp {
        clients: vec![client],
        topics: vec![topic],
        uptime_seconds: 60,
        total_events: 100,
    }));
}

fn event_frame() -> Vec<u8> {
    encode_frame(&env(Msg::Event(sample_event()))).unwrap()
}

fn read_chunked(data: Vec<u8>, chunk: usize) -> Envelope {
    let mut r = Chunked {
        data,
        pos: 0,
        chunk,
    };
    read_frame(&mut r).unwrap().unwrap()
}

#[test]
fn partial_read_byte_by_byte() {
    let got = read_chunked(event_frame(), 1);
    assert_eq!(got, env(Msg::Event(sample_event())));
}

#[test]
fn partial_read_split_in_header() {
    let got = read_chunked(event_frame(), 2);
    assert_eq!(got, env(Msg::Event(sample_event())));
}

#[test]
fn partial_read_split_in_payload() {
    let frame = event_frame();
    let chunk = 4 + (frame.len() - 4) / 2;
    assert_eq!(read_chunked(frame, chunk), env(Msg::Event(sample_event())));
}

#[test]
fn multiple_frames_in_one_buffer_read_in_order() {
    let ack = env(Msg::Ack(Ack {
        r#ref: "hello".into(),
    }));
    let mut bytes = encode_frame(&ack).unwrap();
    bytes.extend(event_frame());
    let mut r = Cursor::new(bytes);
    assert_eq!(read_frame(&mut r).unwrap().unwrap(), ack);
    assert_eq!(
        read_frame(&mut r).unwrap().unwrap(),
        env(Msg::Event(sample_event()))
    );
    assert!(read_frame(&mut r).unwrap().is_none());
}

#[test]
fn empty_stream_returns_none() {
    assert!(read_frame(&mut Cursor::new(Vec::new())).unwrap().is_none());
}

#[test]
fn eof_inside_header_is_io_error() {
    let err = read_frame(&mut Cursor::new(vec![0, 0])).unwrap_err();
    assert!(matches!(err, FrameError::Io(_)));
}

#[test]
fn eof_inside_body_is_io_error() {
    let mut frame = event_frame();
    frame.truncate(frame.len() - 1);
    let err = read_frame(&mut Cursor::new(frame)).unwrap_err();
    assert!(matches!(err, FrameError::Io(_)));
}

#[test]
fn zero_length_frame_is_empty_envelope() {
    let got = read_frame(&mut Cursor::new(vec![0, 0, 0, 0])).unwrap();
    assert_eq!(got, Some(Envelope { msg: None }));
}

#[test]
fn oversized_header_rejected_without_reading_body() {
    let prefix = (32u32 * 1024 * 1024).to_be_bytes().to_vec();
    let err = read_frame(&mut Cursor::new(prefix)).unwrap_err();
    assert!(matches!(err, FrameError::TooLarge(n) if n == 32 * 1024 * 1024));
}

#[test]
fn max_size_plus_one_rejected() {
    let prefix = ((MAX_FRAME_SIZE + 1) as u32).to_be_bytes().to_vec();
    let err = read_frame(&mut Cursor::new(prefix)).unwrap_err();
    assert!(matches!(err, FrameError::TooLarge(_)));
}

#[test]
fn exactly_max_size_passes_the_size_check() {
    // A body of MAX_FRAME_SIZE zero bytes is not a valid envelope, so the
    // read ends in Decode, which proves it got past the size check.
    let mut bytes = (MAX_FRAME_SIZE as u32).to_be_bytes().to_vec();
    bytes.resize(4 + MAX_FRAME_SIZE, 0);
    let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
    assert!(matches!(err, FrameError::Decode(_)));
}

#[test]
fn encode_rejects_oversized_envelope() {
    let big = env(Msg::Publish(Publish {
        topic: "t".into(),
        payload: vec![0; MAX_FRAME_SIZE],
        ts_unix_ms: 0,
        metadata: HashMap::new(),
    }));
    assert!(matches!(encode_frame(&big), Err(FrameError::TooLarge(_))));
}

#[test]
fn publish_that_fits_can_become_an_event_that_does_not() {
    let payload = vec![0; MAX_FRAME_SIZE - 16];
    let publish = env(Msg::Publish(Publish {
        topic: "t".into(),
        payload: payload.clone(),
        ts_unix_ms: 0,
        metadata: HashMap::new(),
    }));
    let event = env(Msg::Event(Event {
        topic: "t".into(),
        payload,
        ts_unix_ms: 1_700_000_000_123,
        sequence: 1,
        publisher: "psmux@HOST".into(),
        ..Default::default()
    }));
    assert!(fits_in_frame(&publish));
    assert!(!fits_in_frame(&event));
}

#[test]
fn announced_body_longer_than_the_stream_is_io_error() {
    let mut bytes = (MAX_FRAME_SIZE as u32).to_be_bytes().to_vec();
    bytes.extend([0u8; 8]);
    let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
    assert!(matches!(err, FrameError::Io(e) if e.kind() == std::io::ErrorKind::UnexpectedEof));
}

#[test]
fn invalid_protobuf_is_decode_error_and_stream_stays_in_sync() {
    let mut bytes = vec![0, 0, 0, 2, 0xff, 0xff];
    bytes.extend(event_frame());
    let mut r = Cursor::new(bytes);
    assert!(matches!(read_frame(&mut r), Err(FrameError::Decode(_))));
    assert_eq!(
        read_frame(&mut r).unwrap().unwrap(),
        env(Msg::Event(sample_event()))
    );
}

#[test]
fn write_frame_matches_encode_frame() {
    let e = env(Msg::StatusReq(StatusReq {}));
    let mut out = Vec::new();
    write_frame(&mut out, &e).unwrap();
    assert_eq!(out, encode_frame(&e).unwrap());
}

// ── Golden wire bytes (field numbers of proto/mycel.proto are the contract) ──
// These bytes were worked out by hand from the protobuf encoding rules, not
// captured from the C++ encoder, so they test 12.6 #15 only for these
// single-field and single-map-entry shapes. Frames captured from mycel.exe
// would make literal fixtures.

fn body(msg: Msg) -> Vec<u8> {
    let frame = encode_frame(&env(msg)).unwrap();
    let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(len, frame.len() - 4, "length prefix counts only the body");
    frame[4..].to_vec()
}

#[test]
fn golden_hello() {
    let m = Msg::Hello(Hello {
        client_id: "a".into(),
        client_type: "b".into(),
    });
    assert_eq!(body(m), [0x0a, 0x06, 0x0a, 0x01, b'a', 0x12, 0x01, b'b']);
}

#[test]
fn golden_subscribe_and_unsubscribe() {
    let s = Msg::Subscribe(Subscribe {
        topic_pattern: "a".into(),
    });
    let u = Msg::Unsubscribe(Unsubscribe {
        topic_pattern: "a".into(),
    });
    assert_eq!(body(s), [0x12, 0x03, 0x0a, 0x01, b'a']);
    assert_eq!(body(u), [0x1a, 0x03, 0x0a, 0x01, b'a']);
}

#[test]
fn golden_publish_with_one_metadata_entry() {
    let m = Msg::Publish(Publish {
        topic: "t".into(),
        payload: Vec::new(),
        ts_unix_ms: 0,
        metadata: HashMap::from([("k".to_string(), "v".to_string())]),
    });
    let expected = [
        0x22, 0x0b, 0x0a, 0x01, b't', 0x22, 0x06, 0x0a, 0x01, b'k', 0x12, 0x01, b'v',
    ];
    assert_eq!(body(m), expected);
}

#[test]
fn golden_replay_with_negative_max() {
    let m = Msg::Replay(Replay {
        topic_pattern: "#".into(),
        since_unix_ms: 5,
        max_events: -1,
    });
    let mut expected = vec![0x2a, 0x10, 0x0a, 0x01, b'#', 0x10, 0x05, 0x18];
    expected.extend([0xff; 9]);
    expected.push(0x01);
    assert_eq!(body(m), expected);
}

#[test]
fn golden_event() {
    let m = Msg::Event(Event {
        topic: "t".into(),
        payload: vec![1],
        ts_unix_ms: 1,
        metadata: HashMap::new(),
        sequence: 2,
        publisher: "p".into(),
    });
    let expected = [
        0x32, 0x0d, 0x0a, 0x01, b't', 0x12, 0x01, 0x01, 0x18, 0x01, 0x28, 0x02, 0x32, 0x01, b'p',
    ];
    assert_eq!(body(m), expected);
}

#[test]
fn golden_status_req() {
    assert_eq!(body(Msg::StatusReq(StatusReq {})), [0x3a, 0x00]);
}

#[test]
fn golden_status_resp_with_events_dropped() {
    let client = ClientInfo {
        events_dropped: 3,
        ..Default::default()
    };
    let m = Msg::StatusResp(StatusResp {
        clients: vec![client],
        ..Default::default()
    });
    assert_eq!(body(m), [0x42, 0x04, 0x0a, 0x02, 0x38, 0x03]);
}

#[test]
fn golden_ack() {
    let m = Msg::Ack(Ack {
        r#ref: "hello".into(),
    });
    let expected = [0x4a, 0x07, 0x0a, 0x05, b'h', b'e', b'l', b'l', b'o'];
    assert_eq!(body(m), expected);
}

#[test]
fn golden_error_and_shutdown() {
    let e = Msg::Error(Error {
        message: "m".into(),
    });
    let s = Msg::Shutdown(Shutdown { reason: "x".into() });
    assert_eq!(body(e), [0x52, 0x03, 0x0a, 0x01, b'm']);
    assert_eq!(body(s), [0x5a, 0x03, 0x0a, 0x01, b'x']);
}
