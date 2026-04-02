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
use rusqlite::{OptionalExtension, params};
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

    fn feed_medium(&self) -> &str {
        self.medium.as_deref().unwrap_or(crate::medium::MUSIC)
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

// ── Serializable types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ArtistResponse {
    artist_id: String,
    name: String,
    sort_name: Option<String>,
    area: Option<String>,
    img_url: Option<String>,
    url: Option<String>,
    begin_year: Option<i64>,
    end_year: Option<i64>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    aliases: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credits: Option<Vec<CreditResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relationships: Option<Vec<RelResponse>>,
}

#[derive(Debug, Serialize)]
struct RelResponse {
    artist_id_a: String,
    artist_id_b: String,
    role: String,
    begin_year: Option<i64>,
    end_year: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct CreditResponse {
    id: i64,
    display_name: String,
    names: Vec<CreditNameResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct CreditNameResponse {
    artist_id: String,
    position: i64,
    name: String,
    join_phrase: String,
}

#[derive(Debug, Serialize)]
struct FeedResponse {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    artist_credit: CreditResponse,
    description: Option<String>,
    image_url: Option<String>,
    language: Option<String>,
    explicit: bool,
    episode_count: i64,
    newest_item_at: Option<i64>,
    oldest_item_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracks: Option<Vec<TrackSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payment_routes: Option<Vec<RouteResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical: Option<CanonicalReleaseRef>,
}

#[derive(Debug, Serialize)]
struct TrackSummary {
    track_guid: String,
    title: String,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
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
    artist_credit: CreditResponse,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    enclosure_bytes: Option<i64>,
    track_number: Option<i64>,
    season: Option<i64>,
    explicit: bool,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    payment_routes: Option<Vec<RouteResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_time_splits: Option<Vec<VtsResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
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
    canonical: Option<CanonicalRecordingRef>,
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
struct ExternalIdResponse {
    scheme: String,
    value: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    artist_signal: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReleaseResolutionSourceResponse {
    feed_guid: String,
    feed_url: String,
    title: String,
    match_type: String,
    confidence: i64,
    source_ids: Vec<SourceEntityIdResponse>,
    source_links: Vec<SourceEntityLinkResponse>,
    source_platforms: Vec<SourcePlatformClaimResponse>,
    source_release_claims: Vec<SourceReleaseClaimResponse>,
    remote_items: Vec<FeedRemoteItemResponse>,
}

#[derive(Debug, Serialize)]
struct ReleaseResolutionResponse {
    release_id: String,
    title: String,
    artist_credit: CreditResponse,
    sources: Vec<ReleaseResolutionSourceResponse>,
}

#[derive(Debug, Serialize)]
struct RecordingResolutionSourceResponse {
    track_guid: String,
    feed_guid: String,
    title: String,
    match_type: String,
    confidence: i64,
    source_ids: Vec<SourceEntityIdResponse>,
    source_links: Vec<SourceEntityLinkResponse>,
    source_contributors: Vec<SourceContributorClaimResponse>,
    source_release_claims: Vec<SourceReleaseClaimResponse>,
    source_enclosures: Vec<SourceItemEnclosureResponse>,
}

#[derive(Debug, Serialize)]
struct RecordingResolutionResponse {
    recording_id: String,
    title: String,
    artist_credit: CreditResponse,
    releases: Vec<RecordingReleaseSummary>,
    sources: Vec<RecordingResolutionSourceResponse>,
}

#[derive(Debug, Serialize)]
struct ArtistResolutionFeedEvidenceResponse {
    feed_guid: String,
    feed_url: String,
    title: String,
    canonical_release: Option<CanonicalReleaseRef>,
    source_ids: Vec<SourceEntityIdResponse>,
    source_links: Vec<SourceEntityLinkResponse>,
    source_platforms: Vec<SourcePlatformClaimResponse>,
    remote_items: Vec<FeedRemoteItemResponse>,
}

#[derive(Debug, Serialize)]
struct ArtistResolutionTrackEvidenceResponse {
    track_guid: String,
    feed_guid: String,
    feed_title: String,
    title: String,
    artist_credit: CreditResponse,
    canonical_recording: Option<CanonicalRecordingRef>,
    source_ids: Vec<SourceEntityIdResponse>,
    source_links: Vec<SourceEntityLinkResponse>,
    source_contributors: Vec<SourceContributorClaimResponse>,
}

#[derive(Debug, Serialize)]
struct ArtistResolutionResponse {
    artist_id: String,
    name: String,
    external_ids: Vec<ExternalIdResponse>,
    redirected_from: Vec<String>,
    feeds: Vec<ArtistResolutionFeedEvidenceResponse>,
    tracks: Vec<ArtistResolutionTrackEvidenceResponse>,
}

#[derive(Debug, Serialize)]
struct CanonicalReleaseRef {
    release_id: String,
    match_type: String,
    confidence: i64,
}

#[derive(Debug, Serialize)]
struct CanonicalRecordingRef {
    recording_id: String,
    match_type: String,
    confidence: i64,
}

#[derive(Debug, Serialize)]
struct ReleaseTrackResponse {
    position: i64,
    recording_id: String,
    title: String,
    duration_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_track_guid: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReleaseSourceSummary {
    feed_guid: String,
    feed_url: String,
    title: String,
    match_type: String,
    confidence: i64,
    platforms: Vec<String>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RecordingSourceSummary {
    track_guid: String,
    feed_guid: String,
    title: String,
    match_type: String,
    confidence: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    primary_enclosure_url: Option<String>,
    enclosure_urls: Vec<String>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReleaseListItemResponse {
    release_id: String,
    title: String,
    artist_credit: CreditResponse,
    description: Option<String>,
    image_url: Option<String>,
    release_date: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
struct ReleaseResponse {
    release_id: String,
    title: String,
    artist_credit: CreditResponse,
    description: Option<String>,
    image_url: Option<String>,
    release_date: Option<i64>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracks: Option<Vec<ReleaseTrackResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sources: Option<Vec<ReleaseSourceSummary>>,
}

#[derive(Debug, Serialize)]
struct RecordingReleaseSummary {
    release_id: String,
    title: String,
    position: i64,
}

#[derive(Debug, Serialize)]
struct RecordingResponse {
    recording_id: String,
    title: String,
    artist_credit: CreditResponse,
    duration_secs: Option<i64>,
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    sources: Option<Vec<RecordingSourceSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    releases: Option<Vec<RecordingReleaseSummary>>,
}

/// Intermediate row type for track queries to avoid complex tuple types.
struct TrackRow {
    track_guid: String,
    feed_guid: String,
    credit_id: i64,
    title: String,
    pub_date: Option<i64>,
    duration_secs: Option<i64>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    enclosure_bytes: Option<i64>,
    track_number: Option<i64>,
    season: Option<i64>,
    explicit_int: i64,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
}

/// Intermediate row type for feed queries to avoid complex tuple types.
struct FeedRow {
    feed_guid: String,
    feed_url: String,
    title: String,
    raw_medium: Option<String>,
    credit_id: i64,
    description: Option<String>,
    image_url: Option<String>,
    language: Option<String>,
    explicit_int: i64,
    episode_count: i64,
    newest_item_at: Option<i64>,
    oldest_item_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

struct ReleaseRow {
    release_id: String,
    title: String,
    title_lower: String,
    credit_id: i64,
    description: Option<String>,
    image_url: Option<String>,
    release_date: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

struct RecentReleaseRow {
    release_id: String,
    title: String,
    credit_id: i64,
    description: Option<String>,
    image_url: Option<String>,
    release_date: Option<i64>,
    created_at: i64,
    updated_at: i64,
    recent_at: i64,
}

// ── GET /v1/artists/{id} ────────────────────────────────────────────────────

async fn handle_get_artist(
    State(state): State<Arc<api::AppState>>,
    Path(artist_id): Path<String>,
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

        // Check for redirect first.
        let resolved_id: String = conn
            .query_row(
                "SELECT new_artist_id FROM artist_id_redirect WHERE old_artist_id = ?1",
                params![artist_id],
                |row| row.get(0),
            )
            .unwrap_or(artist_id);

        let artist = conn
            .query_row(
                "SELECT artist_id, name, sort_name, area, img_url, url, begin_year, end_year, \
             created_at, updated_at FROM artists WHERE artist_id = ?1",
                params![resolved_id],
                |row| {
                    Ok(ArtistResponse {
                        artist_id: row.get(0)?,
                        name: row.get(1)?,
                        sort_name: row.get(2)?,
                        area: row.get(3)?,
                        img_url: row.get(4)?,
                        url: row.get(5)?,
                        begin_year: row.get(6)?,
                        end_year: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        aliases: None,
                        credits: None,
                        tags: None,
                        relationships: None,
                    })
                },
            )
            .map_err(|_err| api::ApiError {
                status: StatusCode::NOT_FOUND,
                message: "artist not found".into(),
                www_authenticate: None,
            })?;

        let mut artist = artist;

        if params.includes("aliases") {
            let mut stmt = conn.prepare(
                "SELECT alias_lower FROM artist_aliases WHERE artist_id = ?1 ORDER BY alias_lower",
            )?;
            let aliases: Vec<String> = stmt
                .query_map(params![resolved_id], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            artist.aliases = Some(aliases);
        }

        if params.includes("credits") {
            let credits = db::get_artist_credits_for_artist(&conn, &resolved_id)?;
            artist.credits = Some(
                credits
                    .into_iter()
                    .map(|c| CreditResponse {
                        id: c.id,
                        display_name: c.display_name,
                        names: c
                            .names
                            .into_iter()
                            .map(|n| CreditNameResponse {
                                artist_id: n.artist_id,
                                position: n.position,
                                name: n.name,
                                join_phrase: n.join_phrase,
                            })
                            .collect(),
                    })
                    .collect(),
            );
        }

        if params.includes("tags") {
            artist.tags = Some(load_tags(&conn, "artist", &resolved_id)?);
        }

        if params.includes("relationships") {
            let rels = db::get_artist_rels(&conn, &resolved_id)?;
            artist.relationships = Some(
                rels.into_iter()
                    .map(|r| RelResponse {
                        artist_id_a: r.artist_id_a,
                        artist_id_b: r.artist_id_b,
                        role: r.rel_type_name,
                        begin_year: r.begin_year,
                        end_year: r.end_year,
                    })
                    .collect(),
            );
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: artist,
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

// ── GET /v1/artists/{id}/feeds ──────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single paginated feed-list flow with batch credit loading"
)]
async fn handle_get_artist_feeds(
    State(state): State<Arc<api::AppState>>,
    Path(artist_id): Path<String>,
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
        let medium_filter = params.feed_medium().to_string();

        // Resolve redirect.
        let resolved_id: String = conn
            .query_row(
                "SELECT new_artist_id FROM artist_id_redirect WHERE old_artist_id = ?1",
                params![artist_id],
                |row| row.get(0),
            )
            .unwrap_or(artist_id);

        // Verify artist exists.
        conn.query_row(
            "SELECT 1 FROM artists WHERE artist_id = ?1",
            params![resolved_id],
            |_| Ok(()),
        )
        .map_err(|_err| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "artist not found".into(),
            www_authenticate: None,
        })?;

        // Issue-CURSOR-STABILITY — 2026-03-14
        // Cursor encodes (title_lower, feed_guid) for a unique tiebreaker.
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
            let cursor_title = parts[0];
            let cursor_guid = parts[1];

            let mut stmt = conn.prepare(
                "SELECT f.feed_guid, f.feed_url, f.title, f.raw_medium, f.artist_credit_id, \
                 f.description, f.image_url, f.language, f.explicit, \
                 f.episode_count, f.newest_item_at, f.oldest_item_at, \
                 f.created_at, f.updated_at \
                 FROM feeds f \
                 JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id \
                 WHERE acn.artist_id = ?1 \
                   AND lower(f.raw_medium) = lower(?2) \
                   AND (f.title_lower > ?3 OR (f.title_lower = ?3 AND f.feed_guid > ?4)) \
                 ORDER BY f.title_lower ASC, f.feed_guid ASC \
                 LIMIT ?5",
            )?;
            stmt.query_map(
                params![
                    resolved_id,
                    medium_filter,
                    cursor_title,
                    cursor_guid,
                    limit + 1
                ],
                |row| {
                    Ok(FeedRow {
                        feed_guid: row.get(0)?,
                        feed_url: row.get(1)?,
                        title: row.get(2)?,
                        raw_medium: row.get(3)?,
                        credit_id: row.get(4)?,
                        description: row.get(5)?,
                        image_url: row.get(6)?,
                        language: row.get(7)?,
                        explicit_int: row.get(8)?,
                        episode_count: row.get(9)?,
                        newest_item_at: row.get(10)?,
                        oldest_item_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
            )?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT f.feed_guid, f.feed_url, f.title, f.raw_medium, f.artist_credit_id, \
                 f.description, f.image_url, f.language, f.explicit, \
                 f.episode_count, f.newest_item_at, f.oldest_item_at, \
                 f.created_at, f.updated_at \
                 FROM feeds f \
                 JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id \
                 WHERE acn.artist_id = ?1 AND lower(f.raw_medium) = lower(?2) \
                 ORDER BY f.title_lower ASC, f.feed_guid ASC \
                 LIMIT ?3",
            )?;
            stmt.query_map(params![resolved_id, medium_filter, limit + 1], |row| {
                Ok(FeedRow {
                    feed_guid: row.get(0)?,
                    feed_url: row.get(1)?,
                    title: row.get(2)?,
                    raw_medium: row.get(3)?,
                    credit_id: row.get(4)?,
                    description: row.get(5)?,
                    image_url: row.get(6)?,
                    language: row.get(7)?,
                    explicit_int: row.get(8)?,
                    episode_count: row.get(9)?,
                    newest_item_at: row.get(10)?,
                    oldest_item_at: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
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
                .map(|r| encode_cursor(&format!("{}\0{}", r.title.to_lowercase(), r.feed_guid)))
        } else {
            None
        };

        // Issue-6 batch credits — 2026-03-13
        // Batch-load all credits in two queries instead of 2*N.
        let credit_map = load_credits_for_feeds(&conn, &items)?;

        let mut feeds = Vec::with_capacity(items.len());
        for r in items {
            let credit = credit_map
                .get(&r.credit_id)
                .cloned()
                .map_or_else(|| load_credit(&conn, r.credit_id), Ok)?;
            feeds.push(FeedResponse {
                feed_guid: r.feed_guid,
                feed_url: r.feed_url,
                title: r.title,
                raw_medium: r.raw_medium,
                artist_credit: credit,
                description: r.description,
                image_url: r.image_url,
                language: r.language,
                explicit: r.explicit_int != 0,
                episode_count: r.episode_count,
                newest_item_at: r.newest_item_at,
                oldest_item_at: r.oldest_item_at,
                created_at: r.created_at,
                updated_at: r.updated_at,
                tracks: None,
                payment_routes: None,
                tags: None,
                source_links: None,
                source_ids: None,
                source_contributors: None,
                source_platforms: None,
                source_release_claims: None,
                remote_items: None,
                publisher: None,
                canonical: None,
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
                "SELECT feed_guid, feed_url, title, raw_medium, artist_credit_id, description, image_url, \
             language, explicit, episode_count, newest_item_at, oldest_item_at, \
             created_at, updated_at \
             FROM feeds WHERE feed_guid = ?1",
                params![feed_guid],
                |row| {
                    Ok(FeedRow {
                        feed_guid: row.get(0)?,
                        feed_url: row.get(1)?,
                        title: row.get(2)?,
                        raw_medium: row.get(3)?,
                        credit_id: row.get(4)?,
                        description: row.get(5)?,
                        image_url: row.get(6)?,
                        language: row.get(7)?,
                        explicit_int: row.get(8)?,
                        episode_count: row.get(9)?,
                        newest_item_at: row.get(10)?,
                        oldest_item_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
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

// ── Shared helpers ──────────────────────────────────────────────────────────

fn load_credit(
    conn: &rusqlite::Connection,
    credit_id: i64,
) -> Result<CreditResponse, api::ApiError> {
    let (id, display_name): (i64, String) = conn
        .query_row(
            "SELECT id, display_name FROM artist_credit WHERE id = ?1",
            params![credit_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_err| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "missing artist credit".into(),
            www_authenticate: None,
        })?;

    let mut stmt = conn
        .prepare(
            "SELECT artist_id, position, name, join_phrase \
         FROM artist_credit_name WHERE artist_credit_id = ?1 ORDER BY position",
        )
        .map_err(db::DbError::from)?;
    let names: Vec<CreditNameResponse> = stmt
        .query_map(params![id], |row| {
            Ok(CreditNameResponse {
                artist_id: row.get(0)?,
                position: row.get(1)?,
                name: row.get(2)?,
                join_phrase: row.get(3)?,
            })
        })
        .map_err(db::DbError::from)?
        .collect::<Result<_, _>>()
        .map_err(db::DbError::from)?;

    Ok(CreditResponse {
        id,
        display_name,
        names,
    })
}

// Issue-6 batch credits — 2026-03-13
/// Converts a model `ArtistCredit` to the query-local `CreditResponse`.
fn credit_from_model(ac: &crate::model::ArtistCredit) -> CreditResponse {
    CreditResponse {
        id: ac.id,
        display_name: ac.display_name.clone(),
        names: ac
            .names
            .iter()
            .map(|n| CreditNameResponse {
                artist_id: n.artist_id.clone(),
                position: n.position,
                name: n.name.clone(),
                join_phrase: n.join_phrase.clone(),
            })
            .collect(),
    }
}

// Issue-6 batch credits — 2026-03-13
/// Batch-loads credits for a set of `FeedRow` items, returning a `HashMap`
/// of `credit_id -> CreditResponse` for O(1) lookup. Falls back to the
/// single-load path for any credit IDs missing from the batch result.
fn load_credits_for_feeds(
    conn: &rusqlite::Connection,
    items: &[FeedRow],
) -> Result<HashMap<i64, CreditResponse>, api::ApiError> {
    let credit_ids: Vec<i64> = items.iter().map(|r| r.credit_id).collect();
    let batch = db::load_credits_batch(conn, &credit_ids)?;
    Ok(batch
        .into_iter()
        .map(|(id, ac)| (id, credit_from_model(&ac)))
        .collect())
}

fn load_tags(
    conn: &rusqlite::Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<String>, api::ApiError> {
    let sql = match entity_type {
        "artist" => {
            "SELECT t.name FROM tags t JOIN artist_tag at ON at.tag_id = t.id WHERE at.artist_id = ?1 ORDER BY t.name"
        }
        "feed" => {
            "SELECT t.name FROM tags t JOIN feed_tag ft ON ft.tag_id = t.id WHERE ft.feed_guid = ?1 ORDER BY t.name"
        }
        "track" => {
            "SELECT t.name FROM tags t JOIN track_tag tt ON tt.tag_id = t.id WHERE tt.track_guid = ?1 ORDER BY t.name"
        }
        _ => return Ok(Vec::new()),
    };
    let mut stmt = conn.prepare(sql)?;
    let tags: Vec<String> = stmt
        .query_map(params![entity_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(tags)
}

fn load_feed_platform_keys(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<Vec<String>, api::ApiError> {
    Ok(db::get_source_platform_claims_for_feed(conn, feed_guid)?
        .into_iter()
        .map(|claim| claim.platform_key)
        .collect())
}

fn load_entity_link_urls(
    conn: &rusqlite::Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<String>, api::ApiError> {
    Ok(
        db::get_source_entity_links_for_entity(conn, entity_type, entity_id)?
            .into_iter()
            .map(|link| link.url)
            .collect(),
    )
}

fn build_feed_response(
    conn: &rusqlite::Connection,
    row: FeedRow,
    params: &ListQuery,
) -> Result<FeedResponse, api::ApiError> {
    let feed_guid = row.feed_guid.clone();
    let credit = load_credit(conn, row.credit_id)?;

    let mut resp = FeedResponse {
        feed_guid: row.feed_guid,
        feed_url: row.feed_url,
        title: row.title,
        raw_medium: row.raw_medium,
        artist_credit: credit,
        description: row.description,
        image_url: row.image_url,
        language: row.language,
        explicit: row.explicit_int != 0,
        episode_count: row.episode_count,
        newest_item_at: row.newest_item_at,
        oldest_item_at: row.oldest_item_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        tracks: None,
        payment_routes: None,
        tags: None,
        source_links: None,
        source_ids: None,
        source_contributors: None,
        source_platforms: None,
        source_release_claims: None,
        remote_items: None,
        publisher: None,
        canonical: None,
    };

    if params.includes("tracks") {
        let mut stmt = conn.prepare(
            "SELECT track_guid, title, pub_date, duration_secs \
             FROM tracks WHERE feed_guid = ?1 ORDER BY pub_date DESC",
        )?;
        let tracks: Vec<TrackSummary> = stmt
            .query_map(params![feed_guid], |row| {
                Ok(TrackSummary {
                    track_guid: row.get(0)?,
                    title: row.get(1)?,
                    pub_date: row.get(2)?,
                    duration_secs: row.get(3)?,
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

    if params.includes("tags") {
        resp.tags = Some(load_tags(conn, "feed", &feed_guid)?);
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

    if params.includes("canonical") {
        resp.canonical = conn
            .query_row(
                "SELECT release_id, match_type, confidence \
                 FROM source_feed_release_map WHERE feed_guid = ?1",
                params![feed_guid],
                |row| {
                    Ok(CanonicalReleaseRef {
                        release_id: row.get(0)?,
                        match_type: row.get(1)?,
                        confidence: row.get(2)?,
                    })
                },
            )
            .optional()?;
    }

    Ok(resp)
}

fn build_track_response(
    conn: &rusqlite::Connection,
    row: TrackRow,
    params: &ListQuery,
) -> Result<TrackResponse, api::ApiError> {
    let track_guid = row.track_guid.clone();
    let credit = load_credit(conn, row.credit_id)?;

    let mut resp = TrackResponse {
        track_guid: row.track_guid,
        feed_guid: row.feed_guid,
        title: row.title,
        artist_credit: credit,
        pub_date: row.pub_date,
        duration_secs: row.duration_secs,
        enclosure_url: row.enclosure_url,
        enclosure_type: row.enclosure_type,
        enclosure_bytes: row.enclosure_bytes,
        track_number: row.track_number,
        season: row.season,
        explicit: row.explicit_int != 0,
        description: row.description,
        created_at: row.created_at,
        updated_at: row.updated_at,
        payment_routes: None,
        value_time_splits: None,
        tags: None,
        source_links: None,
        source_ids: None,
        source_contributors: None,
        source_release_claims: None,
        source_enclosures: None,
        canonical: None,
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

    if params.includes("tags") {
        resp.tags = Some(load_tags(conn, "track", &track_guid)?);
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
        let claims = db::get_source_contributor_claims_for_entity(conn, "track", &track_guid)?;
        // Feed→track inheritance: fall back to parent feed contributors when
        // the track has none of its own.
        let claims = if claims.is_empty() {
            db::get_source_contributor_claims_for_entity(conn, "feed", &resp.feed_guid)?
        } else {
            claims
        };
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

    if params.includes("canonical") {
        resp.canonical = conn
            .query_row(
                "SELECT recording_id, match_type, confidence \
                 FROM source_item_recording_map WHERE track_guid = ?1",
                params![track_guid],
                |row| {
                    Ok(CanonicalRecordingRef {
                        recording_id: row.get(0)?,
                        match_type: row.get(1)?,
                        confidence: row.get(2)?,
                    })
                },
            )
            .optional()?;
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

fn external_id_response(id: crate::db::ExternalIdRow) -> ExternalIdResponse {
    ExternalIdResponse {
        scheme: id.scheme,
        value: id.value,
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

        let artist_signal = (two_way_validated
            && publisher_music_pair_has_confirmed_artist(
                conn,
                &publisher_feed_guid,
                &music_feed_guid,
            )?)
        .then(|| "confirmed_artist".to_string());

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
            artist_signal,
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

fn publisher_music_pair_has_confirmed_artist(
    conn: &rusqlite::Connection,
    publisher_feed_guid: &str,
    music_feed_guid: &str,
) -> Result<bool, api::ApiError> {
    let publisher_artist_id = single_feed_artist_id(conn, publisher_feed_guid)?;
    let music_artist_id = single_feed_artist_id(conn, music_feed_guid)?;
    Ok(matches!(
        (publisher_artist_id.as_deref(), music_artist_id.as_deref()),
        (Some(left), Some(right)) if left == right
    ))
}

fn single_feed_artist_id(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<Option<String>, api::ApiError> {
    let mut stmt = conn.prepare(
        "SELECT acn.artist_id
         FROM feeds f
         JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id
         WHERE f.feed_guid = ?1
         ORDER BY acn.position, acn.artist_id",
    )?;
    let artist_ids = stmt
        .query_map(params![feed_guid], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if artist_ids.len() == 1 {
        Ok(artist_ids.into_iter().next())
    } else {
        Ok(None)
    }
}

// ── GET /v1/releases/{id} ──────────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single canonical detail flow with optional expansions"
)]
async fn handle_get_release(
    State(state): State<Arc<api::AppState>>,
    Path(release_id): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let release = db::get_release(&conn, &release_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "release not found".into(),
            www_authenticate: None,
        })?;

        let credit = load_credit(&conn, release.artist_credit_id)?;
        let mut resp = ReleaseResponse {
            release_id: release.release_id.clone(),
            title: release.title,
            artist_credit: credit,
            description: release.description,
            image_url: release.image_url,
            release_date: release.release_date,
            created_at: release.created_at,
            updated_at: release.updated_at,
            tracks: None,
            sources: None,
        };

        if params.includes("tracks") {
            let mut items = Vec::new();
            for rel in db::get_release_recordings(&conn, &release_id)? {
                let recording =
                    db::get_recording(&conn, &rel.recording_id)?.ok_or_else(|| api::ApiError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: "release references missing recording".into(),
                        www_authenticate: None,
                    })?;
                items.push(ReleaseTrackResponse {
                    position: rel.position,
                    recording_id: rel.recording_id,
                    title: recording.title,
                    duration_secs: recording.duration_secs,
                    source_track_guid: rel.source_track_guid,
                });
            }
            resp.tracks = Some(items);
        }

        if params.includes("sources") {
            let mut items = Vec::new();
            for map in db::get_source_feed_release_maps_for_release(&conn, &release_id)? {
                let feed = db::get_feed(&conn, &map.feed_guid)?.ok_or_else(|| api::ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "release source references missing feed".into(),
                    www_authenticate: None,
                })?;
                items.push(ReleaseSourceSummary {
                    feed_guid: feed.feed_guid,
                    feed_url: feed.feed_url,
                    title: feed.title,
                    match_type: map.match_type,
                    confidence: map.confidence,
                    platforms: load_feed_platform_keys(&conn, &map.feed_guid)?,
                    links: load_entity_link_urls(&conn, "feed", &map.feed_guid)?,
                });
            }
            resp.sources = Some(items);
        }

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

// ── GET /v1/releases/{id}/resolution ───────────────────────────────────────

async fn handle_get_release_resolution(
    State(state): State<Arc<api::AppState>>,
    Path(release_id): Path<String>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let release = db::get_release(&conn, &release_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "release not found".into(),
            www_authenticate: None,
        })?;

        let artist_credit = load_credit(&conn, release.artist_credit_id)?;
        let mut sources = Vec::new();
        for map in db::get_source_feed_release_maps_for_release(&conn, &release_id)? {
            let feed = db::get_feed(&conn, &map.feed_guid)?.ok_or_else(|| api::ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "release source references missing feed".into(),
                www_authenticate: None,
            })?;
            sources.push(ReleaseResolutionSourceResponse {
                feed_guid: feed.feed_guid.clone(),
                feed_url: feed.feed_url,
                title: feed.title,
                match_type: map.match_type,
                confidence: map.confidence,
                source_ids: db::get_source_entity_ids_for_entity(&conn, "feed", &feed.feed_guid)?
                    .into_iter()
                    .map(entity_id_response)
                    .collect(),
                source_links: db::get_source_entity_links_for_entity(
                    &conn,
                    "feed",
                    &feed.feed_guid,
                )?
                .into_iter()
                .map(entity_link_response)
                .collect(),
                source_platforms: db::get_source_platform_claims_for_feed(&conn, &feed.feed_guid)?
                    .into_iter()
                    .map(platform_claim_response)
                    .collect(),
                source_release_claims: db::get_source_release_claims_for_entity(
                    &conn,
                    "feed",
                    &feed.feed_guid,
                )?
                .into_iter()
                .map(release_claim_response)
                .collect(),
                remote_items: db::get_feed_remote_items_for_feed(&conn, &feed.feed_guid)?
                    .into_iter()
                    .map(feed_remote_item_response)
                    .collect(),
            });
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: ReleaseResolutionResponse {
                release_id: release.release_id,
                title: release.title,
                artist_credit,
                sources,
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

// ── GET /v1/releases/{id}/sources ──────────────────────────────────────────

async fn handle_get_release_sources(
    State(state): State<Arc<api::AppState>>,
    Path(release_id): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let _release = db::get_release(&conn, &release_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "release not found".into(),
            www_authenticate: None,
        })?;

        let mut feeds = Vec::new();
        for map in db::get_source_feed_release_maps_for_release(&conn, &release_id)? {
            let feed = db::get_feed(&conn, &map.feed_guid)?.ok_or_else(|| api::ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "release source references missing feed".into(),
                www_authenticate: None,
            })?;
            let row = FeedRow {
                feed_guid: feed.feed_guid,
                feed_url: feed.feed_url,
                title: feed.title,
                raw_medium: feed.raw_medium,
                credit_id: feed.artist_credit_id,
                description: feed.description,
                image_url: feed.image_url,
                language: feed.language,
                explicit_int: i64::from(feed.explicit),
                episode_count: feed.episode_count,
                newest_item_at: feed.newest_item_at,
                oldest_item_at: feed.oldest_item_at,
                created_at: feed.created_at,
                updated_at: feed.updated_at,
            };
            feeds.push(build_feed_response(&conn, row, &params)?);
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: feeds,
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

// ── GET /v1/recordings/{id} ────────────────────────────────────────────────

#[allow(
    clippy::too_many_lines,
    reason = "single canonical detail flow with optional expansions"
)]
async fn handle_get_recording(
    State(state): State<Arc<api::AppState>>,
    Path(recording_id): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let recording = db::get_recording(&conn, &recording_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "recording not found".into(),
            www_authenticate: None,
        })?;

        let credit = load_credit(&conn, recording.artist_credit_id)?;
        let mut resp = RecordingResponse {
            recording_id: recording.recording_id.clone(),
            title: recording.title,
            artist_credit: credit,
            duration_secs: recording.duration_secs,
            created_at: recording.created_at,
            updated_at: recording.updated_at,
            sources: None,
            releases: None,
        };

        if params.includes("sources") {
            let mut items = Vec::new();
            for map in db::get_source_item_recording_maps_for_recording(&conn, &recording_id)? {
                let track =
                    db::get_track(&conn, &map.track_guid)?.ok_or_else(|| api::ApiError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: "recording source references missing track".into(),
                        www_authenticate: None,
                    })?;
                let enclosure_urls =
                    db::get_source_item_enclosures_for_entity(&conn, "track", &map.track_guid)?
                        .into_iter()
                        .map(|enclosure| enclosure.url)
                        .collect::<Vec<_>>();
                items.push(RecordingSourceSummary {
                    track_guid: track.track_guid.clone(),
                    feed_guid: track.feed_guid.clone(),
                    title: track.title,
                    match_type: map.match_type,
                    confidence: map.confidence,
                    primary_enclosure_url: track.enclosure_url,
                    enclosure_urls,
                    links: load_entity_link_urls(&conn, "track", &map.track_guid)?,
                });
            }
            resp.sources = Some(items);
        }

        if params.includes("releases") {
            let mut stmt = conn.prepare(
                "SELECT r.release_id, r.title, rr.position \
                 FROM release_recordings rr \
                 JOIN releases r ON r.release_id = rr.release_id \
                 WHERE rr.recording_id = ?1 \
                 ORDER BY r.title_lower, r.release_id, rr.position",
            )?;
            let releases: Vec<RecordingReleaseSummary> = stmt
                .query_map(params![recording_id], |row| {
                    Ok(RecordingReleaseSummary {
                        release_id: row.get(0)?,
                        title: row.get(1)?,
                        position: row.get(2)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            resp.releases = Some(releases);
        }

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

// ── GET /v1/recordings/{id}/resolution ─────────────────────────────────────

async fn handle_get_recording_resolution(
    State(state): State<Arc<api::AppState>>,
    Path(recording_id): Path<String>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let recording = db::get_recording(&conn, &recording_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "recording not found".into(),
            www_authenticate: None,
        })?;
        let artist_credit = load_credit(&conn, recording.artist_credit_id)?;

        let mut stmt = conn.prepare(
            "SELECT r.release_id, r.title, rr.position \
             FROM release_recordings rr \
             JOIN releases r ON r.release_id = rr.release_id \
             WHERE rr.recording_id = ?1 \
             ORDER BY r.title_lower, r.release_id, rr.position",
        )?;
        let releases: Vec<RecordingReleaseSummary> = stmt
            .query_map(params![recording_id], |row| {
                Ok(RecordingReleaseSummary {
                    release_id: row.get(0)?,
                    title: row.get(1)?,
                    position: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut sources = Vec::new();
        for map in db::get_source_item_recording_maps_for_recording(&conn, &recording_id)? {
            let track = db::get_track(&conn, &map.track_guid)?.ok_or_else(|| api::ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "recording source references missing track".into(),
                www_authenticate: None,
            })?;
            sources.push(RecordingResolutionSourceResponse {
                track_guid: track.track_guid.clone(),
                feed_guid: track.feed_guid.clone(),
                title: track.title,
                match_type: map.match_type,
                confidence: map.confidence,
                source_ids: db::get_source_entity_ids_for_entity(
                    &conn,
                    "track",
                    &track.track_guid,
                )?
                .into_iter()
                .map(entity_id_response)
                .collect(),
                source_links: db::get_source_entity_links_for_entity(
                    &conn,
                    "track",
                    &track.track_guid,
                )?
                .into_iter()
                .map(entity_link_response)
                .collect(),
                source_contributors: db::get_source_contributor_claims_for_entity(
                    &conn,
                    "track",
                    &track.track_guid,
                )?
                .into_iter()
                .map(contributor_claim_response)
                .collect(),
                source_release_claims: db::get_source_release_claims_for_entity(
                    &conn,
                    "track",
                    &track.track_guid,
                )?
                .into_iter()
                .map(release_claim_response)
                .collect(),
                source_enclosures: db::get_source_item_enclosures_for_entity(
                    &conn,
                    "track",
                    &track.track_guid,
                )?
                .into_iter()
                .map(enclosure_response)
                .collect(),
            });
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: RecordingResolutionResponse {
                recording_id: recording.recording_id,
                title: recording.title,
                artist_credit,
                releases,
                sources,
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

// ── GET /v1/recordings/{id}/sources ────────────────────────────────────────

async fn handle_get_recording_sources(
    State(state): State<Arc<api::AppState>>,
    Path(recording_id): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let _recording = db::get_recording(&conn, &recording_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "recording not found".into(),
            www_authenticate: None,
        })?;

        let mut tracks = Vec::new();
        for map in db::get_source_item_recording_maps_for_recording(&conn, &recording_id)? {
            let track = db::get_track(&conn, &map.track_guid)?.ok_or_else(|| api::ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "recording source references missing track".into(),
                www_authenticate: None,
            })?;
            let row = TrackRow {
                track_guid: track.track_guid,
                feed_guid: track.feed_guid,
                credit_id: track.artist_credit_id,
                title: track.title,
                pub_date: track.pub_date,
                duration_secs: track.duration_secs,
                enclosure_url: track.enclosure_url,
                enclosure_type: track.enclosure_type,
                enclosure_bytes: track.enclosure_bytes,
                track_number: track.track_number,
                season: track.season,
                explicit_int: i64::from(track.explicit),
                description: track.description,
                created_at: track.created_at,
                updated_at: track.updated_at,
            };
            tracks.push(build_track_response(&conn, row, &params)?);
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: tracks,
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

// ── GET /v1/artists/{id}/releases ──────────────────────────────────────────

async fn handle_get_artist_releases(
    State(state): State<Arc<api::AppState>>,
    Path(artist_id): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let limit = params.capped_limit();

        let rows: Vec<ReleaseRow> = if let Some(ref cursor_str) = params.cursor {
            let decoded = decode_cursor(cursor_str)?;
            let parts: Vec<&str> = decoded.splitn(2, '\0').collect();
            if parts.len() != 2 {
                return Err(api::ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "invalid cursor format".into(),
                    www_authenticate: None,
                });
            }
            let cursor_title = parts[0];
            let cursor_release_id = parts[1];
            let mut stmt = conn.prepare(
                "SELECT r.release_id, r.title, r.title_lower, r.artist_credit_id, r.description, \
                 r.image_url, r.release_date, r.created_at, r.updated_at \
                 FROM releases r \
                 WHERE EXISTS(SELECT 1 FROM artist_credit_name acn \
                              WHERE acn.artist_credit_id = r.artist_credit_id AND acn.artist_id = ?1) \
                   AND (r.title_lower, r.release_id) > (?2, ?3) \
                 ORDER BY r.title_lower, r.release_id \
                 LIMIT ?4",
            )?;
            stmt.query_map(params![artist_id, cursor_title, cursor_release_id, limit + 1], |row| {
                Ok(ReleaseRow {
                    release_id: row.get(0)?,
                    title: row.get(1)?,
                    title_lower: row.get(2)?,
                    credit_id: row.get(3)?,
                    description: row.get(4)?,
                    image_url: row.get(5)?,
                    release_date: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT r.release_id, r.title, r.title_lower, r.artist_credit_id, r.description, \
                 r.image_url, r.release_date, r.created_at, r.updated_at \
                 FROM releases r \
                 WHERE EXISTS(SELECT 1 FROM artist_credit_name acn \
                              WHERE acn.artist_credit_id = r.artist_credit_id AND acn.artist_id = ?1) \
                 ORDER BY r.title_lower, r.release_id \
                 LIMIT ?2",
            )?;
            stmt.query_map(params![artist_id, limit + 1], |row| {
                Ok(ReleaseRow {
                    release_id: row.get(0)?,
                    title: row.get(1)?,
                    title_lower: row.get(2)?,
                    credit_id: row.get(3)?,
                    description: row.get(4)?,
                    image_url: row.get(5)?,
                    release_date: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
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
            items.last().map(|r| encode_cursor(&format!("{}\0{}", r.title_lower, r.release_id)))
        } else {
            None
        };

        let credit_ids: Vec<i64> = items.iter().map(|row| row.credit_id).collect();
        let credit_map = db::load_credits_batch(&conn, &credit_ids)?
            .into_iter()
            .map(|(id, ac)| (id, credit_from_model(&ac)))
            .collect::<HashMap<_, _>>();

        let releases = items
            .into_iter()
            .map(|row| {
                let credit = credit_map
                    .get(&row.credit_id)
                    .cloned()
                    .map_or_else(|| load_credit(&conn, row.credit_id), Ok)?;
                Ok::<_, api::ApiError>(ReleaseListItemResponse {
                    release_id: row.release_id,
                    title: row.title,
                    artist_credit: credit,
                    description: row.description,
                    image_url: row.image_url,
                    release_date: row.release_date,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok::<_, api::ApiError>(QueryResponse {
            data: releases,
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

// ── GET /v1/artists/{id}/resolution ────────────────────────────────────────

async fn handle_get_artist_resolution(
    State(state): State<Arc<api::AppState>>,
    Path(artist_id): Path<String>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;

        let artist = db::get_artist_by_id(&conn, &artist_id)?.ok_or_else(|| api::ApiError {
            status: StatusCode::NOT_FOUND,
            message: "artist not found".into(),
            www_authenticate: None,
        })?;

        let external_ids = db::get_external_ids(&conn, "artist", &artist_id)?
            .into_iter()
            .map(external_id_response)
            .collect();
        let redirected_from = {
            let mut stmt = conn.prepare(
                "SELECT old_artist_id FROM artist_id_redirect \
                 WHERE new_artist_id = ?1 ORDER BY old_artist_id",
            )?;
            stmt.query_map(params![artist_id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?
        };

        let mut stmt = conn.prepare(
            "SELECT DISTINCT f.feed_guid, f.feed_url, f.title \
             FROM artist_credit_name acn \
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
             JOIN feeds f ON f.artist_credit_id = ac.id \
             WHERE acn.artist_id = ?1 \
             ORDER BY f.title_lower, f.feed_guid",
        )?;
        let feed_rows: Vec<(String, String, String)> = stmt
            .query_map(params![artist_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<_, _>>()?;

        let mut feeds = Vec::new();
        for (feed_guid, feed_url, title) in feed_rows {
            let canonical_release = conn
                .query_row(
                    "SELECT release_id, match_type, confidence \
                     FROM source_feed_release_map WHERE feed_guid = ?1",
                    params![feed_guid],
                    |row| {
                        Ok(CanonicalReleaseRef {
                            release_id: row.get(0)?,
                            match_type: row.get(1)?,
                            confidence: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            feeds.push(ArtistResolutionFeedEvidenceResponse {
                feed_guid: feed_guid.clone(),
                feed_url,
                title,
                canonical_release,
                source_ids: db::get_source_entity_ids_for_entity(&conn, "feed", &feed_guid)?
                    .into_iter()
                    .map(entity_id_response)
                    .collect(),
                source_links: db::get_source_entity_links_for_entity(&conn, "feed", &feed_guid)?
                    .into_iter()
                    .map(entity_link_response)
                    .collect(),
                source_platforms: db::get_source_platform_claims_for_feed(&conn, &feed_guid)?
                    .into_iter()
                    .map(platform_claim_response)
                    .collect(),
                remote_items: db::get_feed_remote_items_for_feed(&conn, &feed_guid)?
                    .into_iter()
                    .map(feed_remote_item_response)
                    .collect(),
            });
        }

        let mut track_stmt = conn.prepare(
            "SELECT DISTINCT t.track_guid, t.feed_guid, f.title, t.title, t.artist_credit_id \
             FROM artist_credit_name acn \
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
             JOIN tracks t ON t.artist_credit_id = ac.id \
             JOIN feeds f ON f.feed_guid = t.feed_guid \
             WHERE acn.artist_id = ?1 \
             ORDER BY f.title_lower, f.feed_guid, t.pub_date, t.track_guid",
        )?;
        let track_rows: Vec<(String, String, String, String, i64)> = track_stmt
            .query_map(params![artist_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut tracks = Vec::new();
        for (track_guid, feed_guid, feed_title, title, credit_id) in track_rows {
            let canonical_recording = conn
                .query_row(
                    "SELECT recording_id, match_type, confidence \
                     FROM source_item_recording_map WHERE track_guid = ?1",
                    params![track_guid],
                    |row| {
                        Ok(CanonicalRecordingRef {
                            recording_id: row.get(0)?,
                            match_type: row.get(1)?,
                            confidence: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            tracks.push(ArtistResolutionTrackEvidenceResponse {
                track_guid: track_guid.clone(),
                feed_guid,
                feed_title,
                title,
                artist_credit: load_credit(&conn, credit_id)?,
                canonical_recording,
                source_ids: db::get_source_entity_ids_for_entity(&conn, "track", &track_guid)?
                    .into_iter()
                    .map(entity_id_response)
                    .collect(),
                source_links: db::get_source_entity_links_for_entity(&conn, "track", &track_guid)?
                    .into_iter()
                    .map(entity_link_response)
                    .collect(),
                source_contributors: db::get_source_contributor_claims_for_entity(
                    &conn,
                    "track",
                    &track_guid,
                )?
                .into_iter()
                .map(contributor_claim_response)
                .collect(),
            });
        }

        Ok::<_, api::ApiError>(QueryResponse {
            data: ArtistResolutionResponse {
                artist_id: artist.artist_id,
                name: artist.name,
                external_ids,
                redirected_from,
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
                "SELECT track_guid, feed_guid, artist_credit_id, title, pub_date, \
             duration_secs, enclosure_url, enclosure_type, enclosure_bytes, \
             track_number, season, explicit, description, created_at, updated_at \
             FROM tracks WHERE track_guid = ?1",
                params![track_guid],
                |row| {
                    Ok(TrackRow {
                        track_guid: row.get(0)?,
                        feed_guid: row.get(1)?,
                        credit_id: row.get(2)?,
                        title: row.get(3)?,
                        pub_date: row.get(4)?,
                        duration_secs: row.get(5)?,
                        enclosure_url: row.get(6)?,
                        enclosure_type: row.get(7)?,
                        enclosure_bytes: row.get(8)?,
                        track_number: row.get(9)?,
                        season: row.get(10)?,
                        explicit_int: row.get(11)?,
                        description: row.get(12)?,
                        created_at: row.get(13)?,
                        updated_at: row.get(14)?,
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
        let medium_filter = params.feed_medium().to_string();

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
                "SELECT feed_guid, feed_url, title, raw_medium, artist_credit_id, \
                 description, image_url, language, explicit, \
                 episode_count, newest_item_at, oldest_item_at, \
                 created_at, updated_at \
                 FROM feeds \
                 WHERE lower(raw_medium) = lower(?1)
                   AND (newest_item_at, feed_guid) < (?2, ?3) \
                 ORDER BY newest_item_at DESC, feed_guid DESC \
                 LIMIT ?4",
            )?;
            stmt.query_map(
                params![medium_filter, cursor_ts, cursor_guid, limit + 1],
                |row| {
                    Ok(FeedRow {
                        feed_guid: row.get(0)?,
                        feed_url: row.get(1)?,
                        title: row.get(2)?,
                        raw_medium: row.get(3)?,
                        credit_id: row.get(4)?,
                        description: row.get(5)?,
                        image_url: row.get(6)?,
                        language: row.get(7)?,
                        explicit_int: row.get(8)?,
                        episode_count: row.get(9)?,
                        newest_item_at: row.get(10)?,
                        oldest_item_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
            )?
            .collect::<Result<_, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT feed_guid, feed_url, title, raw_medium, artist_credit_id, \
                 description, image_url, language, explicit, \
                 episode_count, newest_item_at, oldest_item_at, \
                 created_at, updated_at \
                 FROM feeds \
                 WHERE lower(raw_medium) = lower(?1) \
                 ORDER BY newest_item_at DESC, feed_guid DESC \
                 LIMIT ?2",
            )?;
            stmt.query_map(params![medium_filter, limit + 1], |row| {
                Ok(FeedRow {
                    feed_guid: row.get(0)?,
                    feed_url: row.get(1)?,
                    title: row.get(2)?,
                    raw_medium: row.get(3)?,
                    credit_id: row.get(4)?,
                    description: row.get(5)?,
                    image_url: row.get(6)?,
                    language: row.get(7)?,
                    explicit_int: row.get(8)?,
                    episode_count: row.get(9)?,
                    newest_item_at: row.get(10)?,
                    oldest_item_at: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
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

        // Issue-6 batch credits — 2026-03-13
        // Batch-load all credits in two queries instead of 2*N.
        let credit_map = load_credits_for_feeds(&conn, &items)?;

        let mut feeds = Vec::with_capacity(items.len());
        for r in items {
            let credit = credit_map
                .get(&r.credit_id)
                .cloned()
                .map_or_else(|| load_credit(&conn, r.credit_id), Ok)?;
            feeds.push(FeedResponse {
                feed_guid: r.feed_guid,
                feed_url: r.feed_url,
                title: r.title,
                raw_medium: r.raw_medium,
                artist_credit: credit,
                description: r.description,
                image_url: r.image_url,
                language: r.language,
                explicit: r.explicit_int != 0,
                episode_count: r.episode_count,
                newest_item_at: r.newest_item_at,
                oldest_item_at: r.oldest_item_at,
                created_at: r.created_at,
                updated_at: r.updated_at,
                tracks: None,
                payment_routes: None,
                tags: None,
                source_links: None,
                source_ids: None,
                source_contributors: None,
                source_platforms: None,
                source_release_claims: None,
                remote_items: None,
                publisher: None,
                canonical: None,
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

// ── GET /v1/recent ──────────────────────────────────────────────────────────

async fn handle_get_recent(
    State(state): State<Arc<api::AppState>>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let limit = params.capped_limit();

        let base_sql =
            "SELECT release_id, title, title_lower, artist_credit_id, description, image_url, \
             release_date, created_at, updated_at, recent_at \
             FROM ( \
                 SELECT r.release_id, r.title, r.title_lower, r.artist_credit_id, r.description, \
                        r.image_url, r.release_date, r.created_at, r.updated_at, \
                        COALESCE(MAX(f.newest_item_at), r.release_date, r.created_at) AS recent_at \
                 FROM releases r \
                 LEFT JOIN source_feed_release_map sfr ON sfr.release_id = r.release_id \
                 LEFT JOIN feeds f ON f.feed_guid = sfr.feed_guid \
                 GROUP BY r.release_id, r.title, r.title_lower, r.artist_credit_id, \
                          r.description, r.image_url, r.release_date, r.created_at, r.updated_at \
             ) recent_releases";

        let rows: Vec<RecentReleaseRow> = if let Some(ref cursor_str) = params.cursor {
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
            let cursor_release_id = parts[1];
            let sql = format!(
                "{base_sql} \
                 WHERE (recent_at, release_id) < (?1, ?2) \
                 ORDER BY recent_at DESC, release_id DESC \
                 LIMIT ?3"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params![cursor_ts, cursor_release_id, limit + 1], |row| {
                Ok(RecentReleaseRow {
                    release_id: row.get(0)?,
                    title: row.get(1)?,
                    credit_id: row.get(3)?,
                    description: row.get(4)?,
                    image_url: row.get(5)?,
                    release_date: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    recent_at: row.get(9)?,
                })
            })?
            .collect::<Result<_, _>>()?
        } else {
            let sql = format!(
                "{base_sql} \
                 ORDER BY recent_at DESC, release_id DESC \
                 LIMIT ?1"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params![limit + 1], |row| {
                Ok(RecentReleaseRow {
                    release_id: row.get(0)?,
                    title: row.get(1)?,
                    credit_id: row.get(3)?,
                    description: row.get(4)?,
                    image_url: row.get(5)?,
                    release_date: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    recent_at: row.get(9)?,
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
            items.last().map(|r| encode_cursor(&format!("{}\0{}", r.recent_at, r.release_id)))
        } else {
            None
        };

        let credit_ids: Vec<i64> = items.iter().map(|row| row.credit_id).collect();
        let credit_map = db::load_credits_batch(&conn, &credit_ids)?
            .into_iter()
            .map(|(id, ac)| (id, credit_from_model(&ac)))
            .collect::<HashMap<_, _>>();

        let releases = items
            .into_iter()
            .map(|row| {
                let credit = credit_map
                    .get(&row.credit_id)
                    .cloned()
                    .map_or_else(|| load_credit(&conn, row.credit_id), Ok)?;
                Ok::<_, api::ApiError>(ReleaseListItemResponse {
                    release_id: row.release_id,
                    title: row.title,
                    artist_credit: credit,
                    description: row.description,
                    image_url: row.image_url,
                    release_date: row.release_date,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok::<_, api::ApiError>(QueryResponse {
            data: releases,
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
            Some("artist" | "release" | "recording" | "feed" | "track") => crate::search::search(
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
                for entity_type in ["artist", "release", "recording", "feed"] {
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

#[derive(Debug, Serialize)]
struct ResolverSourceLayerStatus {
    authoritative: bool,
    preserved: bool,
    immediate_endpoints: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ResolverQueueStatus {
    total: i64,
    ready: i64,
    locked: i64,
    failed: i64,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "resolver status intentionally exposes independent import and backfill pause-state flags"
)]
#[derive(Debug, Serialize)]
struct ResolverDerivedLayerStatus {
    caught_up: bool,
    import_active: bool,
    import_stale: bool,
    import_heartbeat_at: Option<i64>,
    backfill_active: bool,
    backfill_stale: bool,
    backfill_heartbeat_at: Option<i64>,
    queue: ResolverQueueStatus,
    resolver_backed_endpoints: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ResolverStatusResponse {
    api_version: &'static str,
    node_pubkey: String,
    source_layer: ResolverSourceLayerStatus,
    resolver: ResolverDerivedLayerStatus,
}

async fn handle_capabilities(State(state): State<Arc<api::AppState>>) -> impl IntoResponse {
    let mut include_params = HashMap::new();
    include_params.insert(
        "artist",
        vec!["aliases", "credits", "tags", "relationships"],
    );
    include_params.insert(
        "feed",
        vec![
            "tracks",
            "payment_routes",
            "tags",
            "source_links",
            "source_ids",
            "source_contributors",
            "source_platforms",
            "source_release_claims",
            "remote_items",
            "publisher",
            "canonical",
        ],
    );
    include_params.insert(
        "track",
        vec![
            "payment_routes",
            "value_time_splits",
            "tags",
            "source_links",
            "source_ids",
            "source_contributors",
            "source_release_claims",
            "source_enclosures",
            "canonical",
        ],
    );
    include_params.insert("release", vec!["tracks", "sources"]);
    include_params.insert("recording", vec!["sources", "releases"]);

    Json(CapabilitiesResponse {
        api_version: "v1",
        node_pubkey: state.node_pubkey_hex.clone(),
        capabilities: vec!["query", "search", "sync", "push"],
        entity_types: vec!["artist", "feed", "track", "release", "recording"],
        include_params,
    })
}

// ── GET /v1/resolver/status ────────────────────────────────────────────────

async fn handle_resolver_status(
    State(state): State<Arc<api::AppState>>,
) -> Result<impl IntoResponse, api::ApiError> {
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        let conn = state2.db.reader().map_err(|e| api::ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("database reader pool error: {e}"),
            www_authenticate: None,
        })?;
        let counts = db::get_resolver_queue_counts(&conn)?;
        let import_state = db::resolver_import_state(&conn)?;
        let backfill_state = db::resolver_backfill_state(&conn)?;
        Ok::<_, api::ApiError>(ResolverStatusResponse {
            api_version: "v1",
            node_pubkey: state2.node_pubkey_hex.clone(),
            source_layer: ResolverSourceLayerStatus {
                authoritative: true,
                preserved: true,
                immediate_endpoints: vec![
                    "/v1/feeds/{guid}",
                    "/v1/tracks/{guid}",
                    "/v1/feeds/recent",
                ],
            },
            resolver: ResolverDerivedLayerStatus {
                caught_up: counts.total == 0 && !import_state.active && !backfill_state.active,
                import_active: import_state.active,
                import_stale: import_state.stale,
                import_heartbeat_at: import_state.heartbeat_at,
                backfill_active: backfill_state.active,
                backfill_stale: backfill_state.stale,
                backfill_heartbeat_at: backfill_state.heartbeat_at,
                queue: ResolverQueueStatus {
                    total: counts.total,
                    ready: counts.ready,
                    locked: counts.locked,
                    failed: counts.failed,
                },
                resolver_backed_endpoints: vec![
                    "/v1/search",
                    "/v1/search?type=feed",
                    "/v1/search?type=track",
                    "/v1/recent",
                    "/v1/artists/{id}",
                    "/v1/artists/{id}/resolution",
                    "/v1/artists/{id}/releases",
                    "/v1/releases/{id}",
                    "/v1/releases/{id}/resolution",
                    "/v1/releases/{id}/sources",
                    "/v1/recordings/{id}",
                    "/v1/recordings/{id}/resolution",
                    "/v1/recordings/{id}/sources",
                ],
            },
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

// ── Router builder ──────────────────────────────────────────────────────────

use axum::routing::get;

pub fn query_routes() -> axum::Router<Arc<api::AppState>> {
    axum::Router::new()
        .route("/v1/artists/{id}", get(handle_get_artist))
        .route("/v1/artists/{id}/feeds", get(handle_get_artist_feeds))
        .route("/v1/artists/{id}/releases", get(handle_get_artist_releases))
        .route(
            "/v1/artists/{id}/resolution",
            get(handle_get_artist_resolution),
        )
        .route("/v1/feeds/{guid}", get(handle_get_feed))
        .route("/v1/feeds/recent", get(handle_get_recent_feeds))
        .route("/v1/releases/{id}", get(handle_get_release))
        .route(
            "/v1/releases/{id}/resolution",
            get(handle_get_release_resolution),
        )
        .route("/v1/releases/{id}/sources", get(handle_get_release_sources))
        .route("/v1/recordings/{id}", get(handle_get_recording))
        .route(
            "/v1/recordings/{id}/resolution",
            get(handle_get_recording_resolution),
        )
        .route(
            "/v1/recordings/{id}/sources",
            get(handle_get_recording_sources),
        )
        .route("/v1/tracks/{guid}", get(handle_get_track))
        .route("/v1/wallets/{id}", get(handle_get_wallet))
        .route("/v1/recent", get(handle_get_recent))
        .route("/v1/search", get(handle_search))
        .route("/v1/node/capabilities", get(handle_capabilities))
        .route("/v1/resolver/status", get(handle_resolver_status))
        .route("/v1/peers", get(handle_get_peers))
}
