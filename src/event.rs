//! Event types and signing payload for the stophammer sync protocol.
//!
//! [`Event`] is the immutable sync primitive replicated between all nodes.
//! Each event carries an [`EventPayload`] (one of several domain-specific
//! variants), an ed25519 signature over [`EventSigningPayload`], and a
//! monotonic `seq` assigned by the primary at commit time.
//!
//! `seq` is included in the signing payload to prevent MITM inflation of
//! the delivery-ordering cursor (Issue-SEQ-INTEGRITY).

use std::collections::BTreeMap;

use crate::model::{
    Artist, ArtistCredit, Feed, FeedBlockKind, FeedPaymentRoute, FeedRemoteItemRaw, LiveEvent,
    PaymentRoute, RouteRecipient, SourceContributorClaim, SourceEntityIdClaim, SourceEntityLink,
    SourceItemEnclosure, SourceItemTranscript, SourcePlatformClaim, SourceReleaseClaim, Track,
    TrackRemoteItemRaw, ValueTimeSplit,
};
use serde::{Deserialize, Serialize};

/// Discriminant identifying which domain action produced an [`Event`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// A feed was created or its metadata changed.
    FeedUpserted,
    /// A feed was permanently removed from the index.
    FeedRetired,
    /// A track was created or its metadata/payment routes changed.
    TrackUpserted,
    /// A track was deleted from a feed.
    TrackRemoved,
    /// An artist record was created or its display name changed.
    ArtistUpserted,
    /// The full set of payment routes for a track was atomically replaced.
    RoutesReplaced,
    /// An artist credit was created (multi-artist attribution).
    ArtistCreditCreated,
    /// Feed-level payment routes were replaced.
    FeedRoutesReplaced,
    /// Feed-level remote-item references were replaced.
    FeedRemoteItemsReplaced,
    /// Per-track remote-item references were replaced.
    TrackRemoteItemsReplaced,
    /// The ephemeral live-event snapshot for a feed was replaced.
    LiveEventsReplaced,
    /// The staged contributor-claim snapshot for a feed was replaced.
    SourceContributorClaimsReplaced,
    /// The staged entity-ID snapshot for a feed was replaced.
    SourceEntityIdsReplaced,
    /// The staged entity-link snapshot for a feed was replaced.
    SourceEntityLinksReplaced,
    /// The staged release-claim snapshot for a feed was replaced.
    SourceReleaseClaimsReplaced,
    /// The staged item-enclosure snapshot for a feed was replaced.
    SourceItemEnclosuresReplaced,
    /// The staged item-transcript snapshot for a feed was replaced.
    SourceItemTranscriptsReplaced,
    /// The staged platform-claim snapshot for a feed was replaced.
    SourcePlatformClaimsReplaced,
    /// A URL was observed to give a `podcast:guid`.
    FeedUrlObserved,
    /// A feed GUID or URL was blocked from ingest (ADR 0053 Section 1).
    FeedBlocked,
    /// A previously blocked feed GUID or URL was unblocked (ADR 0053 Section 1).
    FeedUnblocked,
    /// The summary of a mirror body at a URL is new or changed (ADR 0058 §1).
    FeedCopyObserved,
    /// The operator resolved an open copy (ADR 0058 §4).
    FeedCopyResolved,
    /// A source URL's declared GUID is new or changed, or it returned to the
    /// GUID the record already holds (ADR 0052 §4, task 007).
    FeedGuidChangeObserved,
    /// The operator approved or rejected a pending GUID change (ADR 0052 §4,
    /// task 007).
    FeedGuidChangeDecided,
    /// A GUID change transition retired the old record and linked it to the
    /// new one (ADR 0052 §5, task 007).
    FeedGuidSuperseded,
}

/// Typed payload carried inside an [`Event`]; variant mirrors [`EventType`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
    /// Payload for a feed create-or-update event.
    FeedUpserted(FeedUpsertedPayload),
    /// Payload for a feed removal event.
    FeedRetired(FeedRetiredPayload),
    /// Payload for a track create-or-update event.
    TrackUpserted(TrackUpsertedPayload),
    /// Payload for a track deletion event.
    TrackRemoved(TrackRemovedPayload),
    /// Payload for an artist create-or-update event.
    ArtistUpserted(ArtistUpsertedPayload),
    /// Payload for an atomic payment-route replacement event.
    RoutesReplaced(RoutesReplacedPayload),
    /// Payload for an artist credit creation event.
    ArtistCreditCreated(ArtistCreditCreatedPayload),
    /// Payload for a feed-level payment route replacement event.
    FeedRoutesReplaced(FeedRoutesReplacedPayload),
    /// Payload for replacing feed-level `podcast:remoteItem` references.
    FeedRemoteItemsReplaced(FeedRemoteItemsReplacedPayload),
    /// Payload for replacing per-track `podcast:remoteItem` references.
    TrackRemoteItemsReplaced(TrackRemoteItemsReplacedPayload),
    /// Payload for replacing ephemeral live-event rows for a feed.
    LiveEventsReplaced(LiveEventsReplacedPayload),
    /// Payload for replacing staged contributor claims for a feed.
    SourceContributorClaimsReplaced(SourceContributorClaimsReplacedPayload),
    /// Payload for replacing staged entity IDs for a feed.
    SourceEntityIdsReplaced(SourceEntityIdsReplacedPayload),
    /// Payload for replacing staged entity links for a feed.
    SourceEntityLinksReplaced(SourceEntityLinksReplacedPayload),
    /// Payload for replacing staged release claims for a feed.
    SourceReleaseClaimsReplaced(SourceReleaseClaimsReplacedPayload),
    /// Payload for replacing staged item enclosures for a feed.
    SourceItemEnclosuresReplaced(SourceItemEnclosuresReplacedPayload),
    /// Payload for replacing staged item transcripts for a feed.
    SourceItemTranscriptsReplaced(SourceItemTranscriptsReplacedPayload),
    /// Payload for replacing staged platform claims for a feed.
    SourcePlatformClaimsReplaced(SourcePlatformClaimsReplacedPayload),
    /// Payload for a URL-observation event.
    FeedUrlObserved(FeedUrlObservedPayload),
    /// Payload for a feed-block event.
    FeedBlocked(FeedBlockedPayload),
    /// Payload for a feed-unblock event.
    FeedUnblocked(FeedUnblockedPayload),
    /// Payload for a mirror-summary-observed event.
    FeedCopyObserved(FeedCopyObservedPayload),
    /// Payload for a copy-resolution event.
    FeedCopyResolved(FeedCopyResolvedPayload),
    /// Payload for a pending-GUID-change-observed event.
    FeedGuidChangeObserved(FeedGuidChangeObservedPayload),
    /// Payload for a GUID-change-decision event.
    FeedGuidChangeDecided(FeedGuidChangeDecidedPayload),
    /// Payload for a GUID-change-transition event.
    FeedGuidSuperseded(FeedGuidSupersededPayload),
}

/// The full signed event — the sync primitive between all nodes.
///
/// `payload_json` carries the canonical inner-payload JSON string that was
/// used when computing the ed25519 signature. It is **not** included in the
/// wire representation (`#[serde(skip)]`) because the typed `payload` field
/// already covers the content; it exists solely so `verify_event_signature`
/// can hash exactly the same bytes that were signed, without re-serializing
/// through `serde_json::Value` (which sorts object keys alphabetically and
/// would produce a different digest).
///
/// Callers that construct `Event` from the wire (community sync) must populate
/// `payload_json` using [`Event::payload_json_from_payload`] before calling
/// `verify_event_signature`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub event_type: EventType,
    pub payload: EventPayload,
    pub subject_guid: String,
    pub signed_by: String,     // hex ed25519 pubkey
    pub signature: String,     // hex ed25519 sig over sha256(EventSigningPayload)
    pub seq: i64,              // monotonic, assigned by primary at commit
    pub created_at: i64,       // unix seconds
    pub warnings: Vec<String>, // verifier warnings stored for audit
    /// Canonical inner-payload JSON string used when computing the ed25519 signature.
    ///
    /// Transmitted over the wire so community nodes can verify the signature
    /// against the exact bytes that were signed, without re-serializing the
    /// typed `payload` (which would produce alphabetically-sorted keys via
    /// `serde_json::Value` and break the digest).
    pub payload_json: String,
}

/// Canonical byte representation that is hashed and signed with ed25519.
///
/// `payload_json` is pre-serialized to avoid any re-encoding ambiguity that
/// could arise from round-tripping through a typed value.
///
/// `seq` is included so that a MITM cannot inflate the delivery-ordering
/// cursor by altering the unsigned wire value.  The primary assigns `seq`
/// at commit time and signs it; replicas verify the signature before
/// advancing their sync cursor, closing the seq-inflation attack vector.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[derive(Debug, Serialize)]
pub struct EventSigningPayload<'a> {
    pub event_id: &'a str,
    pub event_type: &'a EventType,
    pub payload_json: &'a str,
    pub subject_guid: &'a str,
    pub created_at: i64,
    pub seq: i64, // Issue-SEQ-INTEGRITY — 2026-03-14
}

// ── Payload types ──────────────────────────────────────────────────────────

/// Emitted when a feed is created or any of its metadata fields change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedUpsertedPayload {
    pub feed: Feed,
    /// The operator's reason for an ADR 0058 section 5 relocation. `None`
    /// outside a relocation. The apply step does not read this field; the
    /// updated `feed` already carries every column a replica applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Emitted when a feed is permanently removed from the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRetiredPayload {
    pub feed_guid: String,
    pub reason: Option<String>,
}

/// Emitted when a track is created or its metadata, routes, or time-splits change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackUpsertedPayload {
    pub track: Track,
    pub routes: Vec<PaymentRoute>,
    pub value_time_splits: Vec<ValueTimeSplit>,
}

/// Emitted when a track is deleted from its parent feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRemovedPayload {
    pub track_guid: String,
    pub feed_guid: String,
}

/// Emitted when an artist record is created or its display name changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistUpsertedPayload {
    pub artist: Artist,
}

/// Emitted when the full payment-route set for a track is atomically replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutesReplacedPayload {
    pub track_guid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_guid: Option<String>,
    pub routes: Vec<PaymentRoute>,
}

/// Emitted when a new artist credit is created (multi-artist attribution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistCreditCreatedPayload {
    pub artist_credit: ArtistCredit,
}

/// Emitted when feed-level payment routes are atomically replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRoutesReplacedPayload {
    pub feed_guid: String,
    pub routes: Vec<FeedPaymentRoute>,
}

/// Emitted when the full set of feed-level `podcast:remoteItem` refs is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRemoteItemsReplacedPayload {
    pub feed_guid: String,
    pub remote_items: Vec<FeedRemoteItemRaw>,
}

/// Emitted when the full set of per-track `podcast:remoteItem` refs is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRemoteItemsReplacedPayload {
    pub track_guid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_guid: Option<String>,
    pub remote_items: Vec<TrackRemoteItemRaw>,
}

/// Emitted when the current in-progress live items for a feed are replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveEventsReplacedPayload {
    pub feed_guid: String,
    pub live_events: Vec<LiveEvent>,
}

/// Emitted when the full set of staged contributor claims for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceContributorClaimsReplacedPayload {
    pub feed_guid: String,
    pub claims: Vec<SourceContributorClaim>,
}

/// Emitted when the full set of staged entity-ID claims for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntityIdsReplacedPayload {
    pub feed_guid: String,
    pub claims: Vec<SourceEntityIdClaim>,
}

/// Emitted when the full set of staged entity links for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntityLinksReplacedPayload {
    pub feed_guid: String,
    pub links: Vec<SourceEntityLink>,
}

/// Emitted when the full set of staged release claims for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReleaseClaimsReplacedPayload {
    pub feed_guid: String,
    pub claims: Vec<SourceReleaseClaim>,
}

/// Emitted when the full set of staged item enclosures for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceItemEnclosuresReplacedPayload {
    pub feed_guid: String,
    pub enclosures: Vec<SourceItemEnclosure>,
}

/// Emitted when the full set of staged item transcripts for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceItemTranscriptsReplacedPayload {
    pub feed_guid: String,
    pub transcripts: Vec<SourceItemTranscript>,
}

/// Emitted when the full set of staged platform claims for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePlatformClaimsReplacedPayload {
    pub feed_guid: String,
    pub claims: Vec<SourcePlatformClaim>,
}

/// Emitted when the node observes that a URL gives a `podcast:guid`.
///
/// ADR 0049 Section 1. `url` is `canonical_url` or `source_url` from the
/// ingest request. `feed_guid` is the `podcast:guid` the feed body carried.
/// A community node applies this to derive the same URL-to-GUID table the
/// primary node holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedUrlObservedPayload {
    pub url: String,
    pub feed_guid: String,
    pub observed_at: i64,
}

/// Emitted when a feed GUID or URL is blocked from ingest.
///
/// ADR 0053 Section 1. `block_id` is the primary-assigned UUID that names the
/// row in `feed_blocks`, so every node stores the same row under the same
/// identity. The subject GUID of the [`Event`] carrying this payload is
/// `block_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedBlockedPayload {
    pub block_id: String,
    pub kind: FeedBlockKind,
    pub value: String,
    pub reason: String,
    pub blocked_at: i64,
}

/// Emitted when a previously blocked feed GUID or URL is unblocked.
///
/// ADR 0053 Section 1. `block_id` names the `feed_blocks` row to remove.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedUnblockedPayload {
    pub block_id: String,
}

/// Emitted when the ADR 0058 Section 1 summary of a mirror body at `url` is
/// new or changes for `feed_guid`.
///
/// The subject GUID of the [`Event`] carrying this payload is `feed_guid`.
/// `first_seen` is the time the primary first saw this pair. A replayed
/// event for the same pair carries the same `first_seen`, so applying it
/// keeps the row's original value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedCopyObservedPayload {
    pub feed_guid: String,
    pub url: String,
    pub first_seen: i64,
    pub title: String,
    pub item_guids: Vec<String>,
    pub feed_recipients: Vec<RouteRecipient>,
    pub track_recipients: BTreeMap<String, Vec<RouteRecipient>>,
    pub summary_digest: String,
}

/// Emitted when the operator resolves an open copy of `feed_guid` at `url`
/// (ADR 0058 Section 4).
///
/// The subject GUID of the [`Event`] carrying this payload is `feed_guid`.
/// `decision` is `"keep_source"` or `"relocate"`. `resolved_digest` is the
/// `summary_digest` of the row at the time of the resolution. The
/// resolution holds only while the row keeps that digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedCopyResolvedPayload {
    pub feed_guid: String,
    pub url: String,
    pub decision: String,
    pub reason: String,
    pub resolved_at: i64,
    pub resolved_digest: String,
}

/// Emitted when the `feed_guid_changes` row of `source_url` is new, or its
/// `new_guid` changes (ADR 0052 section 4, task 007).
///
/// `new_guid` equal to `old_guid` is the delete marker: the source URL
/// returned to the GUID the record already holds, and the apply step
/// deletes the row instead of upserting it. There is no separate event type
/// for a delete; task 007 chose to reuse this one, so a community node needs
/// no new match arm for the return case.
///
/// The subject GUID of the [`Event`] carrying this payload is `old_guid`:
/// the record the pending change concerns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedGuidChangeObservedPayload {
    pub source_url: String,
    pub old_guid: String,
    pub new_guid: String,
    pub first_seen: i64,
}

/// Emitted when the operator decides a pending GUID change (ADR 0052 section
/// 4, task 007).
///
/// The subject GUID of the [`Event`] carrying this payload is `old_guid`.
/// `decision` is `"approve"` or `"reject"`. The node does not keep the body
/// of the submission that opened the row: `approve` only sets the decision,
/// and the transition runs at the next submission of `new_guid` from
/// `source_url`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedGuidChangeDecidedPayload {
    pub source_url: String,
    pub old_guid: String,
    pub new_guid: String,
    pub decision: String,
    pub reason: String,
    pub decided_at: i64,
}

/// Emitted when a GUID-change transition retires `old_guid` and links it to
/// `new_guid` (ADR 0052 section 5, task 007).
///
/// The subject GUID of the [`Event`] carrying this payload is `old_guid`.
/// This event carries no fields of the retired record or the admitted one:
/// the transition also emits the ordinary `FeedRetired` event for `old_guid`
/// and the ordinary `FeedUpserted`/`TrackUpserted` events that admit the new
/// record at `new_guid`, so this payload is the link between the two, for
/// navigation only. `GET /v1/feeds/{old_guid}` answers `404` with
/// `superseded_by` from the row this event writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedGuidSupersededPayload {
    pub old_guid: String,
    pub new_guid: String,
    pub source_url: String,
}
