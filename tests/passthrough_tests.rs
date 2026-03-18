use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct PassthroughQueue {
    entries: Arc<Mutex<Vec<Vec<u8>>>>,
    max_depth: usize,
}

impl PassthroughQueue {
    fn new(max_depth: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            max_depth,
        }
    }

    fn push(&self, data: Vec<u8>) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.max_depth {
            entries.remove(0);
        }
        entries.push(data);
    }

    fn drain(&self) -> Vec<Vec<u8>> {
        let mut entries = self.entries.lock().unwrap();
        std::mem::take(&mut *entries)
    }
}

#[test]
fn test_passthrough_queue_basic() {
    let q = PassthroughQueue::new(64);
    q.push(b"\x1b]0;title\x07".to_vec());
    let drained = q.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0], b"\x1b]0;title\x07");
    assert!(q.drain().is_empty());
}

#[test]
fn test_passthrough_queue_max_depth() {
    let q = PassthroughQueue::new(3);
    q.push(b"a".to_vec());
    q.push(b"b".to_vec());
    q.push(b"c".to_vec());
    q.push(b"d".to_vec());
    let drained = q.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0], b"b");
    assert_eq!(drained[2], b"d");
}

#[test]
fn test_passthrough_gated_on_config() {
    assert!(!should_forward("off", true));
    assert!(!should_forward("off", false));
    assert!(should_forward("on", true));
    assert!(!should_forward("on", false));
    assert!(should_forward("all", true));
    assert!(should_forward("all", false));
}

fn should_forward(config: &str, is_active_pane: bool) -> bool {
    match config {
        "all" => true,
        "on" => is_active_pane,
        _ => false,
    }
}
