//! Shipping the gateway's audit trail to the control plane (D7).
//!
//! [`ControlPlaneAuditSink`] wraps the gateway's normal [`AuditSink`] (the
//! JSONL file or the tracing target, which stays authoritative) and also
//! enqueues each event for [`run_audit_shipping`], a background task that
//! hash-chains batches and `POST`s them to `.../audit`.
//!
//! **Fail static.** `record` never blocks and never errors: it writes the
//! local sink, then `try_send`s to a bounded channel, dropping (with a metric)
//! if the channel is full. A shipping failure is logged and retried; it never
//! touches enforcement or the local sink.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use skimasque::audit::{AuditEvent, AuditSink};
use tokio::sync::mpsc;

use crate::control::{audit_hash, AuditHead, ChainedAuditEvent, ControlPlane, GatewayIdentity};

/// Room for a burst of decisions before the shipper catches up.
pub const AUDIT_BUFFER: usize = 4096;
/// Events per `POST .../audit`.
const BATCH_MAX: usize = 256;
/// Cap on unshipped events held in memory during a control-plane outage. Beyond
/// it the oldest are dropped from the *control-plane copy* -- the local sink
/// still has every event.
const MAX_PENDING: usize = 10_000;

/// An [`AuditSink`] that records locally and also queues for the control plane.
pub struct ControlPlaneAuditSink {
    inner: Arc<dyn AuditSink>,
    tx: mpsc::Sender<AuditEvent>,
}

impl std::fmt::Debug for ControlPlaneAuditSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPlaneAuditSink").finish_non_exhaustive()
    }
}

impl ControlPlaneAuditSink {
    /// Wrap `inner` so events also flow to `tx` (whose receiver goes to
    /// [`run_audit_shipping`]).
    pub fn wrap(inner: Arc<dyn AuditSink>, tx: mpsc::Sender<AuditEvent>) -> Arc<dyn AuditSink> {
        Arc::new(Self { inner, tx })
    }

    /// The channel to pair a sink with its shipping task.
    pub fn channel() -> (mpsc::Sender<AuditEvent>, mpsc::Receiver<AuditEvent>) {
        mpsc::channel(AUDIT_BUFFER)
    }
}

impl AuditSink for ControlPlaneAuditSink {
    fn record(&self, event: &AuditEvent) {
        self.inner.record(event);
        if self.tx.try_send(event.clone()).is_err() {
            metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "dropped")
                .increment(1);
        }
    }
}

/// Drain the receiver, hash-chain batches, and ship them. Runs until the sender
/// is dropped. Persists the chain tail to `chain_path` after each accepted
/// batch, so a restart resumes the sequence.
pub async fn run_audit_shipping(
    control: ControlPlane,
    identity: GatewayIdentity,
    mut rx: mpsc::Receiver<AuditEvent>,
    chain_path: PathBuf,
) {
    let head = resume(&control, &identity, &chain_path).await;
    let mut next_seq = head.seq + 1;
    let mut prev_hash = head.hash;
    let mut pending: Vec<AuditEvent> = Vec::new();
    let mut backoff = Duration::from_secs(1);

    loop {
        if pending.is_empty() {
            match rx.recv().await {
                Some(event) => pending.push(event),
                None => return, // the gateway is shutting down
            }
        }
        while pending.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(event) => pending.push(event),
                Err(_) => break,
            }
        }

        let take = pending.len().min(BATCH_MAX);
        let (batch, batch_tail_hash) = chain_batch(&pending[..take], next_seq, &prev_hash);

        match control.ship_audit(&identity, &batch).await {
            Ok(_) => {
                next_seq += take as u64;
                prev_hash = batch_tail_hash;
                persist(&chain_path, next_seq - 1, &prev_hash);
                pending.drain(..take);
                metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "shipped")
                    .increment(take as u64);
                backoff = Duration::from_secs(1);
            }
            Err(error) => {
                metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "error")
                    .increment(1);
                let conflict = error.to_string().contains("409");
                tracing::warn!(
                    %error,
                    "shipping audit events failed; the local sink still has them"
                );
                if conflict {
                    // Our sequence disagrees with the control plane's chain --
                    // re-base on its head and rebuild the batch next loop.
                    if let Ok(head) = control.audit_head(&identity).await {
                        next_seq = head.seq + 1;
                        prev_hash = head.hash;
                        continue;
                    }
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                while pending.len() < MAX_PENDING {
                    match rx.try_recv() {
                        Ok(event) => pending.push(event),
                        Err(_) => break,
                    }
                }
                if pending.len() > MAX_PENDING {
                    let overflow = pending.len() - MAX_PENDING;
                    pending.drain(..overflow);
                    metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "dropped")
                        .increment(overflow as u64);
                    tracing::warn!(
                        overflow,
                        "audit ship backlog full; dropped oldest events from the control-plane \
                         copy (the local sink still has them)"
                    );
                }
            }
        }
    }
}

/// Where to resume the chain: the control plane's head, else the persisted
/// local tail, else genesis.
async fn resume(control: &ControlPlane, identity: &GatewayIdentity, chain_path: &std::path::Path) -> AuditHead {
    match control.audit_head(identity).await {
        Ok(head) => head,
        Err(error) => match std::fs::read(chain_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<AuditHead>(&b).ok())
        {
            Some(head) => {
                eprintln!(
                    "warning: could not fetch the audit head ({error}); \
                     resuming the chain from the local record at seq {}",
                    head.seq
                );
                head
            }
            None => {
                tracing::warn!(%error, "no audit head available; starting the chain from genesis");
                AuditHead::genesis()
            }
        },
    }
}

/// Sequence and hash a slice of events; returns the batch and the hash of its
/// last event (the new chain tail if it ships).
fn chain_batch(
    events: &[AuditEvent],
    first_seq: u64,
    prev_hash: &str,
) -> (Vec<ChainedAuditEvent>, String) {
    let mut batch = Vec::with_capacity(events.len());
    let mut hash = prev_hash.to_owned();
    for (i, event) in events.iter().enumerate() {
        let seq = first_seq + i as u64;
        let event_json = serde_json::to_string(event).unwrap_or_default();
        let this = audit_hash(seq, &hash, &event_json);
        batch.push(ChainedAuditEvent {
            seq,
            prev_hash: hash,
            event_json,
        });
        hash = this;
    }
    (batch, hash)
}

fn persist(chain_path: &std::path::Path, seq: u64, hash: &str) {
    let head = AuditHead {
        seq,
        hash: hash.to_owned(),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&head) {
        let _ = std::fs::write(chain_path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(decision: &'static str) -> AuditEvent {
        AuditEvent {
            timestamp: "2026-03-01T00:00:00.000Z".into(),
            decision,
            protocol: "connect-tcp",
            application: "terraform".into(),
            destination: "db:5432".into(),
            client: "10.0.0.1:5000".into(),
            identity: Default::default(),
            policy: None,
            rule: None,
            reason: None,
            suggested_rule: None,
        }
    }

    #[test]
    fn chain_batch_links_each_event_to_the_last() {
        let genesis = crate::control::AUDIT_GENESIS;
        let events = [event("allow"), event("deny"), event("allow")];
        let (batch, tail) = chain_batch(&events, 1, genesis);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].seq, 1);
        assert_eq!(batch[0].prev_hash, genesis);
        assert_eq!(
            batch[1].prev_hash,
            audit_hash(1, genesis, &batch[0].event_json)
        );
        assert_eq!(batch[2].seq, 3);
        assert_eq!(tail, audit_hash(3, &batch[2].prev_hash, &batch[2].event_json));
    }
}
