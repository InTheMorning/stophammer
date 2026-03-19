//! Event types and signing payload for the stophammer sync protocol.
//!
//! [`Event`] is the immutable sync primitive replicated between all nodes.
//! Each event carries an [`EventPayload`] (one of several domain-specific
//! variants), an ed25519 signature over [`EventSigningPayload`], and a
//! monotonic `seq` assigned by the primary at commit time.
//!
//! `seq` is included in the signing payload to prevent MITM inflation of
//! the delivery-ordering cursor (Issue-SEQ-INTEGRITY).

use crate::model::{
    Artist, ArtistCredit, Feed, FeedPaymentRoute, FeedRemoteItemRaw, LiveEvent, PaymentRoute,
    Recording, Release, ReleaseRecording, SourceContributorClaim, SourceEntityIdClaim,
    SourceEntityLink, SourceFeedReleaseMap, SourceItemEnclosure, SourceItemRecordingMap,
    SourcePlatformClaim, SourceReleaseClaim, Track, ValueTimeSplit,
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
    /// Two artist records were merged; one was absorbed into the other.
    ArtistMerged,
    /// An artist credit was created (multi-artist attribution).
    ArtistCreditCreated,
    /// Feed-level payment routes were replaced.
    FeedRoutesReplaced,
    /// Feed-level remote-item references were replaced.
    FeedRemoteItemsReplaced,
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
    /// The staged platform-claim snapshot for a feed was replaced.
    SourcePlatformClaimsReplaced,
    /// The primary resolver replaced canonical release/recording state for a feed.
    CanonicalFeedStateReplaced,
    /// The primary resolver replaced promoted IDs and provenance for a feed.
    CanonicalFeedPromotionsReplaced,
    /// The primary resolver completed artist-identity work for a feed.
    ArtistIdentityFeedResolved,
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
    /// Payload for an artist merge event.
    ArtistMerged(ArtistMergedPayload),
    /// Payload for an artist credit creation event.
    ArtistCreditCreated(ArtistCreditCreatedPayload),
    /// Payload for a feed-level payment route replacement event.
    FeedRoutesReplaced(FeedRoutesReplacedPayload),
    /// Payload for replacing feed-level `podcast:remoteItem` references.
    FeedRemoteItemsReplaced(FeedRemoteItemsReplacedPayload),
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
    /// Payload for replacing staged platform claims for a feed.
    SourcePlatformClaimsReplaced(SourcePlatformClaimsReplacedPayload),
    /// Payload for replacing canonical release/recording state for a feed.
    CanonicalFeedStateReplaced(CanonicalFeedStateReplacedPayload),
    /// Payload for replacing canonical promoted IDs and provenance for a feed.
    CanonicalFeedPromotionsReplaced(CanonicalFeedPromotionsReplacedPayload),
    /// Payload for authoritative completion of feed-scoped artist identity work.
    ArtistIdentityFeedResolved(ArtistIdentityFeedResolvedPayload),
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
    pub artist: Artist,
    pub artist_credit: ArtistCredit,
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
    pub artist_credit: ArtistCredit,
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
    pub routes: Vec<PaymentRoute>,
}

/// Emitted when two artist records are merged: `source_artist_id` is absorbed
/// into `target_artist_id` and then deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistMergedPayload {
    /// The artist that was absorbed and deleted.
    pub source_artist_id: String,
    /// The artist that now owns all feeds, tracks, and aliases.
    pub target_artist_id: String,
    /// Alias strings (lowercased) transferred from the source to the target.
    pub aliases_transferred: Vec<String>,
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

/// Emitted when the full set of staged platform claims for a feed is replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePlatformClaimsReplacedPayload {
    pub feed_guid: String,
    pub claims: Vec<SourcePlatformClaim>,
}

/// Emitted by the primary resolver when it replaces canonical release and
/// recording state derived from one source feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalFeedStateReplacedPayload {
    pub feed_guid: String,
    pub releases: Vec<Release>,
    pub recordings: Vec<Recording>,
    pub release_recordings: Vec<ReleaseRecording>,
    pub release_maps: Vec<SourceFeedReleaseMap>,
    pub recording_maps: Vec<SourceItemRecordingMap>,
}

/// Emitted by the primary resolver when it replaces promoted external IDs and
/// provenance derived from one source feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalFeedPromotionsReplacedPayload {
    pub feed_guid: String,
    pub external_ids: Vec<crate::model::ResolvedExternalIdByFeed>,
    pub entity_sources: Vec<crate::model::ResolvedEntitySourceByFeed>,
}

/// Emitted by the primary resolver when artist-identity work for one feed has
/// converged for the current source state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistIdentityFeedResolvedPayload {
    pub feed_guid: String,
    pub seed_artists: usize,
    pub candidate_groups: usize,
    pub groups_processed: usize,
    pub merges_applied: usize,
    pub pending_reviews: usize,
    pub blocked_reviews: usize,
}
