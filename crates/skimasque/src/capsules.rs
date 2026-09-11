//! Reading and writing the Capsule Protocol on a request stream.
//!
//! Both ends of a tunnel do the same thing with the data stream: decode
//! capsules, forward DATAGRAM capsules into the tunnel's inbound queue, and
//! hand the rest to whoever understands them. Only the concrete stream type
//! differs between client and server, so the protocol half lives here and each
//! side keeps a short read loop of its own.
//!
//! CONNECT-UDP has no use for control capsules and leaves the control sink
//! unset, so they are dropped as RFC 9297 requires. CONNECT-IP does its address
//! assignment and route advertisement through them, and sets one.

use std::ops::ControlFlow;

use bytes::Bytes;
use skimasque_core::capsule::{Capsule, CapsuleDecoder, CapsuleType};
use tokio::sync::mpsc;
use tracing::{trace, warn};

/// Decodes capsules from stream bytes and routes them onward.
#[derive(Debug)]
pub(crate) struct CapsulePump {
    decoder: CapsuleDecoder,
    datagrams: mpsc::Sender<Bytes>,
    control: Option<mpsc::Sender<Capsule>>,
}

impl CapsulePump {
    /// A pump that forwards datagrams and drops every other capsule type.
    pub(crate) fn new(datagrams: mpsc::Sender<Bytes>) -> Self {
        Self {
            decoder: CapsuleDecoder::new(),
            datagrams,
            control: None,
        }
    }

    /// A pump that also forwards control capsules to `control`.
    #[cfg(feature = "connect-ip")]
    pub(crate) fn with_control(
        datagrams: mpsc::Sender<Bytes>,
        control: mpsc::Sender<Capsule>,
    ) -> Self {
        Self {
            decoder: CapsuleDecoder::new(),
            datagrams,
            control: Some(control),
        }
    }

    /// Feed the next chunk of stream bytes.
    ///
    /// Returns [`ControlFlow::Break`] when the tunnel should be torn down:
    /// either the peer sent a malformed capsule, which RFC 9297 Section 3.3
    /// makes a fatal message error, or nothing is listening any more.
    pub(crate) async fn push(&mut self, chunk: &[u8]) -> ControlFlow<()> {
        self.decoder.push(chunk);
        loop {
            match self.decoder.next_capsule() {
                Ok(Some(capsule)) => {
                    if capsule.kind == CapsuleType::DATAGRAM {
                        // RFC 9297, Section 3.5 gives a DATAGRAM capsule the
                        // same semantics as a QUIC DATAGRAM frame, so it joins
                        // the same queue.
                        if self.datagrams.send(capsule.value).await.is_err() {
                            return ControlFlow::Break(());
                        }
                    } else if let Some(control) = &self.control {
                        if control.send(capsule).await.is_err() {
                            return ControlFlow::Break(());
                        }
                    } else {
                        // RFC 9297, Section 3.2: silently drop unknown types
                        // and carry on parsing.
                        trace!(kind = %capsule.kind, "ignoring capsule");
                    }
                }
                Ok(None) => return ControlFlow::Continue(()),
                Err(error) => {
                    warn!(%error, "malformed capsule");
                    return ControlFlow::Break(());
                }
            }
        }
    }

    /// Check that the stream ended on a capsule boundary.
    pub(crate) fn finish(&self) {
        if let Err(error) = self.decoder.finish() {
            warn!(%error, "stream ended mid-capsule");
        }
    }
}

#[cfg(feature = "connect-ip")]
pub(crate) use capsule_writer::CapsuleWriter;

/// Writing capsules to a request stream.
///
/// `h3` gives the client and server halves of a request stream distinct types
/// with identical inherent `send_data` methods and no common trait, so this
/// exists to let one tunnel type drive either. It is boxed rather than generic
/// because the alternative is threading the stream type through every
/// CONNECT-IP type for no benefit. The whole thing is CONNECT-IP-only: a
/// CONNECT-UDP tunnel never writes to its request stream.
#[cfg(feature = "connect-ip")]
mod capsule_writer {
    use std::future::Future;
    use std::pin::Pin;

    use bytes::Bytes;

    use crate::Error;

    pub(crate) trait CapsuleWriter: Send {
        fn write_capsule<'a>(
            &'a mut self,
            bytes: Bytes,
        ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

        /// Named distinctly from `h3`'s inherent `finish`, so which one is being
        /// called is never a question of method-resolution order.
        fn finish_stream(&mut self)
            -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;
    }

    macro_rules! impl_capsule_writer {
        ($ty:ty) => {
            impl CapsuleWriter for $ty {
                fn write_capsule<'a>(
                    &'a mut self,
                    bytes: Bytes,
                ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
                    Box::pin(async move { self.send_data(bytes).await.map_err(Error::from) })
                }

                fn finish_stream(
                    &mut self,
                ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
                    Box::pin(async move { self.finish().await.map_err(Error::from) })
                }
            }
        };
    }

    impl_capsule_writer!(h3::client::RequestStream<h3_quinn::SendStream<Bytes>, Bytes>);
    impl_capsule_writer!(h3::server::RequestStream<h3_quinn::SendStream<Bytes>, Bytes>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimasque_core::capsule::CapsuleType;

    #[tokio::test]
    async fn datagram_capsules_reach_the_datagram_queue() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut pump = CapsulePump::new(tx);
        assert!(pump
            .push(&Capsule::datagram(Bytes::from_static(b"payload")).encode())
            .await
            .is_continue());
        assert_eq!(rx.recv().await.unwrap(), Bytes::from_static(b"payload"));
    }

    /// Without a control sink, a non-datagram capsule is dropped rather than
    /// treated as an error -- that is what RFC 9297 requires of an endpoint
    /// that does not understand a capsule type.
    #[tokio::test]
    async fn control_capsules_are_dropped_when_nobody_wants_them() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut pump = CapsulePump::new(tx);
        let capsule = Capsule::new(CapsuleType::ADDRESS_ASSIGN, Bytes::from_static(b"\x00"));
        assert!(pump.push(&capsule.encode()).await.is_continue());
        assert!(rx.try_recv().is_err());
    }

    #[cfg(feature = "connect-ip")]
    #[tokio::test]
    async fn control_capsules_are_delivered_when_someone_does() {
        let (datagrams, mut datagram_rx) = mpsc::channel(4);
        let (control, mut control_rx) = mpsc::channel(4);
        let mut pump = CapsulePump::with_control(datagrams, control);

        let assign = Capsule::new(CapsuleType::ADDRESS_ASSIGN, Bytes::from_static(b"\x00"));
        let mut wire = assign.encode().to_vec();
        wire.extend_from_slice(&Capsule::datagram(Bytes::from_static(b"pkt")).encode());
        assert!(pump.push(&wire).await.is_continue());

        assert_eq!(control_rx.recv().await.unwrap(), assign);
        assert_eq!(datagram_rx.recv().await.unwrap(), Bytes::from_static(b"pkt"));
    }

    #[tokio::test]
    async fn a_malformed_capsule_tears_the_tunnel_down() {
        let (tx, _rx) = mpsc::channel(4);
        // Declare a value far larger than the decoder will buffer.
        let mut pump = CapsulePump::new(tx);
        let mut wire = bytes::BytesMut::new();
        skimasque_core::capsule::write_header(CapsuleType::DATAGRAM, 10_000_000, &mut wire);
        assert!(pump.push(&wire).await.is_break());
    }
}
