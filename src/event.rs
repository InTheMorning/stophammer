// Rust guideline compliant (M-APP-ERROR, M-MODULE-DOCS) — 2026-03-09

//! Event types and signing payload for the stophammer sync protocol.
//!
//! [`Event`] is the immutable sync primitive replicated between all nodes.
//! Each event carries an [`EventPayload`] (one of several domain-specific
//! variants), an ed25519 signature over [`EventSigningPayload`], and a
//! monotonic `seq` assigned by the primary at commit time.
//!
//! `seq` is intentionally excluded from the signing payload — it is a
//! delivery-ordering field and does not affect content integrity.

use serde::{Deserialize, Serialize};
use crate::model::{Artist, Feed, PaymentRoute, Track, ValueTimeSplit};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    FeedUpserted,
    FeedRetired,
    TrackUpserted,
    TrackRemoved,
    ArtistUpserted,
    RoutesReplaced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
    FeedUpserted(FeedUpsertedPayload),
    FeedRetired(FeedRetiredPayload),
    TrackUpserted(TrackUpsertedPayload),
    TrackRemoved(TrackRemovedPayload),
    ArtistUpserted(ArtistUpsertedPayload),
    RoutesReplaced(RoutesReplacedPayload),
}

/// The full signed event — the sync primitive between all nodes.
#[expect(clippy::struct_field_names, reason = "event_id and event_type are canonical field names in the protocol")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id:     String,
    pub event_type:   EventType,
    pub payload:      EventPayload,
    pub subject_guid: String,
    pub signed_by:    String,       // hex ed25519 pubkey
    pub signature:    String,       // hex ed25519 sig over sha256(EventSigningPayload)
    pub seq:          i64,          // monotonic, assigned by primary at commit
    pub created_at:   i64,          // unix seconds
    pub warnings:     Vec<String>,  // verifier warnings stored for audit
}

/// Canonical form that gets signed.
/// `payload_json` is pre-serialized to avoid any encoding ambiguity.
/// seq is intentionally excluded — it is a delivery-ordering field, not content integrity.
#[derive(Debug, Serialize)]
pub struct EventSigningPayload<'a> {
    pub event_id:     &'a str,
    pub event_type:   &'a EventType,
    pub payload_json: &'a str,
    pub subject_guid: &'a str,
    pub created_at:   i64,
}

// ── Payload types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedUpsertedPayload {
    pub feed:   Feed,
    pub artist: Artist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRetiredPayload {
    pub feed_guid: String,
    pub reason:    Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackUpsertedPayload {
    pub track:             Track,
    pub routes:            Vec<PaymentRoute>,
    pub value_time_splits: Vec<ValueTimeSplit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRemovedPayload {
    pub track_guid: String,
    pub feed_guid:  String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistUpsertedPayload {
    pub artist: Artist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutesReplacedPayload {
    pub track_guid: String,
    pub routes:     Vec<PaymentRoute>,
}
