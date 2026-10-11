//! Blocking mycel client over one TCP connection.
//!
//! Works against the psmux bus and the C++ mycel server. Each request reads
//! the very next frame as its reply, which both servers guarantee as long
//! as the connection has no live subscription when the request is sent.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::{Shutdown as NetShutdown, SocketAddr, TcpStream};
use std::time::Duration;

use super::codec::{read_frame, write_frame, FrameError};
use super::proto::{
    Ack, Envelope, Event, Hello, Msg, Publish, Replay, Shutdown, StatusReq, StatusResp, Subscribe,
    Unsubscribe,
};

/// Why a client call failed.
#[derive(Debug)]
pub enum ClientError {
    /// Socket or framing failure. The connection is unusable.
    Frame(FrameError),
    /// The server closed the connection.
    Closed,
    /// The server answered with `Error`.
    Rejected(String),
    /// The server answered with a frame that is not the expected reply.
    Unexpected(Box<Envelope>),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Frame(e) => write!(f, "{e}"),
            ClientError::Closed => f.write_str("connection closed by server"),
            ClientError::Rejected(m) => write!(f, "server rejected request: {m}"),
            ClientError::Unexpected(e) => write!(f, "unexpected reply: {e:?}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<FrameError> for ClientError {
    fn from(e: FrameError) -> Self {
        ClientError::Frame(e)
    }
}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Frame(FrameError::Io(e))
    }
}

/// One Hello'd connection to a mycel server.
#[derive(Debug)]
pub struct Client {
    stream: TcpStream,
}

impl Client {
    /// Connect, send Hello, and wait (up to `timeout`) for the Ack.
    pub fn connect(
        addr: SocketAddr,
        client_id: &str,
        timeout: Duration,
    ) -> Result<Client, ClientError> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        let mut client = Client { stream };
        client.send(Msg::Hello(Hello {
            client_id: client_id.into(),
            client_type: "both".into(),
        }))?;
        client.expect_ack("hello")?;
        client.stream.set_read_timeout(None)?;
        Ok(client)
    }

    /// Bound how long the next reads may block. `None` blocks forever. A
    /// read that times out leaves the stream unusable (see the codec).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    /// Publish an event. Fire-and-forget: the server sends no reply.
    pub fn publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        metadata: HashMap<String, String>,
    ) -> Result<(), ClientError> {
        self.send(Msg::Publish(Publish {
            topic: topic.into(),
            payload: payload.to_vec(),
            ts_unix_ms: 0,
            metadata,
        }))
    }

    /// Start live delivery for `pattern`.
    pub fn subscribe(&mut self, pattern: &str) -> Result<(), ClientError> {
        self.send(Msg::Subscribe(Subscribe {
            topic_pattern: pattern.into(),
        }))?;
        self.expect_ack("subscribe")
    }

    /// Stop live delivery for `pattern`.
    pub fn unsubscribe(&mut self, pattern: &str) -> Result<(), ClientError> {
        self.send(Msg::Unsubscribe(Unsubscribe {
            topic_pattern: pattern.into(),
        }))?;
        self.expect_ack("unsubscribe")
    }

    /// Buffered events matching `pattern` with `ts >= since_unix_ms`, up to
    /// `max_events` (`<= 0` for all), oldest first.
    pub fn replay(
        &mut self,
        pattern: &str,
        since_unix_ms: i64,
        max_events: i32,
    ) -> Result<Vec<Event>, ClientError> {
        self.send(Msg::Replay(Replay {
            topic_pattern: pattern.into(),
            since_unix_ms,
            max_events,
        }))?;
        let mut events = Vec::new();
        loop {
            match self.next_msg()? {
                Msg::Event(e) => events.push(e),
                Msg::Ack(a) if a.r#ref == "replay_complete" => return Ok(events),
                other => return Err(unexpected(other)),
            }
        }
    }

    /// Ask for the server status.
    pub fn status(&mut self) -> Result<StatusResp, ClientError> {
        self.send(Msg::StatusReq(StatusReq {}))?;
        match self.next_msg()? {
            Msg::StatusResp(s) => Ok(s),
            other => Err(unexpected(other)),
        }
    }

    /// Ask the server to stop and wait for its Ack.
    pub fn shutdown(&mut self, reason: &str) -> Result<(), ClientError> {
        self.send(Msg::Shutdown(Shutdown {
            reason: reason.into(),
        }))?;
        self.expect_ack("shutdown")
    }

    /// The next delivered event. Any other frame is an error.
    pub fn next_event(&mut self) -> Result<Event, ClientError> {
        match self.next_msg()? {
            Msg::Event(e) => Ok(e),
            other => Err(unexpected(other)),
        }
    }

    /// The next frame's message. An empty envelope is skipped.
    pub fn next_msg(&mut self) -> Result<Msg, ClientError> {
        loop {
            match read_frame(&mut self.stream)? {
                None => return Err(ClientError::Closed),
                Some(Envelope { msg: Some(m) }) => return Ok(m),
                Some(Envelope { msg: None }) => {}
            }
        }
    }

    /// True if the server has closed the connection (or sent something we
    /// never asked for). Does not block. A publisher that never reads uses
    /// this to notice a restarted server before its next write is lost.
    pub fn peer_closed(&self) -> bool {
        if self.stream.set_nonblocking(true).is_err() {
            return true;
        }
        let mut byte = [0u8; 1];
        let closed = match self.stream.peek(&mut byte) {
            Ok(_) => true,
            Err(e) => e.kind() != io::ErrorKind::WouldBlock,
        };
        closed || self.stream.set_nonblocking(false).is_err()
    }

    /// Close the connection.
    pub fn close(self) {
        let _ = self.stream.shutdown(NetShutdown::Both);
    }

    fn send(&mut self, msg: Msg) -> Result<(), ClientError> {
        write_frame(&mut self.stream, &Envelope { msg: Some(msg) })?;
        Ok(())
    }

    fn expect_ack(&mut self, reference: &str) -> Result<(), ClientError> {
        match self.next_msg()? {
            Msg::Ack(Ack { r#ref }) if r#ref == reference => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

fn unexpected(msg: Msg) -> ClientError {
    match msg {
        Msg::Error(e) => ClientError::Rejected(e.message),
        other => ClientError::Unexpected(Box::new(Envelope { msg: Some(other) })),
    }
}

/// True if a mycel server at `addr` answers Hello within `timeout`.
pub fn probe(addr: SocketAddr, timeout: Duration) -> bool {
    match Client::connect(addr, "psmux-bus-probe", timeout) {
        Ok(c) => {
            c.close();
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_client.rs"]
mod tests;
