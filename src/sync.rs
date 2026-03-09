// Rust guideline compliant (M-APP-ERROR, M-MODULE-DOCS) — 2026-03-09

//! Node-to-node sync protocol types.
//!
//! [`SyncEventsResponse`] is returned by `GET /sync/events` for incremental
//! polling. [`ReconcileRequest`] / [`ReconcileResponse`] implement a
//! negentropy-style set-difference handshake used when a community node
//! rejoins after downtime: the node sends what it has, the primary returns
//! what it is missing and flags any event IDs unknown to the primary.

use serde::{Deserialize, Serialize};
use crate::event::Event;

/// Response for `GET /sync/events?after_seq={n}&limit={m}`.
/// Community nodes poll this to stay current.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncEventsResponse {
    pub events:   Vec<Event>,
    pub has_more: bool,
    pub next_seq: i64,
}

/// POST /sync/reconcile
/// Negentropy-style diff: node sends what it has, primary returns what it's missing.
/// Used when a node comes back online after downtime.
#[derive(Debug, Deserialize)]
pub struct ReconcileRequest {
    pub node_pubkey: String,
    pub have:        Vec<EventRef>,
    pub since_seq:   i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventRef {
    pub event_id: String,
    pub seq:      i64,
}

#[derive(Debug, Serialize)]
pub struct ReconcileResponse {
    pub send_to_node:  Vec<Event>,     // events the node is missing
    pub unknown_to_us: Vec<EventRef>,  // node has these, primary doesn't (anomaly)
}
