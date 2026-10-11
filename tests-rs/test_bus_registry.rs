// Client registry tests for src/bus/registry.rs.
// mycel tests/test_subscriber.cpp RegistryTest cases.

use super::*;

fn hello(id: &str) -> Hello {
    Hello {
        client_id: id.into(),
        client_type: "both".into(),
    }
}

fn registry(ids: &[(ConnId, &str)]) -> Registry<u32> {
    ids.iter()
        .map(|(c, id)| (*c, ClientRecord::new(&hello(id), 1000, 10)))
        .collect()
}

#[test]
fn add_and_lookup() {
    let r = registry(&[(1, "psmux@HOST")]);
    assert_eq!(r[&1].client_id, "psmux@HOST");
    assert_eq!(r[&1].connected_at, 1000);
}

#[test]
fn lookup_nonexistent() {
    assert!(!registry(&[(1, "a")]).contains_key(&2));
}

#[test]
fn duplicate_client_ids_are_separate_records() {
    let r = registry(&[(1, "psmux@HOST"), (2, "psmux@HOST")]);
    assert_eq!(r.len(), 2);
}

#[test]
fn second_hello_replaces_the_record() {
    // C++ orphaned the first record of a double-Hello connection.
    let mut r = registry(&[(1, "first")]);
    r.insert(1, ClientRecord::new(&hello("second"), 2000, 10));
    assert_eq!(r.len(), 1);
    assert_eq!(r[&1].client_id, "second");
}

#[test]
fn remove_client_takes_its_queue_with_it() {
    let mut r = registry(&[(1, "a")]);
    r.get_mut(&1).unwrap().queue.push_event(7);
    let mut gone = r.remove(&1).unwrap();
    assert_eq!(gone.queue.pop(), Some(7));
    assert!(r.is_empty());
}

#[test]
fn clients_are_listed_in_connection_order() {
    let r = registry(&[(3, "c"), (1, "a"), (2, "b")]);
    let ids: Vec<&str> = r.values().map(|c| c.client_id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn info_carries_counters_and_drops() {
    let mut rec: ClientRecord<u32> = ClientRecord::new(&hello("a"), 1000, 1);
    rec.events_published = 4;
    rec.events_received = 2;
    rec.queue.push_event(1);
    rec.queue.push_event(2);
    let info = rec.info(vec!["a/#".into()]);
    assert_eq!(info.client_type, "both");
    assert_eq!(info.subscriptions, ["a/#"]);
    assert_eq!((info.events_published, info.events_received), (4, 2));
    assert_eq!(info.events_dropped, 1);
}
