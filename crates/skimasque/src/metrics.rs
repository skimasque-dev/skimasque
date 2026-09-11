//! Gateway metrics, emitted through the [`metrics`] facade.
//!
//! The proxy calls these on the connection and tunnel lifecycle; a binary that
//! installs a recorder (`skimasque-server` installs the Prometheus exporter)
//! collects them, and without one they compile to near-nothing. Naming the
//! series here keeps the label sets in one place.
//!
//! | Series | Kind | Labels |
//! |---|---|---|
//! | `skimasque_connections_accepted_total`  | counter | — |
//! | `skimasque_connections_refused_total`   | counter | `reason` = `connection_limit` \| `rate_limit` \| `per_source_rate` |
//! | `skimasque_connections_active`          | gauge   | — |
//! | `skimasque_tunnels_opened_total`        | counter | `protocol` = `connect-udp` \| `connect-tcp` |
//! | `skimasque_tunnels_rejected_total`      | counter | `reason` = `per_connection_limit` |
//! | `skimasque_tunnels_active`              | gauge   | — |
//! | `skimasque_bytes_relayed_total`         | counter | `direction` = `to_target` \| `to_client` |
//! | `skimasque_reloads_total`               | counter | `kind` = `policy` \| `tls`; `outcome` = `ok` \| `error` |
//! | `skimasque_acme_events_total`           | counter | `kind` = `deployed_new` \| `deployed_cached` \| `cache_store` \| `challenge` \| `error` |
//! | `skimasque_rate_limit_throttled_total`  | counter | `limit` = `bandwidth` \| `packets` |
//! | `skimasque_rate_limit_throttled_millis_total` | counter | `limit` = `bandwidth` \| `packets` |
//! | `skimasque_transfer_cap_reached_total`  | counter | — |
//! | `skimasque_exchange_throttled_total`    | counter | — |

use std::sync::atomic::{AtomicU64, Ordering};

use metrics::{counter, describe_counter, describe_gauge, gauge, Unit};

// Process-lifetime totals the gateway reports to the control plane for usage
// accounting (Phase 4). They are bumped alongside the Prometheus counters, which
// the `metrics` facade does not let us read back.
static TUNNELS_OPENED: AtomicU64 = AtomicU64::new(0);
static BYTES_TO_TARGET: AtomicU64 = AtomicU64::new(0);
static BYTES_TO_CLIENT: AtomicU64 = AtomicU64::new(0);

/// A cumulative-since-process-start view of the gateway's throughput.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub tunnels_opened: u64,
    pub bytes_to_target: u64,
    pub bytes_to_client: u64,
}

/// The current totals. The control plane turns successive snapshots into a
/// per-org accumulator (a decrease means the gateway restarted).
pub fn usage_snapshot() -> UsageSnapshot {
    UsageSnapshot {
        tunnels_opened: TUNNELS_OPENED.load(Ordering::Relaxed),
        bytes_to_target: BYTES_TO_TARGET.load(Ordering::Relaxed),
        bytes_to_client: BYTES_TO_CLIENT.load(Ordering::Relaxed),
    }
}

/// Register the series with help text and units, and set the unlabeled ones to
/// zero so a freshly started gateway's `/metrics` is not blank. Call once, right
/// after installing a recorder.
pub fn describe() {
    describe_counter!(
        "skimasque_connections_accepted_total",
        "QUIC connections that cleared the accept-path checks and got a handshake task"
    );
    describe_counter!(
        "skimasque_connections_refused_total",
        "QUIC connections refused before a handshake task was spawned (label: reason)"
    );
    describe_gauge!(
        "skimasque_connections_active",
        "QUIC connections currently being served"
    );
    describe_counter!(
        "skimasque_tunnels_opened_total",
        "Tunnels accepted and relayed (label: protocol)"
    );
    describe_counter!(
        "skimasque_tunnels_rejected_total",
        "Tunnel requests refused (label: reason)"
    );
    describe_gauge!(
        "skimasque_tunnels_active",
        "Tunnels currently relaying payload"
    );
    describe_counter!(
        "skimasque_bytes_relayed_total",
        Unit::Bytes,
        "Payload bytes moved through tunnels (label: direction)"
    );
    describe_counter!(
        "skimasque_reloads_total",
        "Policy and TLS hot-reload attempts (labels: kind, outcome)"
    );
    describe_counter!(
        "skimasque_acme_events_total",
        "ACME certificate lifecycle events (label: kind)"
    );
    describe_counter!(
        "skimasque_rate_limit_throttled_total",
        "Relay chunks parked on a policy rate limit (label: limit = bandwidth | packets)"
    );
    describe_counter!(
        "skimasque_rate_limit_throttled_millis_total",
        Unit::Milliseconds,
        "Total time relay chunks spent parked on a policy rate limit (label: limit)"
    );
    describe_counter!(
        "skimasque_transfer_cap_reached_total",
        "Tunnels stopped for hitting their policy's total_bytes ceiling"
    );
    describe_counter!(
        "skimasque_exchange_throttled_total",
        "Token-exchange requests refused 429 for exceeding a source's rate"
    );

    counter!("skimasque_connections_accepted_total").increment(0);
    gauge!("skimasque_connections_active").set(0.0);
    gauge!("skimasque_tunnels_active").set(0.0);
}

/// A QUIC connection cleared the accept-path checks and got a handshake task.
pub fn connection_accepted() {
    counter!("skimasque_connections_accepted_total").increment(1);
}

/// A `quinn::Incoming` was refused before a handshake task was spawned.
pub fn connection_refused(reason: &'static str) {
    counter!("skimasque_connections_refused_total", "reason" => reason).increment(1);
}

/// The connection task has started; pair with [`connection_ended`].
pub fn connection_started() {
    gauge!("skimasque_connections_active").increment(1.0);
}

/// The connection task has ended.
pub fn connection_ended() {
    gauge!("skimasque_connections_active").decrement(1.0);
}

/// A tunnel was accepted and is about to start relaying.
pub fn tunnel_opened(protocol: &'static str) {
    counter!("skimasque_tunnels_opened_total", "protocol" => protocol).increment(1);
    gauge!("skimasque_tunnels_active").increment(1.0);
    TUNNELS_OPENED.fetch_add(1, Ordering::Relaxed);
}

/// A relaying tunnel has finished.
pub fn tunnel_closed() {
    gauge!("skimasque_tunnels_active").decrement(1.0);
}

/// A tunnel request was turned down (currently only the per-connection cap).
pub fn tunnel_rejected(reason: &'static str) {
    counter!("skimasque_tunnels_rejected_total", "reason" => reason).increment(1);
}

/// Payload bytes moved through a tunnel. `direction` is `to_target` or
/// `to_client`.
pub fn bytes_relayed(direction: &'static str, bytes: u64) {
    counter!("skimasque_bytes_relayed_total", "direction" => direction).increment(bytes);
    let total = if direction == "to_target" {
        &BYTES_TO_TARGET
    } else {
        &BYTES_TO_CLIENT
    };
    total.fetch_add(bytes, Ordering::Relaxed);
}

/// A hot-reload attempt finished. `kind` is `policy` or `tls`; `outcome` is `ok`
/// or `error`.
pub fn reload(kind: &'static str, outcome: &'static str) {
    counter!("skimasque_reloads_total", "kind" => kind, "outcome" => outcome).increment(1);
}

/// An ACME certificate lifecycle event. `kind` is `deployed_new` (fresh cert or
/// a renewal), `deployed_cached` (a still-valid cert read from disk at start),
/// `cache_store`, `challenge` (a validation connection was served), or `error`.
pub fn acme_event(kind: &'static str) {
    counter!("skimasque_acme_events_total", "kind" => kind).increment(1);
}

/// A relay chunk was parked on a policy rate limit, for `parked`. `limit` is
/// `bandwidth` or `packets`.
pub fn rate_limit_throttled(limit: &'static str, parked: std::time::Duration) {
    counter!("skimasque_rate_limit_throttled_total", "limit" => limit).increment(1);
    counter!("skimasque_rate_limit_throttled_millis_total", "limit" => limit)
        .increment(parked.as_millis().min(u128::from(u64::MAX)) as u64);
}

/// A tunnel was stopped because it hit its policy's `total_bytes` ceiling.
pub fn transfer_cap_reached() {
    counter!("skimasque_transfer_cap_reached_total").increment(1);
}

/// A token-exchange request was refused `429` for exceeding its source's rate.
pub fn exchange_throttled() {
    counter!("skimasque_exchange_throttled_total").increment(1);
}
