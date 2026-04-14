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

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueTimeSplit {
    pub id: Option<i64>,
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
