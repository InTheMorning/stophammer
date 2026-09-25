//! Core domain types for the stophammer feed index.
//!
//! Defines the persisted entities: [`Artist`], [`ArtistCredit`],
//! [`ArtistCreditName`], [`Feed`], [`Track`], [`PaymentRoute`],
//! [`FeedPaymentRoute`], [`ValueTimeSplit`], [`FeedRemoteItemRaw`],
//! [`LiveEvent`], [`SourceContributorClaim`], [`SourceEntityLink`],
//! [`SourceEntityIdClaim`], [`SourceReleaseClaim`], [`SourceItemEnclosure`],
//! and [`SourcePlatformClaim`].
//! All types derive `Serialize` and `Deserialize` so they can be embedded in
//! event payloads and returned from API endpoints without additional mapping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::ingest::{IngestFeedData, IngestPaymentRoute};

// Field names intentionally repeat the struct prefix (e.g. artist_id, feed_guid)
// because these are canonical Podcast Namespace identifiers used verbatim in
// SQLite columns, JSON payloads, and the RSS/Podcast Index spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub artist_id: String,
    pub name: String,
    pub name_lower: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub begin_year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_year: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// MusicBrainz-style artist credit: a display name for multi-artist attribution.
// Issue-ARTIST-IDENTITY — 2026-03-14
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistCredit {
    pub id: i64,
    pub display_name: String,
    /// Feed GUID that scopes this credit, preventing cross-feed name collisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_guid: Option<String>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<ArtistCreditName>,
}

/// Individual entry within an [`ArtistCredit`], linking to the underlying [`Artist`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistCreditName {
    pub id: i64,
    pub artist_credit_id: i64,
    pub artist_id: String,
    pub position: i64,
    pub name: String,
    #[serde(default)]
    pub join_phrase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    pub feed_guid: String,
    pub feed_url: String,
    pub title: String,
    pub title_lower: String,
    pub artist_credit_id: i64,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub explicit: bool,
    pub itunes_type: Option<String>,
    pub release_artist: Option<String>,
    pub release_artist_sort: Option<String>,
    pub release_date: Option<i64>,
    pub release_kind: Option<String>,
    pub episode_count: i64,
    pub newest_item_at: Option<i64>,
    pub oldest_item_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Verbatim value of the `podcast:medium` tag from the RSS feed, if present.
    pub raw_medium: Option<String>,
    /// Channel `lastBuildDate` as Unix seconds, if the feed published one.
    ///
    /// This is the time the feed file was generated. It is not a release date.
    /// ADR 0043 owns this distinction.
    #[serde(default)]
    pub last_build_date: Option<i64>,
    /// The source of `release_artist`: `"itunes_author"`, `"itunes_owner"`, or
    /// `"placeholder"`.
    ///
    /// Null when no ingest has run since migration 0037 added the column.
    /// ADR 0049 §5 owns this field.
    #[serde(default)]
    pub release_artist_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub track_guid: String,
    pub feed_guid: String,
    pub artist_credit_id: i64,
    pub title: String,
    /// Pre-lowercased copy of `title` used for case-insensitive search queries.
    pub title_lower: String,
    pub pub_date: Option<i64>,
    pub duration_secs: Option<i64>,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub enclosure_bytes: Option<i64>,
    pub track_number: Option<i64>,
    pub season: Option<i64>,
    pub image_url: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub explicit: bool,
    pub description: Option<String>,
    pub track_artist: Option<String>,
    pub track_artist_sort: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

// Issue-11 RouteType alignment — 2026-03-13
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RouteType {
    Node,
    Wallet,
    Keysend,
    Lnaddress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentRoute {
    pub id: Option<i64>,
    pub track_guid: String,
    pub feed_guid: String,
    pub recipient_name: Option<String>,
    pub route_type: RouteType,
    pub address: String,
    pub custom_key: Option<String>,
    pub custom_value: Option<String>,
    pub split: i64,
    /// When `true`, this recipient is an app-fee destination, not an artist split.
    pub fee: bool,
}

/// Feed-level payment route (fallback when a track has no per-track routes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPaymentRoute {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub recipient_name: Option<String>,
    pub route_type: RouteType,
    pub address: String,
    pub custom_key: Option<String>,
    pub custom_value: Option<String>,
    pub split: i64,
    pub fee: bool,
}

// ── route-history recipient set (ADR 0053 Section 4) ────────────────────────

/// One recipient of a payment-route set: an address, its keysend custom
/// record, and its split.
///
/// The route-history read of ADR 0053 Section 4 compares an ordered list of
/// these to decide whether a payment change occurred. A keysend route to a
/// shared node names the account in `custom_key` and `custom_value`, so a
/// change of either one sends the payment to a different recipient. The set
/// excludes the name, the route type and `fee`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RouteRecipient {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_value: Option<String>,
    pub split: i64,
}

impl From<&PaymentRoute> for RouteRecipient {
    fn from(route: &PaymentRoute) -> Self {
        Self {
            address: route.address.clone(),
            custom_key: route.custom_key.clone(),
            custom_value: route.custom_value.clone(),
            split: route.split,
        }
    }
}

impl From<&FeedPaymentRoute> for RouteRecipient {
    fn from(route: &FeedPaymentRoute) -> Self {
        Self {
            address: route.address.clone(),
            custom_key: route.custom_key.clone(),
            custom_value: route.custom_value.clone(),
            split: route.split,
        }
    }
}

/// Builds the ordered recipient set of a track's payment routes (ADR 0053
/// Section 4).
#[must_use]
pub fn recipient_set(routes: &[PaymentRoute]) -> Vec<RouteRecipient> {
    routes.iter().map(RouteRecipient::from).collect()
}

/// Builds the ordered recipient set of a feed's payment routes (ADR 0053
/// Section 4).
#[must_use]
pub fn feed_recipient_set(routes: &[FeedPaymentRoute]) -> Vec<RouteRecipient> {
    routes.iter().map(RouteRecipient::from).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueTimeSplit {
    pub id: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_feed_guid: String,
    /// GUID of the track whose playback triggers this split.
    pub source_track_guid: String,
    pub start_time_secs: i64,
    pub duration_secs: Option<i64>,
    pub remote_feed_guid: String,
    pub remote_item_guid: String,
    pub split: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedRemoteItemRaw {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub position: i64,
    pub medium: Option<String>,
    pub remote_feed_guid: String,
    pub remote_feed_url: Option<String>,
    /// Raw `rel` attribute of the `podcast:remoteItem` element.
    ///
    /// The Podcast Namespace does not define `rel` on `podcast:remoteItem`,
    /// so this value is non-standard. `#[serde(default)]` lets an event that
    /// an earlier node signed, before this field existed, still decode.
    #[serde(default)]
    pub rel: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRemoteItemRaw {
    pub id: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub feed_guid: String,
    pub track_guid: String,
    pub position: i64,
    pub medium: Option<String>,
    pub remote_feed_guid: String,
    pub remote_feed_url: Option<String>,
    /// Raw `rel` attribute of the `podcast:remoteItem` element.
    ///
    /// The Podcast Namespace does not define `rel` on `podcast:remoteItem`,
    /// so this value is non-standard. `#[serde(default)]` lets an event that
    /// an earlier node signed, before this field existed, still decode.
    #[serde(default)]
    pub rel: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveEvent {
    pub live_item_guid: String,
    pub feed_guid: String,
    pub title: String,
    pub content_link: Option<String>,
    pub status: String,
    pub scheduled_start: Option<i64>,
    pub scheduled_end: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceContributorClaim {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub name: String,
    /// Published role text from the source feed, preserved verbatim.
    pub role: Option<String>,
    /// Query-friendly normalized copy of `role` (trimmed, lowercase, collapsed whitespace).
    pub role_norm: Option<String>,
    pub group_name: Option<String>,
    pub href: Option<String>,
    pub img: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npub: Option<String>,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntityIdClaim {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub scheme: String,
    pub value: String,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntityLink {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub link_type: String,
    pub url: String,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReleaseClaim {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub claim_type: String,
    pub claim_value: String,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItemEnclosure {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub url: String,
    pub mime_type: Option<String>,
    pub bytes: Option<i64>,
    pub rel: Option<String>,
    pub title: Option<String>,
    pub is_primary: bool,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItemTranscript {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub entity_type: String,
    pub entity_id: String,
    pub position: i64,
    pub url: String,
    pub mime_type: Option<String>,
    pub language: Option<String>,
    pub rel: Option<String>,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePlatformClaim {
    pub id: Option<i64>,
    pub feed_guid: String,
    pub platform_key: String,
    pub url: Option<String>,
    pub owner_name: Option<String>,
    pub source: String,
    pub extraction_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalId {
    pub scheme: String,
    pub value: String,
}

// ── feed_blocks (ADR 0053) ─────────────────────────────────────────────────

/// The kind of value a [`FeedBlock`] names: a `podcast:guid` or an exact URL.
///
/// ADR 0053 Section 1. A GUID value is stored and compared in lower case. A
/// URL value is stored and compared as the exact trimmed string.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedBlockKind {
    /// The value is a `podcast:guid`.
    Guid,
    /// The value is an exact feed URL.
    Url,
}

impl FeedBlockKind {
    /// Returns the wire value of this kind: `"guid"` or `"url"`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Guid => "guid",
            Self::Url => "url",
        }
    }

    /// Normalizes `value` the way this kind is stored and matched.
    ///
    /// A GUID is lower-cased and trimmed. A URL is trimmed only.
    #[must_use]
    pub fn normalize(&self, value: &str) -> String {
        match self {
            Self::Guid => value.trim().to_lowercase(),
            Self::Url => value.trim().to_string(),
        }
    }
}

/// A row that blocks one feed GUID or one exact feed URL.
///
/// ADR 0053 Section 1. The primary makes `block_id` as a UUID and carries it
/// in the `FeedBlocked` event, so every node holds the same row under the
/// same identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct FeedBlock {
    pub block_id: String,
    pub kind: FeedBlockKind,
    pub value: String,
    pub reason: String,
    pub blocked_at: i64,
}

// ── feed_copies (ADR 0058) ───────────────────────────────────────────────────

/// This is the ADR 0058 Section 1 summary of one submission at a URL for a
/// feed GUID. It holds the channel title, the item GUIDs in the feed's own
/// order, and the recipient set of the feed and of each track.
///
/// [`summary_digest`] covers `item_guids`, `feed_recipients` and
/// `track_recipients` only, so a title change alone makes no event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CopySummary {
    pub title: String,
    pub item_guids: Vec<String>,
    pub feed_recipients: Vec<RouteRecipient>,
    pub track_recipients: BTreeMap<String, Vec<RouteRecipient>>,
}

impl From<&IngestPaymentRoute> for RouteRecipient {
    fn from(route: &IngestPaymentRoute) -> Self {
        Self {
            address: route.address.clone(),
            custom_key: route.custom_key.clone(),
            custom_value: route.custom_value.clone(),
            split: route.split,
        }
    }
}

/// Builds the ADR 0058 Section 1 summary of a mirror body. The summary
/// holds the channel title, the item GUIDs in the feed's own order, and the
/// recipient set of the feed and of each track.
#[must_use]
pub fn copy_summary(feed: &IngestFeedData) -> CopySummary {
    let feed_recipients = feed
        .feed_payment_routes
        .iter()
        .map(RouteRecipient::from)
        .collect();

    let mut item_guids = Vec::with_capacity(feed.tracks.len());
    let mut track_recipients = BTreeMap::new();
    for track in &feed.tracks {
        item_guids.push(track.track_guid.clone());
        let recipients: Vec<RouteRecipient> = track
            .payment_routes
            .iter()
            .map(RouteRecipient::from)
            .collect();
        track_recipients.insert(track.track_guid.clone(), recipients);
    }

    CopySummary {
        title: feed.title.clone(),
        item_guids,
        feed_recipients,
        track_recipients,
    }
}

/// The identity fields of a [`CopySummary`] (ADR 0058 Section 1). The title
/// is left out, so a title change alone does not change the digest.
#[derive(Serialize)]
struct CopySummaryDigestFields<'a> {
    item_guids: &'a [String],
    feed_recipients: &'a [RouteRecipient],
    track_recipients: &'a BTreeMap<String, Vec<RouteRecipient>>,
}

/// Returns the SHA-256 hex digest of the identity fields of `summary`: the
/// `item_guids`, `feed_recipients` and `track_recipients` (ADR 0058 Section
/// 1). The title is not part of the digest.
///
/// `item_guids` and `feed_recipients` keep the submission order, and
/// `BTreeMap` keeps a stable key order, so the same summary always makes the
/// same bytes.
///
/// # Panics
///
/// Panics if `CopySummaryDigestFields` cannot be serialized to JSON. This is
/// a programming error, since the type always serializes.
#[must_use]
pub fn summary_digest(summary: &CopySummary) -> String {
    let fields = CopySummaryDigestFields {
        item_guids: &summary.item_guids,
        feed_recipients: &summary.feed_recipients,
        track_recipients: &summary.track_recipients,
    };
    let bytes =
        serde_json::to_vec(&fields).expect("CopySummaryDigestFields always serializes to JSON");
    hex::encode(sha2::Sha256::digest(&bytes))
}

/// The ADR 0058 Section 1b namespace for deriving a `podcast:guid` from a
/// feed URL.
const GUID_ORIGIN_NAMESPACE: uuid::Uuid = uuid::uuid!("ead4c236-bf58-58c6-a2c6-a6b28d128cb6");

/// True when `feed_guid` is the `UUIDv5` that the ADR 0058 Section 1b
/// namespace derives from `url`. The check removes the scheme and one
/// trailing slash from `url` first.
///
/// This is evidence for the operator. It is not a rule. A correct move to a
/// new host keeps the old GUID, so the old URL can also match.
#[must_use]
pub fn guid_origin_matches(feed_guid: &str, url: &str) -> bool {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let name = without_scheme.strip_suffix('/').unwrap_or(without_scheme);
    let derived = uuid::Uuid::new_v5(&GUID_ORIGIN_NAMESPACE, name.as_bytes());
    derived.to_string().eq_ignore_ascii_case(feed_guid)
}
