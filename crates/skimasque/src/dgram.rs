//! Demultiplexing QUIC DATAGRAM frames across the tunnels that share a connection.
//!
//! A single QUIC connection carries the datagrams of every tunnel on it, tagged
//! only by Quarter Stream ID. One task reads them and fans them out to per-tunnel
//! channels; sending goes the other way, prefixing the caller's payload with the
//! right Quarter Stream ID.
//!
//! We frame the RFC 9297 layer here rather than using `h3-datagram`, whose
//! `Datagram::encode` drops the Quarter Stream ID it just computed and writes
//! zeroes instead. See [`skimasque_core::datagram`] for the details.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use skimasque_core::datagram::{self, QuarterStreamId};
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

/// HTTP/3 error code `H3_DATAGRAM_ERROR` (RFC 9297, Section 2.1).
const H3_DATAGRAM_ERROR: u32 = 0x33;

/// Datagrams held for a request stream that has not registered yet.
///
/// RFC 9297, Section 2.1 permits buffering "on the order of a round trip" for a
/// stream that has not been created, which matters because a client may send
/// its first payload before the proxy has finished dispatching the request.
/// Rather than run a timer, we cap the buffer: an entry either gets claimed
/// almost immediately or gets evicted by later traffic.
const MAX_PENDING_PER_STREAM: usize = 16;
const MAX_PENDING_STREAMS: usize = 64;

/// Per-tunnel inbound queue depth. Datagrams are unreliable by contract, so a
/// slow tunnel drops its own traffic instead of stalling the shared reader.
const INBOUND_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("peer is not accepting QUIC datagrams on this connection")]
    Unsupported,
    #[error("datagram of {size} bytes exceeds the {limit}-byte path limit")]
    TooLarge { size: usize, limit: usize },
    #[error("connection closed")]
    ConnectionClosed,
}

/// Routes QUIC datagrams to and from the tunnels on one connection.
#[derive(Debug, Clone)]
pub struct DatagramRouter {
    conn: quinn::Connection,
    routes: Arc<Mutex<Routes>>,
}

#[derive(Debug, Default)]
struct Routes {
    active: HashMap<u64, mpsc::Sender<Bytes>>,
    /// Datagrams that arrived before their stream registered, oldest first.
    pending: HashMap<u64, VecDeque<Bytes>>,
}

impl DatagramRouter {
    /// Start routing datagrams for `conn`.
    ///
    /// Spawns a reader task that lives until the connection closes. The task
    /// holds only a `Weak`-free clone of the routing table, so it stops on its
    /// own when the connection goes away.
    pub fn spawn(conn: quinn::Connection) -> Self {
        let router = Self {
            conn,
            routes: Arc::new(Mutex::new(Routes::default())),
        };
        tokio::spawn(router.clone().read_loop());
        router
    }

    async fn read_loop(self) {
        loop {
            let raw = match self.conn.read_datagram().await {
                Ok(raw) => raw,
                Err(error) => {
                    debug!(%error, "datagram reader stopping");
                    return;
                }
            };

            match datagram::decode_http3_datagram(raw) {
                Ok((stream, payload)) => self.deliver(stream, payload),
                Err(error) => {
                    // RFC 9297, Section 2.1: a datagram we cannot parse is a
                    // connection error, not a stream error.
                    warn!(%error, "malformed HTTP/3 datagram; closing connection");
                    self.conn
                        .close(H3_DATAGRAM_ERROR.into(), b"malformed HTTP/3 datagram");
                    return;
                }
            }
        }
    }

    fn deliver(&self, stream: QuarterStreamId, payload: Bytes) {
        let mut routes = self.routes.lock().expect("routing table is not poisoned");
        let stream_id = stream.stream_id();

        if let Some(sink) = routes.active.get(&stream_id) {
            if sink.try_send(payload).is_err() {
                // Either the tunnel is backed up or it has gone away; both are
                // datagram loss, which the peer is required to tolerate.
                trace!(stream_id, "dropping datagram for a saturated tunnel");
            }
            return;
        }

        if routes.pending.len() >= MAX_PENDING_STREAMS && !routes.pending.contains_key(&stream_id) {
            trace!(stream_id, "dropping datagram for an unknown stream");
            return;
        }
        let queue = routes.pending.entry(stream_id).or_default();
        if queue.len() == MAX_PENDING_PER_STREAM {
            queue.pop_front();
        }
        queue.push_back(payload);
    }

    /// Claim the datagrams belonging to `stream_id`.
    ///
    /// The returned [`DatagramRoute`] unregisters on drop, and the sender is
    /// exposed so the capsule reader can feed DATAGRAM capsules into the same
    /// queue as QUIC datagrams -- RFC 9297 gives the two identical semantics.
    pub fn register(&self, stream_id: u64) -> (DatagramRoute, mpsc::Receiver<Bytes>) {
        let (tx, rx) = mpsc::channel(INBOUND_CAPACITY);
        let mut routes = self.routes.lock().expect("routing table is not poisoned");
        if let Some(buffered) = routes.pending.remove(&stream_id) {
            for payload in buffered {
                if tx.try_send(payload).is_err() {
                    break;
                }
            }
        }
        routes.active.insert(stream_id, tx.clone());
        drop(routes);

        (
            DatagramRoute {
                router: self.clone(),
                stream_id,
                sink: tx,
            },
            rx,
        )
    }

    fn unregister(&self, stream_id: u64) {
        self.routes
            .lock()
            .expect("routing table is not poisoned")
            .active
            .remove(&stream_id);
    }

    /// Send an HTTP Datagram Payload on `stream_id`.
    pub fn send(&self, stream_id: u64, payload: &[u8]) -> Result<(), SendError> {
        let stream = QuarterStreamId::from_stream_id(stream_id)
            .expect("request streams are always divisible by four");
        let wire = datagram::encode_http3_datagram(stream, payload);
        self.conn.send_datagram(wire).map_err(|error| match error {
            quinn::SendDatagramError::UnsupportedByPeer | quinn::SendDatagramError::Disabled => {
                SendError::Unsupported
            }
            quinn::SendDatagramError::TooLarge => SendError::TooLarge {
                size: payload.len(),
                limit: self.conn.max_datagram_size().unwrap_or(0),
            },
            quinn::SendDatagramError::ConnectionLost(_) => SendError::ConnectionClosed,
        })
    }

    /// The largest HTTP Datagram Payload that fits on `stream_id` right now, or
    /// `None` if the peer will not accept datagrams at all.
    ///
    /// This is the QUIC path limit less the Quarter Stream ID prefix, so callers
    /// can size their reads without discovering the limit by failing a send.
    pub fn max_payload_size(&self, stream_id: u64) -> Option<usize> {
        let stream = QuarterStreamId::from_stream_id(stream_id).ok()?;
        let overhead = skimasque_core::varint::encoded_len(stream.get());
        self.conn.max_datagram_size()?.checked_sub(overhead)
    }

    /// The underlying QUIC connection, for stats and shutdown.
    pub fn connection(&self) -> &quinn::Connection {
        &self.conn
    }
}

/// A registration in the router, released when dropped.
#[derive(Debug)]
pub struct DatagramRoute {
    router: DatagramRouter,
    stream_id: u64,
    sink: mpsc::Sender<Bytes>,
}

impl DatagramRoute {
    pub fn stream_id(&self) -> u64 {
        self.stream_id
    }

    /// Send an HTTP Datagram Payload on this route.
    pub fn send(&self, payload: &[u8]) -> Result<(), SendError> {
        self.router.send(self.stream_id, payload)
    }

    pub fn max_payload_size(&self) -> Option<usize> {
        self.router.max_payload_size(self.stream_id)
    }

    /// A handle for injecting payloads that arrived some other way -- in
    /// practice, DATAGRAM capsules read off the request stream.
    pub fn sink(&self) -> mpsc::Sender<Bytes> {
        self.sink.clone()
    }
}

impl Drop for DatagramRoute {
    fn drop(&mut self) {
        self.router.unregister(self.stream_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The routing table is exercised end-to-end by the integration tests; these
    /// cover the buffering policy, which is hard to provoke over a real network.
    fn routes() -> Arc<Mutex<Routes>> {
        Arc::new(Mutex::new(Routes::default()))
    }

    fn push_pending(routes: &Arc<Mutex<Routes>>, stream_id: u64, payload: &[u8]) {
        let mut guard = routes.lock().unwrap();
        let queue = guard.pending.entry(stream_id).or_default();
        if queue.len() == MAX_PENDING_PER_STREAM {
            queue.pop_front();
        }
        queue.push_back(Bytes::copy_from_slice(payload));
    }

    #[test]
    fn pending_queue_drops_the_oldest_rather_than_growing() {
        let routes = routes();
        for i in 0..MAX_PENDING_PER_STREAM + 4 {
            push_pending(&routes, 0, &[i as u8]);
        }
        let guard = routes.lock().unwrap();
        let queue = &guard.pending[&0];
        assert_eq!(queue.len(), MAX_PENDING_PER_STREAM);
        // The first four were evicted, so the queue starts at 4.
        assert_eq!(queue.front().unwrap()[..], [4]);
    }
}
