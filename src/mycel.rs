//! Optional mycel pub/sub event bus integration.
//!
//! Provides a [`MycelBus`] that lazily connects to a mycel server and publishes
//! pane lifecycle events. Connection failures are silently ignored — mycel is
//! an optional observability channel, not a critical dependency.
//!
//! Enabled via `--features mycel` or `features = ["mycel"]` in Cargo.toml.

use std::sync::mpsc;
use std::thread;

/// A message to publish on the mycel bus.
struct PublishMsg {
    topic: String,
    payload: Vec<u8>,
}

/// Canonical topic names published by psmux.
///
/// External subscribers (canopy, orchestrators, observability tools) depend
/// on these exact strings. Changing any topic here is a breaking change.
pub mod topics {
    pub const PANE_CREATED: &str = "psmux/pane/created";
    pub const PANE_READY: &str = "psmux/pane/ready";
    pub const PANE_EXITED: &str = "psmux/pane/exited";
    pub const EXEC_COMPLETED: &str = "psmux/exec/completed";
    pub const SESSION_CREATED: &str = "psmux/session/created";
    pub const SESSION_RENAMED: &str = "psmux/session/renamed";
    pub const SESSION_KILLED: &str = "psmux/session/killed";
}

/// Handle to the mycel background publisher thread.
///
/// Send-only — the background thread owns the async client.
/// If the mycel server is unreachable, messages are silently dropped.
pub struct MycelBus {
    tx: mpsc::Sender<PublishMsg>,
    client_id: String,
}

impl MycelBus {
    /// Create a new MycelBus that lazily connects to the mycel server.
    ///
    /// Spawns a background thread running a tokio runtime. The connection
    /// is established on the first publish. If the server is unreachable,
    /// messages are dropped and reconnection is attempted on the next publish.
    ///
    /// `client_id` identifies this psmux instance (e.g., `"psmux@hostname"`).
    pub fn new(client_id: &str) -> Self {
        let (tx, rx) = mpsc::channel::<PublishMsg>();
        let client_id = client_id.to_string();
        let client_id_bg = client_id.clone();

        thread::Builder::new()
            .name("mycel-bus".into())
            .spawn(move || {
                let client_id = client_id_bg;
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("mycel: failed to create tokio runtime");

                rt.block_on(async move {
                    let addr = mycel_client::discover_server();
                    let mut client: Option<mycel_client::MycelClient> = None;

                    while let Ok(msg) = rx.recv() {
                        // Lazy connect / reconnect
                        if client.is_none() {
                            match mycel_client::MycelClient::connect(&addr, &client_id).await {
                                Ok(c) => client = Some(c),
                                Err(_) => {
                                    // Server unreachable — silently skip, retry on next publish
                                    continue;
                                }
                            }
                        }

                        if let Some(ref mut c) = client {
                            if c.publish(&msg.topic, &msg.payload, None).await.is_err() {
                                client = None; // force reconnect on next publish
                            }
                        }
                    }
                });
            })
            .expect("mycel: failed to spawn bus thread");

        Self { tx, client_id }
    }

    /// The client_id this bus was initialised with (e.g. `"psmux@hostname"`).
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Publish a JSON payload to a topic.
    ///
    /// Non-blocking — serializes and enqueues the message for the background
    /// thread. Returns immediately even if the mycel server is down.
    pub fn publish(&self, topic: &str, payload: &impl serde::Serialize) {
        if let Ok(bytes) = serde_json::to_vec(payload) {
            let _ = self.tx.send(PublishMsg {
                topic: topic.to_string(),
                payload: bytes,
            });
        }
    }
}

/// Global mycel bus instance.
///
/// Initialized once by `init_mycel_bus()`, accessed via `mycel_bus()`.
static MYCEL_BUS: std::sync::OnceLock<MycelBus> = std::sync::OnceLock::new();

/// Initialize the global mycel bus. Call once at server startup.
/// No-op if already initialized.
pub fn init_mycel_bus(client_id: &str) {
    MYCEL_BUS.get_or_init(|| MycelBus::new(client_id));
}

/// Get a reference to the global mycel bus, if initialized.
pub fn mycel_bus() -> Option<&'static MycelBus> {
    MYCEL_BUS.get()
}

/// Return the client_id the global bus was initialised with.
/// Returns `None` if the bus has not been initialised.
pub fn mycel_client_id() -> Option<&'static str> {
    MYCEL_BUS.get().map(|b| b.client_id())
}

/// Convenience: publish a pane lifecycle event if the bus is connected.
pub fn publish_pane_event(topic: &str, payload: &impl serde::Serialize) {
    if let Some(bus) = mycel_bus() {
        bus.publish(topic, payload);
    }
}
