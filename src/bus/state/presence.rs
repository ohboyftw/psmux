//! Presence and server announcements on `$mycel/*` topics.

use super::super::client::PROBE_CLIENT_ID;
use super::super::proto::Hello;
use super::super::subs::ConnId;
use super::{State, SERVER_PUBLISHER};

impl State {
    /// Forget a connection. Returns `true` only the first time, and only
    /// for a connection that completed Hello. Announces
    /// `$mycel/clients/disconnected`, except for liveness probes.
    pub fn disconnect(&mut self, conn: ConnId, now: i64) -> bool {
        let Some(rec) = self.registry.remove(&conn) else {
            return false;
        };
        self.subs.remove_conn(conn);
        self.marks.remove(&conn);
        if !self.stopping && rec.client_id != PROBE_CLIENT_ID {
            let payload = serde_json::json!({ "client_id": rec.client_id, "ts": now });
            self.publish_system("$mycel/clients/disconnected", &payload, now);
        }
        true
    }

    /// One `$mycel/clients/heartbeat` per registered client.
    pub fn heartbeat(&mut self, now: i64) {
        let ids: Vec<String> = self
            .registry
            .values()
            .map(|r| r.client_id.clone())
            .collect();
        for id in ids {
            let payload = serde_json::json!({ "client_id": id, "ts": now });
            self.publish_system("$mycel/clients/heartbeat", &payload, now);
        }
    }

    /// Announce `$mycel/clients/connected`, except for liveness probes.
    pub(super) fn announce_connected(&mut self, hello: &Hello, now: i64) {
        if hello.client_id == PROBE_CLIENT_ID {
            return;
        }
        let payload = serde_json::json!({
            "client_id": hello.client_id, "ts": now, "type": hello.client_type,
        });
        self.publish_system("$mycel/clients/connected", &payload, now);
    }

    /// Announce `$mycel/server/started`.
    pub fn server_started(&mut self, port: u16, bind: &str, now: i64) {
        let payload = serde_json::json!({
            "client_id": SERVER_PUBLISHER, "ts": now, "port": port, "bind": bind,
        });
        self.publish_system("$mycel/server/started", &payload, now);
    }
}
