#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full spawn_blocking scope"
)]
#![allow(
    clippy::too_many_lines,
    reason = "query handlers intentionally assemble rich paginated/detail responses in one place"
)]

//! Query API handlers for the `/v1/*` read-only endpoints.
//!
//! All handlers are read-only and run on both primary and community nodes.
//! Pagination uses opaque base64-encoded cursors. Nested data can be requested
//! via the `?include=` query parameter.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::model::Feed;
use crate::{api, db, medium};

// ── Pagination ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
struct Pagination {
    cursor: Option<String>,
    has_more: bool,
}

fn encode_cursor(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

fn decode_cursor(cursor: &str) -> Result<String, api::ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_err| api::ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "invalid cursor".into(),
            www_authenticate: None,
        })?;
    String::from_utf8(bytes).map_err(|_err| api::ApiError {
        status: StatusCode::BAD_REQUEST,
        message: "invalid cursor encoding".into(),
        www_authenticate: None,
    })
}

// ── Response envelope ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
struct QueryResponse<T> {
    data: T,
    pagination: Pagination,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize, ToSchema)]
struct ResponseMeta {
    api_version: &'static str,
    node_pubkey: String,
}

fn meta(state: &api::AppState) -> ResponseMeta {
    ResponseMeta {
        api_version: "v1",
        node_pubkey: state.node_pubkey_hex.clone(),
    }
}

// ── Query params ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    cursor: Option<String>,
    limit: Option<i64>,
    include: Option<String>,
    medium: Option<String>,
}

impl ListQuery {
    fn capped_limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }

    fn includes(&self, field: &str) -> bool {
        self.include
            .as_deref()
            .is_some_and(|s| s.split(',').any(|f| f.trim() == field))
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PublisherSearchQuery {
    q: Option<String>,
    limit: Option<i64>,
    case_sensitive: Option<bool>,
}

impl PublisherSearchQuery {
    fn case_sensitive(&self) -> bool {
        self.case_sensitive.unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
pub struct PublisherDetailQuery {
    limit: Option<i64>,
    case_sensitive: Option<bool>,
}

impl PublisherDetailQuery {
    fn capped_limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }

    fn case_sensitive(&self) -> bool {
        self.case_sensitive.unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
pub struct ArtistTracksQuery {
    artist: String,
    limit: Option<i64>,
    cursor: Option<String>,
}

impl ArtistTracksQuery {
    fn capped_limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

fn like_contains_pattern(value: &str) -> String {
    format!(
        "%{}%",
        value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

// ── Serializable types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
struct FeedResponse {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    release_artist: Option<String>,
    /// The source of `release_artist`: `itunes_author`, `itunes_owner`, or
    /// `placeholder`. Null when no ingest has run since migration 0037. ADR
    /// 0049 §5.
    release_artist_source: Option<String>,
    release_artist_sort: Option<String>,
    release_date: Option<i64>,
    last_build_date: Option<i64>,
    release_kind: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    publisher_text: Option<String>,
    /// The title of the feed this album names as its publisher.
    ///
    /// Derived when the node reads this feed: the feed's own
    /// `medium="publisher"` remote item at the lowest position, resolved
    /// with [`db::resolve_listed_feed`]. Null when the feed names no
    /// publisher or the resolver cannot place it. ADR 0049 §5.
    publisher_feed_title: Option<String>,
    /// The number of distinct album artists of a publisher feed.
    ///
    /// The value is derived, not stored. It counts only an album whose
    /// `release_artist` comes from `itunes:author`. A "feat." credit can
    /// count as a different artist. Present only when this feed is a
    /// publisher feed. ADR 0049 §7.
    #[serde(skip_serializing_if = "Option::is_none")]
    distinct_release_artist_count: Option<i64>,
    /// One raw `release_artist` value for each count in
    /// `distinct_release_artist_count`, sorted by the normalized value.
    ///
    /// The value is derived, not stored. It counts only an album whose
    /// `release_artist` comes from `itunes:author`. A "feat." credit can
    /// count as a different artist. Present only when this feed is a
    /// publisher feed. ADR 0049 §7.
    #[serde(skip_serializing_if = "Option::is_none")]
    distinct_release_artists: Option<Vec<String>>,
    language: Option<String>,
    explicit: bool,
    episode_count: Option<i64>,
    newest_item_at: Option<i64>,
    oldest_item_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracks: Option<Vec<TrackSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payment_routes: Option<Vec<RouteResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_links: Option<Vec<SourceEntityLinkResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_ids: Option<Vec<SourceEntityIdResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_contributors: Option<Vec<SourceContributorClaimResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_platforms: Option<Vec<SourcePlatformClaimResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_release_claims: Option<Vec<SourceReleaseClaimResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_items: Option<Vec<FeedRemoteItemResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<Vec<PublisherResponse>>,
}

#[derive(Debug, Serialize, ToSchema)]
struct TrackSummary {
    track_guid: String,
    title: String,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    image_url: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    track_number: Option<i64>,
    publisher_text: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct RouteResponse {
    recipient_name: Option<String>,
    route_type: String,
    address: String,
    custom_key: Option<String>,
    custom_value: Option<String>,
    split: i64,
    fee: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct TrackResponse {
    track_guid: String,
    feed_guid: String,
    title: String,
    publisher_text: Option<String>,
    track_artist: Option<String>,
    track_artist_sort: Option<String>,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    image_url: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    language: Option<String>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    enclosure_bytes: Option<i64>,
    track_number: Option<i64>,
    explicit: bool,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
    feed_title: String,
    release_artist: Option<String>,
    /// The source of `release_artist`: `itunes_author`, `itunes_owner`, or
    /// `placeholder`. Null when no ingest has run since migration 0037. ADR
    /// 0049 §5.
    release_artist_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payment_routes: Option<Vec<RouteResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_time_splits: Option<Vec<VtsResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_links: Option<Vec<SourceEntityLinkResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_ids: Option<Vec<SourceEntityIdResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_contributors: Option<Vec<SourceContributorClaimResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_release_claims: Option<Vec<SourceReleaseClaimResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_enclosures: Option<Vec<SourceItemEnclosureResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_transcripts: Option<Vec<SourceItemTranscriptResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_items: Option<Vec<TrackRemoteItemResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<Vec<PublisherResponse>>,
}

#[derive(Debug, Serialize, ToSchema)]
struct PublisherSearchItem {
    publisher_text: String,
    feed_count: i64,
    track_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct PublisherFeedSummary {
    feed_guid: String,
    feed_url: String,
    title: String,
    image_url: Option<String>,
    episode_count: Option<i64>,
    raw_medium: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct PublisherTrackSummary {
    track_guid: String,
    feed_guid: String,
    title: String,
    image_url: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    duration_secs: Option<i64>,
    track_number: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
struct PublisherDetailResponse {
    publisher_text: String,
    feeds: Vec<PublisherFeedSummary>,
    tracks: Vec<PublisherTrackSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
struct SearchResponseItem {
    entity_type: String,
    entity_id: String,
    rank: f64,
    quality_score: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    feed_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    // ADR 0042: a result row is drawable from the search response alone. The
    // title key is always present. It is null only when the indexed row is
    // gone. A summary field that is absent is not evidence that the value is
    // absent in the full record.
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feed_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    track_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feed_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub_date: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
struct ArtistTrackItem {
    track_guid: String,
    feed_guid: String,
    title: String,
    track_artist: Option<String>,
    track_artist_sort: Option<String>,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    image_url: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    track_number: Option<i64>,
    feed_title: String,
    release_artist: Option<String>,
    /// The source of `release_artist`: `itunes_author`, `itunes_owner`, or
    /// `placeholder`. Null when no ingest has run since migration 0037. ADR
    /// 0049 §5.
    release_artist_source: Option<String>,
    created_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct VtsResponse {
    start_time_secs: i64,
    duration_secs: Option<i64>,
    remote_feed_guid: String,
    remote_item_guid: String,
    split: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct PeerResponse {
    node_pubkey: String,
    node_url: String,
    last_push_at: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceContributorClaimResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    name: String,
    role: Option<String>,
    role_norm: Option<String>,
    group_name: Option<String>,
    href: Option<String>,
    img: Option<String>,
    npub: Option<String>,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceEntityIdResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    scheme: String,
    value: String,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceEntityLinkResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    link_type: String,
    url: String,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceReleaseClaimResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    claim_type: String,
    claim_value: String,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourcePlatformClaimResponse {
    platform_key: String,
    url: Option<String>,
    owner_name: Option<String>,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceItemTranscriptResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    url: String,
    mime_type: Option<String>,
    language: Option<String>,
    rel: Option<String>,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct SourceItemEnclosureResponse {
    entity_type: String,
    entity_id: String,
    position: i64,
    url: String,
    mime_type: Option<String>,
    bytes: Option<i64>,
    rel: Option<String>,
    title: Option<String>,
    is_primary: bool,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
struct FeedRemoteItemResponse {
    position: i64,
    medium: Option<String>,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    /// Raw `rel` attribute. The Podcast Namespace does not define `rel` on
    /// `podcast:remoteItem`, so this value is non-standard.
    rel: Option<String>,
    source: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct TrackRemoteItemResponse {
    position: i64,
    medium: Option<String>,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    /// Raw `rel` attribute. The Podcast Namespace does not define `rel` on
    /// `podcast:remoteItem`, so this value is non-standard.
    rel: Option<String>,
    source: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a named RSS fact in the wire contract (ADR 0049 §4), not a state machine"
)]
struct PublisherResponse {
    direction: String,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    remote_feed_medium: Option<String>,
    publisher_feed_guid: String,
    publisher_feed_url: Option<String>,
    music_feed_guid: String,
    music_feed_url: Option<String>,
    /// The album names this publisher feed. ADR 0049 §4.
    music_names_publisher: bool,
    /// The publisher feed lists this album, by any resolution. ADR 0049 §4.
    publisher_lists_music: bool,
    /// How the node resolved the feed that carries `publisher_lists_music`:
    /// `"guid"`, `"feed_url"` or `"unresolved"`. ADR 0049 §3.
    publisher_link_resolution: String,
    /// The time of the URL observation behind `publisher_link_resolution`.
    /// Null unless that value is `"feed_url"`. ADR 0049 §3.
    publisher_link_observed_at: Option<i64>,
    reciprocal_declared: bool,
    reciprocal_medium: Option<String>,
    two_way_validated: bool,
    /// Raw `rel` of the `medium="music"` item, on the publisher feed, that
    /// lists this album. Null when that item is missing or has no `rel`.
    /// A comma separates two or more roles in this raw value.
    ///
    /// The Podcast Namespace does not define `rel` on `podcast:remoteItem`,
    /// so this value is non-standard. ADR 0049 §6.
    publisher_rel: Option<String>,
    /// Raw `rel` of the `medium="publisher"` item, on the album feed, that
    /// names this publisher. Null when that item is missing or has no
    /// `rel`. A comma separates two or more roles in this raw value.
    ///
    /// The Podcast Namespace does not define `rel` on `podcast:remoteItem`,
    /// so this value is non-standard. ADR 0049 §6.
    music_rel: Option<String>,
    /// The role set, sorted and joined by `", "`, or `"artist"` when
    /// neither side states one. Null on a conflict between the two role
    /// sets. ADR 0049 §6, plan decision 13.
    ///
    /// `"artist"` with `role_source = "default"` is an assumption. It is
    /// not a statement that the feed makes.
    role: Option<String>,
    /// The source of `role`: `"publisher_rel"`, `"music_rel"`, `"default"`
    /// or `"conflict"`. ADR 0049 §6.
    role_source: String,
}

/// Intermediate row type for track queries to avoid complex tuple types.
struct TrackRow {
    track_guid: String,
    feed_guid: String,
    title: String,
    publisher_text: Option<String>,
    track_artist: Option<String>,
    track_artist_sort: Option<String>,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    image_url: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    language: Option<String>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    enclosure_bytes: Option<i64>,
    track_number: Option<i64>,
    explicit_int: i64,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
    feed_title: String,
    release_artist: Option<String>,
    release_artist_source: Option<String>,
}

fn parse_track_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        track_guid: row.get(0)?,
        feed_guid: row.get(1)?,
        title: row.get(2)?,
        publisher_text: row.get(3)?,
        track_artist: row.get(4)?,
        track_artist_sort: row.get(5)?,
        pub_date: row.get(6)?,
        duration_secs: row.get(7)?,
        image_url: row.get(8)?,
        language: row.get(9)?,
        enclosure_url: row.get(10)?,
        enclosure_type: row.get(11)?,
        enclosure_bytes: row.get(12)?,
        track_number: row.get(13)?,
        explicit_int: row.get(15)?,
        description: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        feed_title: row.get(19)?,
        release_artist: row.get(20)?,
        track_image_url: row.get(21)?,
        feed_image_url: row.get(22)?,
        release_artist_source: row.get(23)?,
    })
}

/// Summary values a search result needs to draw a row. ADR 0042.
#[derive(Default)]
struct SearchSummary {
    title: Option<String>,
    feed_title: Option<String>,
    track_image_url: Option<String>,
    feed_image_url: Option<String>,
    pub_date: Option<i64>,
}

/// Reads the summary values for a feed hit.
///
/// A feed row holds no track publication date, so `pub_date` stays empty.
fn feed_search_summary(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<SearchSummary, api::ApiError> {
    conn.query_row(
        "SELECT title, image_url FROM feeds WHERE feed_guid = ?1",
        params![feed_guid],
        |row| {
            Ok(SearchSummary {
                title: row.get(0)?,
                feed_image_url: row.get(1)?,
                ..SearchSummary::default()
            })
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
    .map_err(api::ApiError::from)
}

fn get_track_rows_by_guid(
    conn: &rusqlite::Connection,
    track_guid: &str,
) -> Result<Vec<TrackRow>, api::ApiError> {
    let mut stmt = conn.prepare(
        "SELECT t.track_guid, t.feed_guid, t.title, t.publisher, t.track_artist, t.track_artist_sort, \
         t.pub_date, t.duration_secs, COALESCE(t.image_url, f.image_url), t.language, \
         t.enclosure_url, t.enclosure_type, t.enclosure_bytes, t.track_number, t.season, \
         t.explicit, t.description, t.created_at, t.updated_at, COALESCE(f.title, ''), \
         f.release_artist, t.image_url, f.image_url, f.release_artist_source \
         FROM tracks t LEFT JOIN feeds f ON f.feed_guid = t.feed_guid \
         WHERE t.track_guid = ?1 ORDER BY t.feed_guid ASC",
    )?;

    let rows = stmt.query_map(params![track_guid], parse_track_row)?;
    rows.collect::<Result<_, _>>().map_err(api::ApiError::from)
}

fn get_track_row_for_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Option<TrackRow>, api::ApiError> {
    conn.query_row(
        "SELECT t.track_guid, t.feed_guid, t.title, t.publisher, t.track_artist, t.track_artist_sort, \
         t.pub_date, t.duration_secs, COALESCE(t.image_url, f.image_url), t.language, \
         t.enclosure_url, t.enclosure_type, t.enclosure_bytes, t.track_number, t.season, \
         t.explicit, t.description, t.created_at, t.updated_at, COALESCE(f.title, ''), \
         f.release_artist, t.image_url, f.image_url, f.release_artist_source \
         FROM tracks t LEFT JOIN feeds f ON f.feed_guid = t.feed_guid \
         WHERE t.feed_guid = ?1 AND t.track_guid = ?2",
        params![feed_guid, track_guid],
        parse_track_row,
    )
    .optional()
    .map_err(api::ApiError::from)
}

/// Intermediate row type for feed queries to avoid complex tuple types.
struct FeedRow {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    release_artist: Option<String>,
    release_artist_source: Option<String>,
    release_artist_sort: Option<String>,
    release_date: Option<i64>,
    last_build_date: Option<i64>,
    release_kind: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    publisher_text: Option<String>,
    language: Option<String>,
    explicit_int: i64,
    episode_count: Option<i64>,
    newest_item_at: Option<i64>,
    oldest_item_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

// ── GET /v1/feeds/{guid} ────────────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single paginated-detail flow with optional includes"
)]
async fn handle_get_feed(
    State(state): State<Arc<api::AppState>>,
    Path(feed_guid): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        // Mutex safety compliant — 2026-03-12
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let row = conn
            .query_row(
                "SELECT feed_guid, feed_url, title, raw_medium, release_artist, \
             release_artist_sort, release_date, release_kind, description, image_url, publisher, \
             language, explicit, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, \
             last_build_date, release_artist_source \
             FROM feeds WHERE feed_guid = ?1",
                params![feed_guid],
                |row| {
                    Ok(FeedRow {
                        feed_guid: row.get(0)?,
                        feed_url: row.get(1)?,
                        title: row.get(2)?,
                        raw_medium: row.get(3)?,
                        release_artist: row.get(4)?,
                        release_artist_sort: row.get(5)?,
                        release_date: row.get(6)?,
                        release_kind: row.get(7)?,
                        description: row.get(8)?,
                        image_url: row.get(9)?,
                        publisher_text: row.get(10)?,
                        language: row.get(11)?,
                        explicit_int: row.get(12)?,
                        episode_count: row.get(13)?,
                        newest_item_at: row.get(14)?,
                        oldest_item_at: row.get(15)?,
                        created_at: row.get(16)?,
                        updated_at: row.get(17)?,
                        last_build_date: row.get(18)?,
                        release_artist_source: row.get(19)?,
                    })
                },
            )
            .map_err(|_err| api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: "feed not found".into(),
                www_authenticate: None,
            })?;

        let resp = build_feed_response(&conn, row, &params)?;

        Ok::<_, api::ApiError>(QueryResponse {
            data: resp,
            pagination: Pagination {
                cursor: None,
                has_more: false,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    Ok(Json(result))
}

/// Gives the title of the feed that `feed_guid` names as its publisher.
///
/// Takes the feed's own `medium="publisher"` remote item at the lowest
/// position (the items are stored in position order), resolves it with
/// [`db::resolve_listed_feed`], and reads the title of the resolved feed.
/// Gives `None` when the feed names no publisher or the resolver cannot
/// place it. ADR 0049 §5.
fn resolve_publisher_feed_title(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<Option<String>, api::ApiError> {
    let Some(item) = db::get_feed_remote_items_for_feed(conn, feed_guid)?
        .into_iter()
        .find(|item| item.medium.as_deref() == Some("publisher"))
    else {
        return Ok(None);
    };

    let resolution = db::resolve_listed_feed(
        conn,
        &item.remote_feed_guid,
        item.remote_feed_url.as_deref(),
    )?;
    let Some(resolved_guid) = resolution.feed_guid() else {
        return Ok(None);
    };

    Ok(db::get_feed(conn, resolved_guid)?.map(|feed| feed.title))
}

/// Normalizes a `release_artist` value for the publisher artist count of
/// ADR 0049 §7.
///
/// The steps are: remove the white space at the start and at the end,
/// replace each internal run of white space with one space, then apply
/// [`str::to_lowercase`]. This function does not split a credit such as
/// "feat.": two different credits normalize to two different values.
fn normalize_release_artist(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase()
}

/// Builds the derived artist count of a publisher feed, ADR 0049 §7.
///
/// Reads the albums of `feed_guid` with
/// [`db::get_publisher_album_release_artists`], keeps the album with the
/// lowest `feed_guid` for each distinct normalized `release_artist`, and
/// gives the count and the raw values, sorted by the normalized value.
fn publisher_artist_count(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<(i64, Vec<String>), api::ApiError> {
    let mut albums = db::get_publisher_album_release_artists(conn, feed_guid)?;
    albums.sort_by(|a, b| a.feed_guid.cmp(&b.feed_guid));

    // A `BTreeMap` keyed by the normalized value both dedupes and sorts by
    // that value. `albums` is sorted by `feed_guid` ascending, so the first
    // album `entry` sees for a normalized value is the one with the lowest
    // `feed_guid`, and `or_insert` keeps only that one.
    let mut by_normalized: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for album in albums {
        by_normalized
            .entry(normalize_release_artist(&album.release_artist))
            .or_insert(album.release_artist);
    }

    let count = i64::try_from(by_normalized.len()).unwrap_or(i64::MAX);
    let artists: Vec<String> = by_normalized.into_values().collect();
    Ok((count, artists))
}

fn build_feed_response(
    conn: &rusqlite::Connection,
    row: FeedRow,
    params: &ListQuery,
) -> Result<FeedResponse, api::ApiError> {
    let feed_guid = row.feed_guid.clone();
    let publisher_feed_title = resolve_publisher_feed_title(conn, &feed_guid)?;

    let mut resp = FeedResponse {
        feed_guid: row.feed_guid,
        feed_url: row.feed_url,
        title: row.title,
        raw_medium: row.raw_medium,
        release_artist: row.release_artist,
        release_artist_source: row.release_artist_source,
        release_artist_sort: row.release_artist_sort,
        release_date: row.release_date,
        last_build_date: row.last_build_date,
        release_kind: row.release_kind,
        description: row.description,
        image_url: row.image_url,
        publisher_text: row.publisher_text,
        publisher_feed_title,
        distinct_release_artist_count: None,
        distinct_release_artists: None,
        language: row.language,
        explicit: row.explicit_int != 0,
        episode_count: row.episode_count,
        newest_item_at: row.newest_item_at,
        oldest_item_at: row.oldest_item_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        tracks: None,
        payment_routes: None,
        source_links: None,
        source_ids: None,
        source_contributors: None,
        source_platforms: None,
        source_release_claims: None,
        remote_items: None,
        publisher: None,
    };

    // ADR 0049 §7: only a publisher feed carries the derived artist count.
    if medium::is_publisher(resp.raw_medium.as_deref()) {
        let (count, artists) = publisher_artist_count(conn, &feed_guid)?;
        resp.distinct_release_artist_count = Some(count);
        resp.distinct_release_artists = Some(artists);
    }

    if params.includes("tracks") {
        // ADR 0042: this route read the track column alone, so `image_url` meant
        // something different here than on every other track route. The feed
        // artwork is already in hand, so no join is needed to resolve it.
        let feed_artwork = resp.image_url.clone();
        let mut stmt = conn.prepare(
            "SELECT track_guid, title, pub_date, duration_secs, image_url, track_number, publisher \
             FROM tracks WHERE feed_guid = ?1 ORDER BY track_number ASC, pub_date DESC",
        )?;
        let tracks: Vec<TrackSummary> = stmt
            .query_map(params![feed_guid], |row| {
                let track_image_url: Option<String> = row.get(4)?;
                Ok(TrackSummary {
                    track_guid: row.get(0)?,
                    title: row.get(1)?,
                    pub_date: row.get(2)?,
                    duration_secs: row.get(3)?,
                    image_url: track_image_url.clone().or_else(|| feed_artwork.clone()),
                    track_image_url,
                    feed_image_url: feed_artwork.clone(),
                    track_number: row.get(5)?,
                    publisher_text: row.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        resp.tracks = Some(tracks);
    }

    if params.includes("payment_routes") {
        let mut stmt = conn.prepare(
            "SELECT recipient_name, route_type, address, NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
             FROM feed_payment_routes WHERE feed_guid = ?1",
        )?;
        let routes: Vec<RouteResponse> = stmt
            .query_map(params![feed_guid], |row| {
                Ok(RouteResponse {
                    recipient_name: row.get(0)?,
                    route_type: row.get(1)?,
                    address: row.get(2)?,
                    custom_key: row.get(3)?,
                    custom_value: row.get(4)?,
                    split: row.get(5)?,
                    fee: row.get::<_, i64>(6)? != 0,
                })
            })?
            .collect::<Result<_, _>>()?;
        resp.payment_routes = Some(routes);
    }

    if params.includes("source_links") {
        resp.source_links = Some(
            db::get_source_entity_links_for_entity(conn, "feed", &feed_guid)?
                .into_iter()
                .map(entity_link_response)
                .collect(),
        );
    }

    if params.includes("source_ids") {
        resp.source_ids = Some(
            db::get_source_entity_ids_for_entity(conn, "feed", &feed_guid)?
                .into_iter()
                .map(entity_id_response)
                .collect(),
        );
    }

    if params.includes("source_contributors") {
        resp.source_contributors = Some(
            db::get_source_contributor_claims_for_entity(conn, "feed", &feed_guid)?
                .into_iter()
                .map(contributor_claim_response)
                .collect(),
        );
    }

    if params.includes("source_platforms") {
        resp.source_platforms = Some(
            db::get_source_platform_claims_for_feed(conn, &feed_guid)?
                .into_iter()
                .map(platform_claim_response)
                .collect(),
        );
    }

    if params.includes("source_release_claims") {
        resp.source_release_claims = Some(
            db::get_source_release_claims_for_entity(conn, "feed", &feed_guid)?
                .into_iter()
                .map(release_claim_response)
                .collect(),
        );
    }

    if params.includes("remote_items") {
        resp.remote_items = Some(
            db::get_feed_remote_items_for_feed(conn, &feed_guid)?
                .into_iter()
                .map(feed_remote_item_response)
                .collect(),
        );
    }

    if params.includes("publisher") {
        resp.publisher = Some(load_publisher(conn, &feed_guid)?);
    }

    Ok(resp)
}

fn build_track_response(
    conn: &rusqlite::Connection,
    row: TrackRow,
    params: &ListQuery,
) -> Result<TrackResponse, api::ApiError> {
    let track_guid = row.track_guid.clone();
    let feed_guid = row.feed_guid.clone();

    let mut resp = TrackResponse {
        track_guid: row.track_guid,
        feed_guid: row.feed_guid,
        title: row.title,
        publisher_text: row.publisher_text,
        track_artist: row.track_artist,
        track_artist_sort: row.track_artist_sort,
        pub_date: row.pub_date,
        duration_secs: row.duration_secs,
        image_url: row.image_url,
        track_image_url: row.track_image_url,
        feed_image_url: row.feed_image_url,
        language: row.language,
        enclosure_url: row.enclosure_url,
        enclosure_type: row.enclosure_type,
        enclosure_bytes: row.enclosure_bytes,
        track_number: row.track_number,
        explicit: row.explicit_int != 0,
        description: row.description,
        created_at: row.created_at,
        updated_at: row.updated_at,
        feed_title: row.feed_title,
        release_artist: row.release_artist,
        release_artist_source: row.release_artist_source,
        payment_routes: None,
        value_time_splits: None,
        source_links: None,
        source_ids: None,
        source_contributors: None,
        source_release_claims: None,
        source_enclosures: None,
        source_transcripts: None,
        remote_items: None,
        publisher: None,
    };

    if params.includes("payment_routes") {
        let routes: Vec<RouteResponse> =
            db::get_payment_routes_for_feed_track(conn, &feed_guid, &track_guid)?
                .into_iter()
                .map(|route| RouteResponse {
                    recipient_name: route.recipient_name,
                    route_type: serde_json::to_string(&route.route_type)
                        .expect("serializing RouteType cannot fail")
                        .trim_matches('"')
                        .to_string(),
                    address: route.address,
                    custom_key: route.custom_key,
                    custom_value: route.custom_value,
                    split: route.split,
                    fee: route.fee,
                })
                .collect();
        // Feed→track inheritance: fall back to parent feed routes when the
        // track has none of its own.
        let routes = if routes.is_empty() {
            let mut fstmt = conn.prepare(
                "SELECT recipient_name, route_type, address, NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
                 FROM feed_payment_routes WHERE feed_guid = ?1",
            )?;
            fstmt
                .query_map(params![resp.feed_guid], |row| {
                    Ok(RouteResponse {
                        recipient_name: row.get(0)?,
                        route_type: row.get(1)?,
                        address: row.get(2)?,
                        custom_key: row.get(3)?,
                        custom_value: row.get(4)?,
                        split: row.get(5)?,
                        fee: row.get::<_, i64>(6)? != 0,
                    })
                })?
                .collect::<Result<_, _>>()?
        } else {
            routes
        };
        resp.payment_routes = Some(routes);
    }

    if params.includes("value_time_splits") {
        let vts: Vec<VtsResponse> =
            db::get_value_time_splits_for_feed_track(conn, &feed_guid, &track_guid)?
                .into_iter()
                .map(|split| VtsResponse {
                    start_time_secs: split.start_time_secs,
                    duration_secs: split.duration_secs,
                    remote_feed_guid: split.remote_feed_guid,
                    remote_item_guid: split.remote_item_guid,
                    split: split.split,
                })
                .collect();
        resp.value_time_splits = Some(vts);
    }

    if params.includes("source_links") {
        resp.source_links = Some(
            db::get_source_entity_links_for_feed_entity(conn, &feed_guid, "track", &track_guid)?
                .into_iter()
                .map(entity_link_response)
                .collect(),
        );
    }

    if params.includes("source_ids") {
        resp.source_ids = Some(
            db::get_source_entity_ids_for_feed_entity(conn, &feed_guid, "track", &track_guid)?
                .into_iter()
                .map(entity_id_response)
                .collect(),
        );
    }

    if params.includes("source_contributors") {
        let claims = db::get_effective_source_contributor_claims_for_track(
            conn,
            &resp.feed_guid,
            &track_guid,
        )?;
        resp.source_contributors =
            Some(claims.into_iter().map(contributor_claim_response).collect());
    }

    if params.includes("source_release_claims") {
        resp.source_release_claims = Some(
            db::get_source_release_claims_for_feed_entity(conn, &feed_guid, "track", &track_guid)?
                .into_iter()
                .map(release_claim_response)
                .collect(),
        );
    }

    if params.includes("source_enclosures") {
        resp.source_enclosures = Some(
            db::get_source_item_enclosures_for_feed_entity(conn, &feed_guid, "track", &track_guid)?
                .into_iter()
                .map(enclosure_response)
                .collect(),
        );
    }

    if params.includes("source_transcripts") {
        resp.source_transcripts = Some(
            db::get_source_item_transcripts_for_feed_entity(
                conn,
                &feed_guid,
                "track",
                &track_guid,
            )?
            .into_iter()
            .map(transcript_response)
            .collect(),
        );
    }

    if params.includes("remote_items") {
        resp.remote_items = Some(load_track_remote_items(conn, &feed_guid, &track_guid)?);
    }

    if params.includes("publisher") {
        resp.publisher = Some(load_track_publisher(conn, &feed_guid, &track_guid)?);
    }

    Ok(resp)
}

fn contributor_claim_response(
    claim: crate::model::SourceContributorClaim,
) -> SourceContributorClaimResponse {
    SourceContributorClaimResponse {
        entity_type: claim.entity_type,
        entity_id: claim.entity_id,
        position: claim.position,
        name: claim.name,
        role: claim.role,
        role_norm: claim.role_norm,
        group_name: claim.group_name,
        href: claim.href,
        img: claim.img,
        npub: claim.npub,
        source: claim.source,
        extraction_path: claim.extraction_path,
        observed_at: claim.observed_at,
    }
}

fn entity_id_response(claim: crate::model::SourceEntityIdClaim) -> SourceEntityIdResponse {
    SourceEntityIdResponse {
        entity_type: claim.entity_type,
        entity_id: claim.entity_id,
        position: claim.position,
        scheme: claim.scheme,
        value: claim.value,
        source: claim.source,
        extraction_path: claim.extraction_path,
        observed_at: claim.observed_at,
    }
}

fn entity_link_response(link: crate::model::SourceEntityLink) -> SourceEntityLinkResponse {
    SourceEntityLinkResponse {
        entity_type: link.entity_type,
        entity_id: link.entity_id,
        position: link.position,
        link_type: link.link_type,
        url: link.url,
        source: link.source,
        extraction_path: link.extraction_path,
        observed_at: link.observed_at,
    }
}

fn release_claim_response(claim: crate::model::SourceReleaseClaim) -> SourceReleaseClaimResponse {
    SourceReleaseClaimResponse {
        entity_type: claim.entity_type,
        entity_id: claim.entity_id,
        position: claim.position,
        claim_type: claim.claim_type,
        claim_value: claim.claim_value,
        source: claim.source,
        extraction_path: claim.extraction_path,
        observed_at: claim.observed_at,
    }
}

fn platform_claim_response(
    claim: crate::model::SourcePlatformClaim,
) -> SourcePlatformClaimResponse {
    SourcePlatformClaimResponse {
        platform_key: claim.platform_key,
        url: claim.url,
        owner_name: claim.owner_name,
        source: claim.source,
        extraction_path: claim.extraction_path,
        observed_at: claim.observed_at,
    }
}

fn enclosure_response(enclosure: crate::model::SourceItemEnclosure) -> SourceItemEnclosureResponse {
    SourceItemEnclosureResponse {
        entity_type: enclosure.entity_type,
        entity_id: enclosure.entity_id,
        position: enclosure.position,
        url: enclosure.url,
        mime_type: enclosure.mime_type,
        bytes: enclosure.bytes,
        rel: enclosure.rel,
        title: enclosure.title,
        is_primary: enclosure.is_primary,
        source: enclosure.source,
        extraction_path: enclosure.extraction_path,
        observed_at: enclosure.observed_at,
    }
}

fn transcript_response(t: crate::model::SourceItemTranscript) -> SourceItemTranscriptResponse {
    SourceItemTranscriptResponse {
        entity_type: t.entity_type,
        entity_id: t.entity_id,
        position: t.position,
        url: t.url,
        mime_type: t.mime_type,
        language: t.language,
        rel: t.rel,
        source: t.source,
        extraction_path: t.extraction_path,
        observed_at: t.observed_at,
    }
}

fn feed_remote_item_response(item: crate::model::FeedRemoteItemRaw) -> FeedRemoteItemResponse {
    FeedRemoteItemResponse {
        position: item.position,
        medium: item.medium,
        remote_feed_guid: item.remote_feed_guid,
        remote_feed_url: item.remote_feed_url,
        rel: item.rel,
        source: item.source,
    }
}

/// The resolver-derived facts shared by both directions of a `publisher`
/// view row. ADR 0049 §3 and §4.
struct PublisherLinkFacts {
    music_names_publisher: bool,
    publisher_lists_music: bool,
    publisher_link_resolution: &'static str,
    publisher_link_observed_at: Option<i64>,
    reciprocal_medium: Option<String>,
    publisher_feed_guid: String,
    publisher_feed_url: Option<String>,
    music_feed_guid: String,
    music_feed_url: Option<String>,
    /// The raw `rel` of the matched item on the other side, when the loop
    /// found one. `None` when no candidate matched. ADR 0049 §6.
    matched_item_rel: Option<String>,
}

/// Returns the resolved feed's GUID and stored URL, or the declared GUID and
/// URL when `resolution` did not resolve. Plan decision 11.
fn resolved_or_declared(
    conn: &rusqlite::Connection,
    resolution: &db::ListedFeedResolution,
    declared_guid: &str,
    declared_url: Option<&str>,
) -> Result<(String, Option<String>), api::ApiError> {
    let Some(feed_guid) = resolution.feed_guid() else {
        return Ok((declared_guid.to_string(), declared_url.map(str::to_string)));
    };
    let feed_url = db::get_feed(conn, feed_guid)?.map(|feed| feed.feed_url);
    Ok((
        feed_guid.to_string(),
        feed_url.or_else(|| declared_url.map(str::to_string)),
    ))
}

/// Builds the facts for a `music_to_publisher` row: `current_feed` names
/// `listed_guid` (declared) as its publisher.
///
/// `music_names_publisher` is true by declaration. `publisher_lists_music`
/// holds when the resolved publisher's own `medium="music"` items include one
/// that resolves back to `current_feed`. ADR 0049 §3, task 005 "Constraints".
fn music_to_publisher_facts(
    conn: &rusqlite::Connection,
    current_feed: &Feed,
    listed_guid: &str,
    listed_url: Option<&str>,
) -> Result<PublisherLinkFacts, api::ApiError> {
    let publisher_resolution = db::resolve_listed_feed(conn, listed_guid, listed_url)?;
    let (publisher_feed_guid, publisher_feed_url) =
        resolved_or_declared(conn, &publisher_resolution, listed_guid, listed_url)?;

    let mut publisher_lists_music = false;
    let mut publisher_link_resolution = "unresolved";
    let mut publisher_link_observed_at = None;
    let mut reciprocal_medium = None;
    let mut matched_item_rel = None;

    if let Some(publisher_guid) = publisher_resolution.feed_guid() {
        for candidate in db::get_feed_remote_items_for_feed(conn, publisher_guid)?
            .into_iter()
            .filter(|item| item.medium.as_deref() == Some("music"))
        {
            let candidate_resolution = db::resolve_listed_feed(
                conn,
                &candidate.remote_feed_guid,
                candidate.remote_feed_url.as_deref(),
            )?;
            if candidate_resolution.feed_guid() == Some(current_feed.feed_guid.as_str()) {
                publisher_lists_music = true;
                publisher_link_resolution = candidate_resolution.kind();
                publisher_link_observed_at = candidate_resolution.observed_at();
                reciprocal_medium = candidate.medium;
                matched_item_rel = candidate.rel;
                break;
            }
        }
    }

    Ok(PublisherLinkFacts {
        music_names_publisher: true,
        publisher_lists_music,
        publisher_link_resolution,
        publisher_link_observed_at,
        reciprocal_medium,
        publisher_feed_guid,
        publisher_feed_url,
        music_feed_guid: current_feed.feed_guid.clone(),
        music_feed_url: Some(current_feed.feed_url.clone()),
        matched_item_rel,
    })
}

/// Builds the facts for a `publisher_to_music` row: `current_feed` (a
/// publisher feed) lists `listed_guid` (declared) with `medium="music"`.
///
/// `publisher_lists_music` is true by declaration, and its resolution is the
/// resolution of the listed album. `music_names_publisher` holds when the
/// resolved album's own `medium="publisher"` items include one that resolves
/// back to `current_feed`. ADR 0049 §3, task 005 "Constraints".
fn publisher_to_music_facts(
    conn: &rusqlite::Connection,
    current_feed: &Feed,
    listed_guid: &str,
    listed_url: Option<&str>,
) -> Result<PublisherLinkFacts, api::ApiError> {
    let music_resolution = db::resolve_listed_feed(conn, listed_guid, listed_url)?;
    let (music_feed_guid, music_feed_url) =
        resolved_or_declared(conn, &music_resolution, listed_guid, listed_url)?;

    let mut music_names_publisher = false;
    let mut reciprocal_medium = None;
    let mut matched_item_rel = None;

    if let Some(album_guid) = music_resolution.feed_guid() {
        for candidate in db::get_feed_remote_items_for_feed(conn, album_guid)?
            .into_iter()
            .filter(|item| item.medium.as_deref() == Some("publisher"))
        {
            let candidate_resolution = db::resolve_listed_feed(
                conn,
                &candidate.remote_feed_guid,
                candidate.remote_feed_url.as_deref(),
            )?;
            if candidate_resolution.feed_guid() == Some(current_feed.feed_guid.as_str()) {
                music_names_publisher = true;
                reciprocal_medium = candidate.medium;
                matched_item_rel = candidate.rel;
                break;
            }
        }
    }

    Ok(PublisherLinkFacts {
        music_names_publisher,
        publisher_lists_music: true,
        publisher_link_resolution: music_resolution.kind(),
        publisher_link_observed_at: music_resolution.observed_at(),
        reciprocal_medium,
        publisher_feed_guid: current_feed.feed_guid.clone(),
        publisher_feed_url: Some(current_feed.feed_url.clone()),
        music_feed_guid,
        music_feed_url,
        matched_item_rel,
    })
}

/// Reads a raw `rel` value as a set of roles, and gives that set back as
/// one canonical string. ADR 0049 §6, plan decision 13.
///
/// A comma separates the roles. Each role loses the white space at its
/// start and its end, each internal run of white space becomes one space,
/// and the role is lowercased in the ASCII range. An empty role and a
/// duplicate role are removed. A value with no comma is one role, even
/// when it holds a space, such as `"sound engineer"`.
///
/// The result is the role set, sorted by byte order and joined by `", "`.
/// No role can hold a comma, so this joined string is a faithful encoding
/// of the set: two raw values normalize to the same string exactly when
/// their role sets hold the same roles. An empty set counts as no value.
fn normalize_rel(value: &str) -> Option<String> {
    let roles: std::collections::BTreeSet<String> = value
        .split(',')
        .map(|part| {
            part.split_whitespace()
                .collect::<Vec<&str>>()
                .join(" ")
                .to_ascii_lowercase()
        })
        .filter(|role| !role.is_empty())
        .collect();

    if roles.is_empty() {
        None
    } else {
        Some(roles.into_iter().collect::<Vec<String>>().join(", "))
    }
}

/// Derives `role` and `role_source` from the raw `rel` of the two items of a
/// `publisher` view row. ADR 0049 §6, plan decisions 8 and 13.
///
/// Each side normalizes with [`normalize_rel`] into its role set. When
/// both sides state a value and the role sets are equal, the result names
/// `publisher_rel` as the source. When the role sets differ, the result is
/// a conflict and carries no role: Provenance First requires that the
/// conflict stay visible rather than have the node pick a side.
fn resolve_role(
    publisher_rel: Option<&str>,
    music_rel: Option<&str>,
) -> (Option<String>, &'static str) {
    let publisher_value = publisher_rel.and_then(normalize_rel);
    let music_value = music_rel.and_then(normalize_rel);

    match (publisher_value, music_value) {
        (Some(value), None) => (Some(value), "publisher_rel"),
        (None, Some(value)) => (Some(value), "music_rel"),
        (Some(publisher_value), Some(music_value)) if publisher_value == music_value => {
            (Some(publisher_value), "publisher_rel")
        }
        (Some(_), Some(_)) => (None, "conflict"),
        (None, None) => (Some("artist".to_string()), "default"),
    }
}

/// Builds one `publisher` view row for a remote item of `current_feed`, or
/// `None` when the item's medium is neither `"publisher"` nor `"music"`.
/// ADR 0049 §3, §4 and §6.
fn build_publisher_row(
    conn: &rusqlite::Connection,
    current_feed: &Feed,
    item_medium: Option<&str>,
    item_remote_feed_guid: &str,
    item_remote_feed_url: Option<&str>,
    item_rel: Option<&str>,
) -> Result<Option<PublisherResponse>, api::ApiError> {
    let Some(direction) = publisher_direction(item_medium) else {
        return Ok(None);
    };

    let remote_feed_medium =
        db::get_feed(conn, item_remote_feed_guid)?.and_then(|feed| feed.raw_medium);

    let facts = match direction {
        "music_to_publisher" => music_to_publisher_facts(
            conn,
            current_feed,
            item_remote_feed_guid,
            item_remote_feed_url,
        )?,
        "publisher_to_music" => publisher_to_music_facts(
            conn,
            current_feed,
            item_remote_feed_guid,
            item_remote_feed_url,
        )?,
        _ => unreachable!("publisher_direction only returns the two names handled above"),
    };

    let reciprocal_declared = facts.music_names_publisher && facts.publisher_lists_music;

    // `item` (this row's own remote item) and `facts.matched_item_rel` (the
    // matched item on the other side, if any) give `publisher_rel` and
    // `music_rel` in an order that depends on which side `item` is on.
    // ADR 0049 §6, task 006 "Constraints".
    let own_rel = item_rel.map(str::to_string);
    let (publisher_rel, music_rel) = match direction {
        "music_to_publisher" => (facts.matched_item_rel, own_rel),
        "publisher_to_music" => (own_rel, facts.matched_item_rel),
        _ => unreachable!("publisher_direction only returns the two names handled above"),
    };
    let (role, role_source) = resolve_role(publisher_rel.as_deref(), music_rel.as_deref());

    Ok(Some(PublisherResponse {
        direction: direction.to_string(),
        remote_feed_guid: item_remote_feed_guid.to_string(),
        remote_feed_url: item_remote_feed_url.map(str::to_string),
        remote_feed_medium,
        publisher_feed_guid: facts.publisher_feed_guid,
        publisher_feed_url: facts.publisher_feed_url,
        music_feed_guid: facts.music_feed_guid,
        music_feed_url: facts.music_feed_url,
        music_names_publisher: facts.music_names_publisher,
        publisher_lists_music: facts.publisher_lists_music,
        publisher_link_resolution: facts.publisher_link_resolution.to_string(),
        publisher_link_observed_at: facts.publisher_link_observed_at,
        reciprocal_declared,
        reciprocal_medium: facts.reciprocal_medium,
        two_way_validated: reciprocal_declared,
        publisher_rel,
        music_rel,
        role,
        role_source: role_source.to_string(),
    }))
}

fn load_publisher(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<Vec<PublisherResponse>, api::ApiError> {
    let Some(current_feed) = db::get_feed(conn, feed_guid)? else {
        return Ok(Vec::new());
    };
    let remote_items = db::get_feed_remote_items_for_feed(conn, feed_guid)?;
    let mut rows = Vec::new();

    for item in remote_items {
        if let Some(row) = build_publisher_row(
            conn,
            &current_feed,
            item.medium.as_deref(),
            &item.remote_feed_guid,
            item.remote_feed_url.as_deref(),
            item.rel.as_deref(),
        )? {
            rows.push(row);
        }
    }

    Ok(rows)
}

fn load_track_remote_items(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<TrackRemoteItemResponse>, api::ApiError> {
    let items = db::get_track_remote_items_for_feed_track(conn, feed_guid, track_guid)?;
    Ok(items
        .into_iter()
        .map(|item| TrackRemoteItemResponse {
            position: item.position,
            medium: item.medium,
            remote_feed_guid: item.remote_feed_guid,
            remote_feed_url: item.remote_feed_url,
            rel: item.rel,
            source: item.source,
        })
        .collect())
}

fn load_track_publisher(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<PublisherResponse>, api::ApiError> {
    let Some(current_track) = db::get_track_for_feed(conn, feed_guid, track_guid)? else {
        return Ok(Vec::new());
    };
    // For item-level, the "music feed" is the track's parent feed.
    let current_feed =
        db::get_feed(conn, &current_track.feed_guid)?.ok_or_else(|| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "orphaned track".into(),
            www_authenticate: None,
        })?;
    let remote_items = db::get_track_remote_items_for_feed_track(conn, feed_guid, track_guid)?;
    let mut rows = Vec::new();

    for item in remote_items {
        if let Some(row) = build_publisher_row(
            conn,
            &current_feed,
            item.medium.as_deref(),
            &item.remote_feed_guid,
            item.remote_feed_url.as_deref(),
            item.rel.as_deref(),
        )? {
            rows.push(row);
        }
    }

    Ok(rows)
}

fn publisher_direction(medium: Option<&str>) -> Option<&'static str> {
    match medium {
        Some("publisher") => Some("music_to_publisher"),
        Some("music") => Some("publisher_to_music"),
        _ => None,
    }
}

// ── GET /v1/tracks/{guid} ────────────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single paginated-detail flow with optional includes"
)]
async fn handle_get_track(
    State(state): State<Arc<api::AppState>>,
    Path(track_guid): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<Response, api::ApiError> {
    let state2 = Arc::clone(&state);
    let track_guid2 = track_guid.clone();
    let result = tokio::task::spawn_blocking(move || {
        // Mutex safety compliant — 2026-03-12
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let rows = get_track_rows_by_guid(&conn, &track_guid2)?;
        match rows.len() {
            0 => Err(api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: "track not found".into(),
                www_authenticate: None,
            }),
            1 => {
                let resp = build_track_response(
                    &conn,
                    rows.into_iter().next().expect("one row"),
                    &params,
                )?;
                Ok::<_, api::ApiError>(EitherTrackResponse::Found(Box::new(resp)))
            }
            _ => Ok(EitherTrackResponse::Ambiguous(
                rows.iter()
                    .map(|row| api::AmbiguousTrackGuidCandidate {
                        feed_guid: row.feed_guid.clone(),
                        href: api::canonical_track_href(&row.feed_guid, &row.track_guid),
                    })
                    .collect(),
            )),
        }
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    match result {
        EitherTrackResponse::Found(resp) => Ok(Json(QueryResponse {
            data: *resp,
            pagination: Pagination {
                cursor: None,
                has_more: false,
            },
            meta: meta(&state),
        })
        .into_response()),
        EitherTrackResponse::Ambiguous(candidates) => {
            Ok(api::ambiguous_track_guid_response(&track_guid, candidates))
        }
    }
}

enum EitherTrackResponse {
    Found(Box<TrackResponse>),
    Ambiguous(Vec<api::AmbiguousTrackGuidCandidate>),
}

#[allow(
    clippy::too_many_lines,
    reason = "single paginated-detail flow with optional includes"
)]
async fn handle_get_feed_track(
    State(state): State<Arc<api::AppState>>,
    Path((feed_guid, track_guid)): Path<(String, String)>,
    Query(params): Query<ListQuery>,
) -> Result<Response, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let row = get_track_row_for_feed(&conn, &feed_guid, &track_guid)?.ok_or_else(|| {
            api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("track {track_guid} not found in feed {feed_guid}"),
                www_authenticate: None,
            }
        })?;
        build_track_response(&conn, row, &params)
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    Ok(Json(QueryResponse {
        data: result,
        pagination: Pagination {
            cursor: None,
            has_more: false,
        },
        meta: meta(&state),
    })
    .into_response())
}

// ── GET /v1/feeds/recent ────────────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single paginated-list flow with two SQL branches"
)]
async fn handle_get_recent_feeds(
    State(state): State<Arc<api::AppState>>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        // Mutex safety compliant — 2026-03-12
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let limit = params.capped_limit();
        // ADR 0047: a corrective pass needs every feed the index holds, not one
        // medium. `all` skips the filter. The value is lowered here so the SQL
        // compares one lowered column against one lowered bind.
        let medium = params.medium.as_deref().unwrap_or("music").to_lowercase();

        let rows: Vec<FeedRow> = if let Some(ref cursor_str) = params.cursor {
            let decoded = decode_cursor(cursor_str)?;
            let parts: Vec<&str> = decoded.splitn(2, '\0').collect();
            if parts.len() != 2 {
                return Err(api::ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "invalid cursor format".into(),
                    www_authenticate: None,
                });
            }
            let cursor_ts: i64 = parts[0].parse().map_err(|_err| api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "invalid cursor timestamp".into(),
                www_authenticate: None,
            })?;
            let cursor_guid = parts[1];

            let mut stmt = conn.prepare(
                "SELECT feed_guid, feed_url, title, raw_medium, release_artist, \
                 release_artist_sort, release_date, release_kind, description, image_url, publisher, language, explicit, \
                 episode_count, newest_item_at, oldest_item_at, \
                 created_at, updated_at, last_build_date, release_artist_source \
                 FROM feeds \
                 WHERE (?1 = 'all' OR lower(raw_medium) = ?1)
                   AND (newest_item_at, feed_guid) < (?2, ?3) \
                 ORDER BY newest_item_at DESC, feed_guid DESC \
                 LIMIT ?4",
            )?;
            stmt.query_map(
                params![medium, cursor_ts, cursor_guid, limit + 1],
                |row| {
                    Ok(FeedRow {
                        feed_guid: row.get(0)?,
                        feed_url: row.get(1)?,
                        title: row.get(2)?,
                        raw_medium: row.get(3)?,
                        release_artist: row.get(4)?,
                        release_artist_sort: row.get(5)?,
                        release_date: row.get(6)?,
                        release_kind: row.get(7)?,
                        description: row.get(8)?,
                        image_url: row.get(9)?,
                        publisher_text: row.get(10)?,
                        language: row.get(11)?,
                        explicit_int: row.get(12)?,
                        episode_count: row.get(13)?,
                        newest_item_at: row.get(14)?,
                        oldest_item_at: row.get(15)?,
                        created_at: row.get(16)?,
                        updated_at: row.get(17)?,
                        last_build_date: row.get(18)?,
                        release_artist_source: row.get(19)?,
                    })
                },
            )?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT feed_guid, feed_url, title, raw_medium, release_artist, \
                 release_artist_sort, release_date, release_kind, description, image_url, publisher, language, explicit, \
                 episode_count, newest_item_at, oldest_item_at, \
                 created_at, updated_at, last_build_date, release_artist_source \
                 FROM feeds \
                 WHERE (?1 = 'all' OR lower(raw_medium) = ?1) \
                 ORDER BY newest_item_at DESC, feed_guid DESC \
                 LIMIT ?2",
            )?;
            stmt.query_map(params![medium, limit + 1], |row| {
                Ok(FeedRow {
                    feed_guid: row.get(0)?,
                    feed_url: row.get(1)?,
                    title: row.get(2)?,
                    raw_medium: row.get(3)?,
                    release_artist: row.get(4)?,
                    release_artist_sort: row.get(5)?,
                    release_date: row.get(6)?,
                    release_kind: row.get(7)?,
                    description: row.get(8)?,
                    image_url: row.get(9)?,
                    publisher_text: row.get(10)?,
                    language: row.get(11)?,
                    explicit_int: row.get(12)?,
                    episode_count: row.get(13)?,
                    newest_item_at: row.get(14)?,
                    oldest_item_at: row.get(15)?,
                    created_at: row.get(16)?,
                    updated_at: row.get(17)?,
                    last_build_date: row.get(18)?,
                    release_artist_source: row.get(19)?,
                })
            })?
            .collect::<Result<_, _>>()?
        };

        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let items: Vec<_> = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect();

        let next_cursor = if has_more {
            items.last().and_then(|r| {
                r.newest_item_at
                    .map(|ts| encode_cursor(&format!("{ts}\0{}", r.feed_guid)))
            })
        } else {
            None
        };

        let mut feeds = Vec::with_capacity(items.len());
        for r in items {
            let publisher_feed_title = resolve_publisher_feed_title(&conn, &r.feed_guid)?;
            feeds.push(FeedResponse {
                feed_guid: r.feed_guid,
                feed_url: r.feed_url,
                title: r.title,
                raw_medium: r.raw_medium,
                release_artist: r.release_artist,
                release_artist_source: r.release_artist_source,
                release_artist_sort: r.release_artist_sort,
                release_date: r.release_date,
                last_build_date: r.last_build_date,
                release_kind: r.release_kind,
                description: r.description,
                image_url: r.image_url,
                publisher_text: r.publisher_text,
                publisher_feed_title,
                // The list route does not compute this per-row aggregate.
                // ADR 0049 §7 scopes it to a single feed read.
                distinct_release_artist_count: None,
                distinct_release_artists: None,
                language: r.language,
                explicit: r.explicit_int != 0,
                episode_count: r.episode_count,
                newest_item_at: r.newest_item_at,
                oldest_item_at: r.oldest_item_at,
                created_at: r.created_at,
                updated_at: r.updated_at,
                tracks: None,
                payment_routes: None,
                source_links: None,
                source_ids: None,
                source_contributors: None,
                source_platforms: None,
                source_release_claims: None,
                remote_items: None,
                publisher: None,
            });
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: feeds,
            pagination: Pagination {
                cursor: next_cursor,
                has_more,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    Ok(Json(result))
}

// ── GET /v1/search ──────────────────────────────────────────────────────────

// Issue-SEARCH-KEYSET — 2026-03-14
async fn handle_search(
    State(state): State<Arc<api::AppState>>,
    Query(params): Query<SearchQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let q = params.q.clone();
    let kind = params.kind.clone();
    tracing::info!(q = ?q, kind = ?kind, "search query received");
    // Issue-NEGATIVE-LIMIT — 2026-03-15
    let limit = params.limit.unwrap_or(20).clamp(1, 100);

    // Issue-SEARCH-KEYSET — 2026-03-14
    // Parse keyset cursor: base64(f64_bits_as_decimal \0 rowid_as_decimal).
    // The f64 rank is encoded via `f64::to_bits()` for a lossless round-trip.
    let (cursor_rank, cursor_rowid) = if let Some(ref cursor_str) = params.cursor {
        let decoded = decode_cursor(cursor_str)?;
        let parts: Vec<&str> = decoded.splitn(2, '\0').collect();
        if parts.len() != 2 {
            return Err(api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "invalid cursor format".into(),
                www_authenticate: None,
            });
        }
        let rank_bits: u64 = parts[0].parse().map_err(|_err| api::ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "invalid cursor rank".into(),
            www_authenticate: None,
        })?;
        let rowid: i64 = parts[1].parse().map_err(|_err| api::ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "invalid cursor rowid".into(),
            www_authenticate: None,
        })?;
        let rank = f64::from_bits(rank_bits);
        if rank.is_nan() || rank.is_infinite() {
            return Err(api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "invalid cursor rank: non-finite value".into(),
                www_authenticate: None,
            });
        }
        (Some(rank), Some(rowid))
    } else {
        (None, None)
    };

    let pool = state.db.clone();
    // Issue-WAL-POOL — 2026-03-14: use reader pool for search
    let results = tokio::task::spawn_blocking(move || {
        let conn = pool.reader()?;
        match kind.as_deref() {
            Some("feed" | "track") => crate::search::search(
                &conn,
                &q,
                kind.as_deref(),
                limit + 1,
                cursor_rank,
                cursor_rowid,
            ),
            Some(other) => Err(db::DbError::Other(format!(
                "unsupported search type filter: {other}"
            ))),
            None => {
                let mut merged = Vec::new();
                for entity_type in ["feed", "track"] {
                    merged.extend(crate::search::search(
                        &conn,
                        &q,
                        Some(entity_type),
                        limit + 1,
                        cursor_rank,
                        cursor_rowid,
                    )?);
                }
                merged.sort_by(|a, b| {
                    a.effective_rank
                        .partial_cmp(&b.effective_rank)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.rowid.cmp(&b.rowid))
                });
                merged.truncate(usize::try_from(limit + 1).unwrap_or(usize::MAX));
                Ok(merged)
            }
        }
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?
    // Issue-21 FTS5 sanitize — 2026-03-13
    // Catch FTS5 parse errors and return 400 instead of 500.
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unsupported search type filter:") {
            api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: msg,
                www_authenticate: None,
            }
        } else if msg.contains("fts5: syntax error") || msg.contains("fts5:") {
            api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!("invalid search query: {msg}"),
                www_authenticate: None,
            }
        } else {
            api::ApiError::from(e)
        }
    })?;

    let has_more = results.len() > usize::try_from(limit).unwrap_or(0);

    // Issue-SEARCH-KEYSET — 2026-03-14
    // Encode keyset cursor from the last result's (effective_rank, rowid).
    let next_cursor = if has_more {
        let limit_usize = usize::try_from(limit).unwrap_or(0);
        results.get(limit_usize.saturating_sub(1)).map(|r| {
            let rank_bits = r.effective_rank.to_bits();
            encode_cursor(&format!("{}\0{}", rank_bits, r.rowid))
        })
    } else {
        None
    };

    let limited_results: Vec<_> = results
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(0))
        .collect();
    let pool = state.db.clone();
    let data =
        tokio::task::spawn_blocking(move || -> Result<Vec<SearchResponseItem>, api::ApiError> {
            let conn = pool.reader().map_err(api::ApiError::from)?;
            limited_results
                .into_iter()
                .map(|r| {
                    let (entity_id, feed_guid, href) = if r.entity_type == "track" {
                        if let Some((feed_guid, track_guid)) =
                            db::parse_canonical_track_entity_id(&r.entity_id)
                        {
                            let href = Some(api::canonical_track_href(&feed_guid, &track_guid));
                            (track_guid, Some(feed_guid), href)
                        } else {
                            let track = db::get_track_by_guid(&conn, &r.entity_id)
                                .map_err(api::ApiError::from)?;
                            let feed_guid = track.as_ref().map(|track| track.feed_guid.clone());
                            let href = track.as_ref().map(|track| {
                                api::canonical_track_href(&track.feed_guid, &track.track_guid)
                            });
                            (r.entity_id.clone(), feed_guid, href)
                        }
                    } else {
                        (r.entity_id.clone(), None, None)
                    };
                    let mut summary = SearchSummary::default();
                    if r.entity_type == "track" {
                        if let Some(feed_guid) = feed_guid.as_deref()
                            && let Some(row) = get_track_row_for_feed(&conn, feed_guid, &entity_id)?
                        {
                            summary.title = Some(row.title);
                            summary.feed_title = Some(row.feed_title);
                            summary.track_image_url = row.track_image_url;
                            summary.feed_image_url = row.feed_image_url;
                            summary.pub_date = row.pub_date;
                        }
                    } else {
                        summary = feed_search_summary(&conn, &entity_id)?;
                    }

                    Ok(SearchResponseItem {
                        entity_type: r.entity_type,
                        entity_id,
                        rank: r.rank,
                        quality_score: r.quality_score,
                        feed_guid,
                        href,
                        title: summary.title,
                        feed_title: summary.feed_title,
                        track_image_url: summary.track_image_url,
                        feed_image_url: summary.feed_image_url,
                        pub_date: summary.pub_date,
                    })
                })
                .collect()
        })
        .await
        .map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("internal task panic: {e}"),
            www_authenticate: None,
        })??;

    Ok(Json(QueryResponse {
        data,
        pagination: Pagination {
            cursor: next_cursor,
            has_more,
        },
        meta: meta(&state),
    }))
}

// ── GET /v1/node/capabilities ───────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
struct CapabilitiesResponse {
    api_version: &'static str,
    node_pubkey: String,
    capabilities: Vec<&'static str>,
    entity_types: Vec<&'static str>,
    include_params: HashMap<&'static str, Vec<&'static str>>,
}

async fn handle_capabilities(State(state): State<Arc<api::AppState>>) -> impl IntoResponse {
    let mut include_params = HashMap::new();
    include_params.insert(
        "feed",
        vec![
            "tracks",
            "payment_routes",
            "source_links",
            "source_ids",
            "source_contributors",
            "source_platforms",
            "source_release_claims",
            "remote_items",
            "publisher",
        ],
    );
    include_params.insert(
        "track",
        vec![
            "payment_routes",
            "value_time_splits",
            "source_links",
            "source_ids",
            "source_contributors",
            "source_release_claims",
            "source_enclosures",
            "source_transcripts",
        ],
    );
    Json(CapabilitiesResponse {
        api_version: "v1",
        node_pubkey: state.node_pubkey_hex.clone(),
        capabilities: vec!["query", "search", "sync", "push"],
        entity_types: vec!["feed", "track"],
        include_params,
    })
}

// ── GET /v1/peers ───────────────────────────────────────────────────────────

async fn handle_get_peers(
    State(state): State<Arc<api::AppState>>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        // Mutex safety compliant — 2026-03-12
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let mut stmt = conn.prepare(
            "SELECT node_pubkey, node_url, last_push_at FROM peer_nodes ORDER BY node_pubkey",
        )?;
        let peers: Vec<PeerResponse> = stmt
            .query_map([], |row| {
                Ok(PeerResponse {
                    node_pubkey: row.get(0)?,
                    node_url: row.get(1)?,
                    last_push_at: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok::<_, api::ApiError>(peers)
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;
    Ok(Json(result))
}

// ── GET /v1/publishers ────────────────────────────────────────────────────────

async fn handle_publisher_search(
    State(state): State<Arc<api::AppState>>,
    Query(params): Query<PublisherSearchQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let case_sensitive = params.case_sensitive();
    let q = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let publisher_match = if case_sensitive {
            "(?1 = '' OR instr(publisher, ?1) > 0)"
        } else {
            "publisher LIKE ?1 ESCAPE '\\' COLLATE NOCASE"
        };
        let match_arg = if case_sensitive {
            q
        } else {
            like_contains_pattern(&q)
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT publisher, COUNT(*) as feed_count \
             FROM feeds \
             WHERE publisher IS NOT NULL AND publisher != '' \
               AND lower(raw_medium) = 'music' \
               AND {publisher_match} \
             GROUP BY publisher \
             ORDER BY feed_count DESC, publisher ASC \
             LIMIT ?2"
        ))?;
        let items: Vec<PublisherSearchItem> = stmt
            .query_map(params![match_arg, limit], |row| {
                let publisher_text: String = row.get(0)?;
                let feed_count: i64 = row.get(1)?;
                Ok((publisher_text, feed_count))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(publisher_text, feed_count)| {
                // Count tracks for this publisher
                let track_count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tracks WHERE publisher = ?1",
                        params![publisher_text],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                PublisherSearchItem {
                    publisher_text,
                    feed_count,
                    track_count,
                }
            })
            .collect();
        Ok::<_, api::ApiError>(QueryResponse {
            data: items,
            pagination: Pagination {
                cursor: None,
                has_more: false,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;
    Ok(Json(result))
}

// ── GET /v1/publishers/{publisher} ────────────────────────────────────────────

async fn handle_publisher_detail(
    State(state): State<Arc<api::AppState>>,
    Path(publisher): Path<String>,
    Query(params): Query<PublisherDetailQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let limit = params.capped_limit();
        let publisher_text = publisher.clone();
        let (feed_match, track_match, match_arg) = if params.case_sensitive() {
            (
                "instr(publisher, ?1) > 0",
                "instr(t.publisher, ?1) > 0",
                publisher,
            )
        } else {
            (
                "publisher LIKE ?1 ESCAPE '\\' COLLATE NOCASE",
                "t.publisher LIKE ?1 ESCAPE '\\' COLLATE NOCASE",
                like_contains_pattern(&publisher),
            )
        };

        let mut fstmt = conn.prepare(&format!(
            "SELECT feed_guid, feed_url, title, image_url, episode_count, raw_medium \
             FROM feeds WHERE {feed_match} \
             AND lower(raw_medium) = 'music' \
             ORDER BY newest_item_at DESC LIMIT ?2"
        ))?;
        let feeds: Vec<PublisherFeedSummary> = fstmt
            .query_map(params![match_arg, limit], |row| {
                Ok(PublisherFeedSummary {
                    feed_guid: row.get(0)?,
                    feed_url: row.get(1)?,
                    title: row.get(2)?,
                    image_url: row.get(3)?,
                    episode_count: row.get(4)?,
                    raw_medium: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut tstmt = conn.prepare(&format!(
            "SELECT t.track_guid, t.feed_guid, t.title, COALESCE(t.image_url, f.image_url), \
             t.duration_secs, t.track_number, t.image_url, f.image_url \
             FROM tracks t JOIN feeds f ON f.feed_guid = t.feed_guid \
             WHERE {track_match} \
             AND lower(f.raw_medium) = 'music' \
             ORDER BY t.pub_date DESC LIMIT ?2"
        ))?;
        let tracks: Vec<PublisherTrackSummary> = tstmt
            .query_map(params![match_arg, limit], |row| {
                Ok(PublisherTrackSummary {
                    track_guid: row.get(0)?,
                    feed_guid: row.get(1)?,
                    title: row.get(2)?,
                    image_url: row.get(3)?,
                    duration_secs: row.get(4)?,
                    track_number: row.get(5)?,
                    track_image_url: row.get(6)?,
                    feed_image_url: row.get(7)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        Ok::<_, api::ApiError>(QueryResponse {
            data: PublisherDetailResponse {
                publisher_text,
                feeds,
                tracks,
            },
            pagination: Pagination {
                cursor: None,
                has_more: false,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;
    Ok(Json(result))
}

// ── GET /v1/publisher-links/stats ───────────────────────────────────────────

/// The counts `GET /v1/publisher-links/stats` gives (ADR 0049 §8, plan
/// decision 9). `listed_links` is the sum of the other three fields.
#[derive(Debug, Serialize, ToSchema)]
struct PublisherLinkStatsResponse {
    listed_links: i64,
    resolved_by_guid: i64,
    resolved_by_feed_url: i64,
    unresolved: i64,
}

impl From<db::PublisherLinkStats> for PublisherLinkStatsResponse {
    fn from(stats: db::PublisherLinkStats) -> Self {
        Self {
            listed_links: stats.listed_links,
            resolved_by_guid: stats.resolved_by_guid,
            resolved_by_feed_url: stats.resolved_by_feed_url,
            unresolved: stats.unresolved,
        }
    }
}

async fn handle_publisher_link_stats(
    State(state): State<Arc<api::AppState>>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let link_stats = db::get_publisher_link_stats(&conn)?;
        Ok::<_, api::ApiError>(QueryResponse {
            data: PublisherLinkStatsResponse::from(link_stats),
            pagination: Pagination {
                cursor: None,
                has_more: false,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;
    Ok(Json(result))
}

// ── GET /v1/tracks ──────────────────────────────────────────────────────────

async fn handle_artist_tracks(
    State(state): State<Arc<api::AppState>>,
    Query(params): Query<ArtistTracksQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let artist_lower = params.artist.to_lowercase();
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let limit = params.capped_limit();

        let rows: Vec<ArtistTrackItem> = if let Some(ref cursor_str) = params.cursor {
            let decoded = decode_cursor(cursor_str)?;
            let parts: Vec<&str> = decoded.splitn(2, '\0').collect();
            if parts.len() != 2 {
                return Err(api::ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "invalid cursor format".into(),
                    www_authenticate: None,
                });
            }
            let cursor_ts: i64 = parts[0].parse().map_err(|_err| api::ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "invalid cursor timestamp".into(),
                www_authenticate: None,
            })?;
            let cursor_guid = parts[1];

            let mut stmt = conn.prepare(
                "SELECT t.track_guid, t.feed_guid, t.title, t.track_artist, t.track_artist_sort, \
                 t.pub_date, t.duration_secs, COALESCE(t.image_url, f.image_url), t.track_number, \
                 COALESCE(f.title, ''), f.release_artist, t.created_at, t.image_url, \
                 f.image_url, f.release_artist_source \
                 FROM tracks t LEFT JOIN feeds f ON f.feed_guid = t.feed_guid \
                 WHERE lower(t.track_artist) = ?1 \
                   AND (t.created_at, t.track_guid) < (?2, ?3) \
                 ORDER BY t.created_at DESC, t.track_guid DESC \
                 LIMIT ?4",
            )?;
            stmt.query_map(
                params![artist_lower, cursor_ts, cursor_guid, limit + 1],
                |row| {
                    Ok(ArtistTrackItem {
                        track_guid: row.get(0)?,
                        feed_guid: row.get(1)?,
                        title: row.get(2)?,
                        track_artist: row.get(3)?,
                        track_artist_sort: row.get(4)?,
                        pub_date: row.get(5)?,
                        duration_secs: row.get(6)?,
                        image_url: row.get(7)?,
                        track_number: row.get(8)?,
                        feed_title: row.get(9)?,
                        release_artist: row.get(10)?,
                        created_at: row.get(11)?,
                        track_image_url: row.get(12)?,
                        feed_image_url: row.get(13)?,
                        release_artist_source: row.get(14)?,
                    })
                },
            )?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT t.track_guid, t.feed_guid, t.title, t.track_artist, t.track_artist_sort, \
                 t.pub_date, t.duration_secs, COALESCE(t.image_url, f.image_url), t.track_number, \
                 COALESCE(f.title, ''), f.release_artist, t.created_at, t.image_url, \
                 f.image_url, f.release_artist_source \
                 FROM tracks t LEFT JOIN feeds f ON f.feed_guid = t.feed_guid \
                 WHERE lower(t.track_artist) = ?1 \
                 ORDER BY t.created_at DESC, t.track_guid DESC \
                 LIMIT ?2",
            )?;
            stmt.query_map(params![artist_lower, limit + 1], |row| {
                Ok(ArtistTrackItem {
                    track_guid: row.get(0)?,
                    feed_guid: row.get(1)?,
                    title: row.get(2)?,
                    track_artist: row.get(3)?,
                    track_artist_sort: row.get(4)?,
                    pub_date: row.get(5)?,
                    duration_secs: row.get(6)?,
                    image_url: row.get(7)?,
                    track_number: row.get(8)?,
                    feed_title: row.get(9)?,
                    release_artist: row.get(10)?,
                    created_at: row.get(11)?,
                    track_image_url: row.get(12)?,
                    feed_image_url: row.get(13)?,
                    release_artist_source: row.get(14)?,
                })
            })?
            .collect::<Result<_, _>>()?
        };

        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let items: Vec<_> = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect();

        let next_cursor = if has_more {
            items
                .last()
                .map(|r| encode_cursor(&format!("{}\0{}", r.created_at, r.track_guid)))
        } else {
            None
        };

        Ok::<_, api::ApiError>(QueryResponse {
            data: items,
            pagination: Pagination {
                cursor: next_cursor,
                has_more,
            },
            meta: meta(&state2),
        })
    })
    .await
    .map_err(|e| api::ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    Ok(Json(result))
}

// ── Router builder ──────────────────────────────────────────────────────────

use axum::routing::get;

pub fn query_routes() -> axum::Router<Arc<api::AppState>> {
    axum::Router::new()
        .route("/v1/feeds/{guid}", get(handle_get_feed))
        .route(
            "/v1/feeds/{guid}/tracks/{track_guid}",
            get(handle_get_feed_track),
        )
        .route("/v1/feeds/recent", get(handle_get_recent_feeds))
        .route("/v1/tracks", get(handle_artist_tracks))
        .route("/v1/tracks/{guid}", get(handle_get_track))
        .route("/v1/search", get(handle_search))
        .route("/v1/node/capabilities", get(handle_capabilities))
        .route("/v1/peers", get(handle_get_peers))
        .route("/v1/publishers", get(handle_publisher_search))
        .route("/v1/publishers/{publisher}", get(handle_publisher_detail))
        .route(
            "/v1/publisher-links/stats",
            get(handle_publisher_link_stats),
        )
}

// ── OpenAPI schema registration (ADR 0044) ──────────────────────────────────

/// A named `OpenAPI` schema, paired for insertion into `components.schemas`.
type SchemaEntry = (
    String,
    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
);

/// Pushes `T`'s own schema, plus every schema `T` references, onto `schemas`.
fn register_schema<T: ToSchema>(schemas: &mut Vec<SchemaEntry>) {
    schemas.push((T::name().into_owned(), T::schema()));
    T::schemas(schemas);
}

/// Schemas for every documented `/v1/*` JSON response this module returns.
///
/// Read by `openapi::spec_value` to fill `components.schemas`. No response
/// references these schemas yet (ADR 0044 task 001); a later task adds that.
pub(crate) fn response_schemas() -> Vec<SchemaEntry> {
    let mut schemas = Vec::new();
    register_schema::<Pagination>(&mut schemas);
    register_schema::<ResponseMeta>(&mut schemas);
    register_schema::<FeedResponse>(&mut schemas);
    register_schema::<TrackResponse>(&mut schemas);
    register_schema::<CapabilitiesResponse>(&mut schemas);
    register_schema::<PeerResponse>(&mut schemas);
    register_schema::<PublisherSearchItem>(&mut schemas);
    register_schema::<PublisherDetailResponse>(&mut schemas);
    register_schema::<SearchResponseItem>(&mut schemas);
    register_schema::<ArtistTrackItem>(&mut schemas);
    register_schema::<PublisherLinkStatsResponse>(&mut schemas);
    schemas
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_rel ──────────────────────────────────────────────────────

    #[test]
    fn normalize_rel_trims_and_lowercases() {
        assert_eq!(normalize_rel(" Label "), Some("label".to_string()));
    }

    #[test]
    fn normalize_rel_empty_after_trim_is_no_value() {
        assert_eq!(normalize_rel("   "), None);
    }

    // Renamed from `normalize_rel_keeps_a_comma_as_one_value` (task 006b,
    // plan decision 13). The old rule kept a comma-bearing value as one
    // un-split value: `normalize_rel("sound engineer,  Mastering
    // Engineer")` gave `Some("sound engineer,  mastering   engineer")`
    // (internal spacing kept, no split). The new rule splits on the comma,
    // collapses and trims each role, and sorts the set by byte order.
    #[test]
    fn normalize_rel_splits_a_comma_list_into_a_sorted_role_set() {
        assert_eq!(
            normalize_rel("sound engineer,  Mastering   Engineer"),
            Some("mastering engineer, sound engineer".to_string())
        );
    }

    #[test]
    fn normalize_rel_sorts_multiple_roles() {
        assert_eq!(
            normalize_rel("Artist, Producer"),
            Some("artist, producer".to_string())
        );
    }

    #[test]
    fn normalize_rel_a_value_with_no_comma_is_one_role() {
        assert_eq!(
            normalize_rel("artist producer"),
            Some("artist producer".to_string())
        );
    }

    #[test]
    fn normalize_rel_all_empty_parts_is_no_value() {
        assert_eq!(normalize_rel(" , ,"), None);
    }

    #[test]
    fn normalize_rel_deduplicates_roles_after_lowercasing() {
        assert_eq!(normalize_rel("Label, label"), Some("label".to_string()));
    }

    // ── resolve_role: one case per table row (task 006 "Constraints") ──────

    #[test]
    fn resolve_role_publisher_rel_only() {
        assert_eq!(
            resolve_role(Some("label"), None),
            (Some("label".to_string()), "publisher_rel")
        );
    }

    #[test]
    fn resolve_role_music_rel_only() {
        assert_eq!(
            resolve_role(None, Some("artist")),
            (Some("artist".to_string()), "music_rel")
        );
    }

    #[test]
    fn resolve_role_both_equal_after_normalization() {
        assert_eq!(
            resolve_role(Some(" Label "), Some("label")),
            (Some("label".to_string()), "publisher_rel")
        );
    }

    #[test]
    fn resolve_role_equal_role_sets_in_different_order() {
        assert_eq!(
            resolve_role(Some("producer, artist"), Some("Artist, Producer")),
            (Some("artist, producer".to_string()), "publisher_rel")
        );
    }

    #[test]
    fn resolve_role_both_different_is_conflict() {
        assert_eq!(
            resolve_role(Some("artist, producer"), Some("artist")),
            (None, "conflict")
        );
    }

    #[test]
    fn resolve_role_neither_defaults_to_artist() {
        assert_eq!(
            resolve_role(None, None),
            (Some("artist".to_string()), "default")
        );
    }

    // ── normalize_release_artist (task 008 "Constraints") ───────────────────

    #[test]
    fn normalize_release_artist_trims_and_collapses_and_lowercases() {
        assert_eq!(normalize_release_artist("  Jimmy   V "), "jimmy v");
        assert_eq!(normalize_release_artist("jimmy v"), "jimmy v");
    }

    #[test]
    fn normalize_release_artist_does_not_split_a_feat_credit() {
        assert_ne!(
            normalize_release_artist("A feat. B"),
            normalize_release_artist("A")
        );
    }
}
