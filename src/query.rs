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
    response::IntoResponse,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{api, db};

// ── Pagination ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct QueryResponse<T> {
    data: T,
    pagination: Pagination,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct FeedResponse {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    release_artist: Option<String>,
    release_artist_sort: Option<String>,
    release_date: Option<i64>,
    release_kind: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    publisher_text: Option<String>,
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

#[derive(Debug, Serialize)]
struct TrackSummary {
    track_guid: String,
    title: String,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    image_url: Option<String>,
    track_number: Option<i64>,
    publisher_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct RouteResponse {
    recipient_name: Option<String>,
    route_type: String,
    address: String,
    custom_key: Option<String>,
    custom_value: Option<String>,
    split: i64,
    fee: bool,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct PublisherSearchItem {
    publisher_text: String,
    feed_count: i64,
    track_count: i64,
}

#[derive(Debug, Serialize)]
struct PublisherFeedSummary {
    feed_guid: String,
    feed_url: String,
    title: String,
    image_url: Option<String>,
    episode_count: Option<i64>,
    raw_medium: Option<String>,
}

#[derive(Debug, Serialize)]
struct PublisherTrackSummary {
    track_guid: String,
    feed_guid: String,
    title: String,
    image_url: Option<String>,
    duration_secs: Option<i64>,
    track_number: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PublisherDetailResponse {
    publisher_text: String,
    feeds: Vec<PublisherFeedSummary>,
    tracks: Vec<PublisherTrackSummary>,
}

#[derive(Debug, Serialize)]
struct VtsResponse {
    start_time_secs: i64,
    duration_secs: Option<i64>,
    remote_feed_guid: String,
    remote_item_guid: String,
    split: i64,
}

#[derive(Debug, Serialize)]
struct PeerResponse {
    node_pubkey: String,
    node_url: String,
    last_push_at: Option<i64>,
}

#[derive(Debug, Serialize)]
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
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct SourcePlatformClaimResponse {
    platform_key: String,
    url: Option<String>,
    owner_name: Option<String>,
    source: String,
    extraction_path: String,
    observed_at: i64,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct FeedRemoteItemResponse {
    position: i64,
    medium: Option<String>,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    source: String,
}

#[derive(Debug, Serialize)]
struct TrackRemoteItemResponse {
    position: i64,
    medium: Option<String>,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    source: String,
}

#[derive(Debug, Serialize)]
struct PublisherResponse {
    direction: String,
    remote_feed_guid: String,
    remote_feed_url: Option<String>,
    remote_feed_medium: Option<String>,
    publisher_feed_guid: String,
    publisher_feed_url: Option<String>,
    music_feed_guid: String,
    music_feed_url: Option<String>,
    reciprocal_declared: bool,
    reciprocal_medium: Option<String>,
    two_way_validated: bool,
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
}

/// Intermediate row type for feed queries to avoid complex tuple types.
struct FeedRow {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    release_artist: Option<String>,
    release_artist_sort: Option<String>,
    release_date: Option<i64>,
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
             language, explicit, episode_count, newest_item_at, oldest_item_at, created_at, updated_at \
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

fn build_feed_response(
    conn: &rusqlite::Connection,
    row: FeedRow,
    params: &ListQuery,
) -> Result<FeedResponse, api::ApiError> {
    let feed_guid = row.feed_guid.clone();

    let mut resp = FeedResponse {
        feed_guid: row.feed_guid,
        feed_url: row.feed_url,
        title: row.title,
        raw_medium: row.raw_medium,
        release_artist: row.release_artist,
        release_artist_sort: row.release_artist_sort,
        release_date: row.release_date,
        release_kind: row.release_kind,
        description: row.description,
        image_url: row.image_url,
        publisher_text: row.publisher_text,
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

    if params.includes("tracks") {
        let mut stmt = conn.prepare(
            "SELECT track_guid, title, pub_date, duration_secs, image_url, track_number, publisher \
             FROM tracks WHERE feed_guid = ?1 ORDER BY track_number ASC, pub_date DESC",
        )?;
        let tracks: Vec<TrackSummary> = stmt
            .query_map(params![feed_guid], |row| {
                Ok(TrackSummary {
                    track_guid: row.get(0)?,
                    title: row.get(1)?,
                    pub_date: row.get(2)?,
                    duration_secs: row.get(3)?,
                    image_url: row.get(4)?,
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
        let mut stmt = conn.prepare(
            "SELECT recipient_name, route_type, address, NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
             FROM payment_routes WHERE track_guid = ?1",
        )?;
        let routes: Vec<RouteResponse> = stmt
            .query_map(params![track_guid], |row| {
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
        let mut stmt = conn.prepare(
            "SELECT start_time_secs, duration_secs, remote_feed_guid, remote_item_guid, split \
             FROM value_time_splits WHERE source_track_guid = ?1",
        )?;
        let vts: Vec<VtsResponse> = stmt
            .query_map(params![track_guid], |row| {
                Ok(VtsResponse {
                    start_time_secs: row.get(0)?,
                    duration_secs: row.get(1)?,
                    remote_feed_guid: row.get(2)?,
                    remote_item_guid: row.get(3)?,
                    split: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        resp.value_time_splits = Some(vts);
    }

    if params.includes("source_links") {
        resp.source_links = Some(
            db::get_source_entity_links_for_entity(conn, "track", &track_guid)?
                .into_iter()
                .map(entity_link_response)
                .collect(),
        );
    }

    if params.includes("source_ids") {
        resp.source_ids = Some(
            db::get_source_entity_ids_for_entity(conn, "track", &track_guid)?
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
            db::get_source_release_claims_for_entity(conn, "track", &track_guid)?
                .into_iter()
                .map(release_claim_response)
                .collect(),
        );
    }

    if params.includes("source_enclosures") {
        resp.source_enclosures = Some(
            db::get_source_item_enclosures_for_entity(conn, "track", &track_guid)?
                .into_iter()
                .map(enclosure_response)
                .collect(),
        );
    }

    if params.includes("source_transcripts") {
        resp.source_transcripts = Some(
            db::get_source_item_transcripts_for_entity(conn, "track", &track_guid)?
                .into_iter()
                .map(transcript_response)
                .collect(),
        );
    }

    if params.includes("remote_items") {
        resp.remote_items = Some(load_track_remote_items(conn, &track_guid)?);
    }

    if params.includes("publisher") {
        resp.publisher = Some(load_track_publisher(conn, &track_guid)?);
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
        source: item.source,
    }
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
        let Some(direction) = publisher_direction(item.medium.as_deref()) else {
            continue;
        };

        let remote_feed = db::get_feed(conn, &item.remote_feed_guid)?;
        let reciprocal = db::get_feed_remote_items_for_feed(conn, &item.remote_feed_guid)?
            .into_iter()
            .find(|candidate| {
                candidate.remote_feed_guid == current_feed.feed_guid
                    && candidate.medium.as_deref() == Some(expected_reciprocal_medium(direction))
            });

        let reciprocal_declared = reciprocal.is_some();
        let reciprocal_medium = reciprocal
            .as_ref()
            .and_then(|candidate| candidate.medium.clone());
        let two_way_validated = reciprocal_declared;

        let (publisher_feed_guid, publisher_feed_url, music_feed_guid, music_feed_url) =
            match direction {
                "music_to_publisher" => (
                    item.remote_feed_guid.clone(),
                    remote_feed
                        .as_ref()
                        .map(|feed| feed.feed_url.clone())
                        .or_else(|| item.remote_feed_url.clone()),
                    current_feed.feed_guid.clone(),
                    Some(current_feed.feed_url.clone()),
                ),
                "publisher_to_music" => (
                    current_feed.feed_guid.clone(),
                    Some(current_feed.feed_url.clone()),
                    item.remote_feed_guid.clone(),
                    remote_feed
                        .as_ref()
                        .map(|feed| feed.feed_url.clone())
                        .or_else(|| item.remote_feed_url.clone()),
                ),
                _ => continue,
            };

        rows.push(PublisherResponse {
            direction: direction.to_string(),
            remote_feed_guid: item.remote_feed_guid,
            remote_feed_url: item.remote_feed_url,
            remote_feed_medium: remote_feed
                .as_ref()
                .and_then(|feed| feed.raw_medium.clone()),
            publisher_feed_guid,
            publisher_feed_url,
            music_feed_guid,
            music_feed_url,
            reciprocal_declared,
            reciprocal_medium,
            two_way_validated,
        });
    }

    Ok(rows)
}

fn load_track_remote_items(
    conn: &rusqlite::Connection,
    track_guid: &str,
) -> Result<Vec<TrackRemoteItemResponse>, api::ApiError> {
    let items = db::get_track_remote_items_for_track(conn, track_guid)?;
    Ok(items
        .into_iter()
        .map(|item| TrackRemoteItemResponse {
            position: item.position,
            medium: item.medium,
            remote_feed_guid: item.remote_feed_guid,
            remote_feed_url: item.remote_feed_url,
            source: item.source,
        })
        .collect())
}

fn load_track_publisher(
    conn: &rusqlite::Connection,
    track_guid: &str,
) -> Result<Vec<PublisherResponse>, api::ApiError> {
    let Some(current_track) = db::get_track_by_guid(conn, track_guid)? else {
        return Ok(Vec::new());
    };
    let remote_items = db::get_track_remote_items_for_track(conn, track_guid)?;
    let mut rows = Vec::new();

    for item in remote_items {
        let Some(direction) = publisher_direction(item.medium.as_deref()) else {
            continue;
        };

        // For item-level, the "music feed" is the track's parent feed
        let current_feed =
            db::get_feed(conn, &current_track.feed_guid)?.ok_or_else(|| api::ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "orphaned track".into(),
                www_authenticate: None,
            })?;

        let remote_feed = db::get_feed(conn, &item.remote_feed_guid)?;
        let reciprocal = db::get_feed_remote_items_for_feed(conn, &item.remote_feed_guid)?
            .into_iter()
            .find(|candidate| {
                candidate.remote_feed_guid == current_track.feed_guid
                    && candidate.medium.as_deref() == Some(expected_reciprocal_medium(direction))
            });

        let reciprocal_declared = reciprocal.is_some();
        let reciprocal_medium = reciprocal
            .as_ref()
            .and_then(|candidate| candidate.medium.clone());
        let two_way_validated = reciprocal_declared;

        let (publisher_feed_guid, publisher_feed_url, music_feed_guid, music_feed_url) =
            match direction {
                "music_to_publisher" => (
                    item.remote_feed_guid.clone(),
                    remote_feed
                        .as_ref()
                        .map(|feed| feed.feed_url.clone())
                        .or_else(|| item.remote_feed_url.clone()),
                    current_feed.feed_guid.clone(),
                    Some(current_feed.feed_url.clone()),
                ),
                "publisher_to_music" => (
                    current_feed.feed_guid.clone(),
                    Some(current_feed.feed_url.clone()),
                    item.remote_feed_guid.clone(),
                    remote_feed
                        .as_ref()
                        .map(|feed| feed.feed_url.clone())
                        .or_else(|| item.remote_feed_url.clone()),
                ),
                _ => continue,
            };

        rows.push(PublisherResponse {
            direction: direction.to_string(),
            remote_feed_guid: item.remote_feed_guid,
            remote_feed_url: item.remote_feed_url,
            remote_feed_medium: remote_feed
                .as_ref()
                .and_then(|feed| feed.raw_medium.clone()),
            publisher_feed_guid,
            publisher_feed_url,
            music_feed_guid,
            music_feed_url,
            reciprocal_declared,
            reciprocal_medium,
            two_way_validated,
        });
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

fn expected_reciprocal_medium(direction: &str) -> &'static str {
    match direction {
        "music_to_publisher" => "music",
        "publisher_to_music" => "publisher",
        _ => unreachable!("unexpected publisher direction"),
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
                "SELECT t.track_guid, t.feed_guid, t.title, t.publisher, t.track_artist, t.track_artist_sort, \
             t.pub_date, t.duration_secs, COALESCE(t.image_url, f.image_url), t.language, \
             t.enclosure_url, t.enclosure_type, t.enclosure_bytes, t.track_number, t.season, \
             t.explicit, t.description, t.created_at, t.updated_at, COALESCE(f.title, ''), \
             f.release_artist \
             FROM tracks t LEFT JOIN feeds f ON f.feed_guid = t.feed_guid \
             WHERE t.track_guid = ?1",
                params![track_guid],
                |row| {
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
                    })
                },
            )
            .map_err(|_err| api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: "track not found".into(),
                www_authenticate: None,
            })?;

        let resp = build_track_response(&conn, row, &params)?;

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
        let medium = params.medium.as_deref().unwrap_or("music");

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
                 created_at, updated_at \
                 FROM feeds \
                 WHERE lower(raw_medium) = lower(?1)
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
                    })
                },
            )?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT feed_guid, feed_url, title, raw_medium, release_artist, \
                 release_artist_sort, release_date, release_kind, description, image_url, publisher, language, explicit, \
                 episode_count, newest_item_at, oldest_item_at, \
                 created_at, updated_at \
                 FROM feeds \
                 WHERE lower(raw_medium) = lower(?1) \
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
            feeds.push(FeedResponse {
                feed_guid: r.feed_guid,
                feed_url: r.feed_url,
                title: r.title,
                raw_medium: r.raw_medium,
                release_artist: r.release_artist,
                release_artist_sort: r.release_artist_sort,
                release_date: r.release_date,
                release_kind: r.release_kind,
                description: r.description,
                image_url: r.image_url,
                publisher_text: r.publisher_text,
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
    let data: Vec<serde_json::Value> = results
        .iter()
        .take(usize::try_from(limit).unwrap_or(0))
        .map(|r| {
            serde_json::json!({
                "entity_type": r.entity_type,
                "entity_id": r.entity_id,
                "rank": r.rank,
                "quality_score": r.quality_score,
            })
        })
        .collect();

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

#[derive(Debug, Serialize)]
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
        entity_types: vec!["feed", "track", "wallet"],
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

// ── GET /v1/wallets/{id} ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct WalletResponse {
    wallet_id: String,
    display_name: String,
    wallet_class: String,
    class_confidence: String,
    endpoints: Vec<WalletEndpointResponse>,
    aliases: Vec<WalletAliasResponse>,
    artist_links: Vec<WalletArtistLinkResponse>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
struct WalletEndpointResponse {
    id: i64,
    route_type: String,
    normalized_address: String,
    custom_key: String,
    custom_value: String,
}

#[derive(Debug, Serialize)]
struct WalletAliasResponse {
    alias: String,
    first_seen_at: i64,
    last_seen_at: i64,
}

#[derive(Debug, Serialize)]
struct WalletArtistLinkResponse {
    artist_id: String,
    artist_name: Option<String>,
    confidence: String,
    evidence_entity_type: String,
    evidence_entity_id: String,
    evidence_explanation: String,
}

async fn handle_get_wallet(
    State(state): State<Arc<api::AppState>>,
    Path(wallet_id): Path<String>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        // Follow redirect chain
        let resolved_id: String = conn
            .query_row(
                "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
                params![wallet_id],
                |row| row.get(0),
            )
            .unwrap_or(wallet_id);

        let wallet = conn
            .query_row(
                "SELECT wallet_id, display_name, wallet_class, class_confidence, created_at, updated_at \
                 FROM wallets WHERE wallet_id = ?1",
                params![resolved_id],
                |row| {
                    Ok(WalletResponse {
                        wallet_id: row.get(0)?,
                        display_name: row.get(1)?,
                        wallet_class: row.get(2)?,
                        class_confidence: row.get(3)?,
                        endpoints: Vec::new(),
                        aliases: Vec::new(),
                        artist_links: Vec::new(),
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .map_err(|_err| api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: "wallet not found".into(),
                www_authenticate: None,
            })?;

        let mut wallet = wallet;

        // Endpoints
        {
            let mut stmt = conn.prepare(
                "SELECT id, route_type, normalized_address, custom_key, custom_value \
                 FROM wallet_endpoints WHERE wallet_id = ?1 ORDER BY id",
            )?;
            wallet.endpoints = stmt
                .query_map(params![resolved_id], |row| {
                    Ok(WalletEndpointResponse {
                        id: row.get(0)?,
                        route_type: row.get(1)?,
                        normalized_address: row.get(2)?,
                        custom_key: row.get(3)?,
                        custom_value: row.get(4)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
        }

        // Aliases (across all endpoints)
        {
            let mut stmt = conn.prepare(
                "SELECT wa.alias, wa.first_seen_at, wa.last_seen_at \
                 FROM wallet_aliases wa \
                 JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
                 WHERE we.wallet_id = ?1 \
                 ORDER BY wa.first_seen_at ASC, wa.alias_lower ASC",
            )?;
            wallet.aliases = stmt
                .query_map(params![resolved_id], |row| {
                    Ok(WalletAliasResponse {
                        alias: row.get(0)?,
                        first_seen_at: row.get(1)?,
                        last_seen_at: row.get(2)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
        }

        // Artist links
        {
            let mut stmt = conn.prepare(
                "SELECT wal.artist_id, a.name, wal.confidence, \
                 wal.evidence_entity_type, wal.evidence_entity_id \
                 FROM wallet_artist_links wal \
                 LEFT JOIN artists a ON a.artist_id = wal.artist_id \
                 WHERE wal.wallet_id = ?1 \
                 ORDER BY wal.artist_id",
            )?;
            wallet.artist_links = stmt
                .query_map(params![resolved_id], |row| {
                    let evidence_entity_type: String = row.get(3)?;
                    Ok(WalletArtistLinkResponse {
                        artist_id: row.get(0)?,
                        artist_name: row.get(1)?,
                        confidence: row.get(2)?,
                        evidence_entity_type: evidence_entity_type.clone(),
                        evidence_entity_id: row.get(4)?,
                        evidence_explanation:
                            db::wallet_artist_link_explanation(&evidence_entity_type).to_string(),
                    })
                })?
                .collect::<Result<_, _>>()?;
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: wallet,
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
             t.duration_secs, t.track_number \
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

// ── Router builder ──────────────────────────────────────────────────────────

use axum::routing::get;

pub fn query_routes() -> axum::Router<Arc<api::AppState>> {
    axum::Router::new()
        .route("/v1/feeds/{guid}", get(handle_get_feed))
        .route("/v1/feeds/recent", get(handle_get_recent_feeds))
        .route("/v1/tracks/{guid}", get(handle_get_track))
        .route("/v1/wallets/{id}", get(handle_get_wallet))
        .route("/v1/search", get(handle_search))
        .route("/v1/node/capabilities", get(handle_capabilities))
        .route("/v1/peers", get(handle_get_peers))
        .route("/v1/publishers", get(handle_publisher_search))
        .route("/v1/publishers/{publisher}", get(handle_publisher_detail))
}
