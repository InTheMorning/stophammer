// Rust guideline compliant (M-APP-ERROR, M-MODULE-DOCS) — 2026-03-09

//! Crawler-facing request and response types for `POST /ingest/feed`.
//!
//! [`IngestFeedRequest`] carries the raw crawl result including the content
//! hash used for change detection, optional parsed [`IngestFeedData`], and the
//! crawl token that gates access. [`IngestResponse`] reports whether the
//! submission was accepted, any verifier warnings, and the IDs of events emitted.

use serde::{Deserialize, Serialize};
use crate::model::RouteType;

/// POST /ingest/feed
/// Submitted by a crawler. Core validates via `VerifierChain` before writing.
#[derive(Debug, Deserialize)]
pub struct IngestFeedRequest {
    pub canonical_url: String,
    pub source_url:    String,
    pub crawl_token:   String,
    pub http_status:   u16,
    pub content_hash:  String,
    pub feed_data:     Option<IngestFeedData>,
}

#[derive(Debug, Deserialize)]
pub struct IngestFeedData {
    pub feed_guid:    String,
    pub title:        String,
    pub description:  Option<String>,
    pub image_url:    Option<String>,
    pub language:     Option<String>,
    pub explicit:     bool,
    pub itunes_type:  Option<String>,
    pub raw_medium:   Option<String>,
    pub author_name:  Option<String>,
    pub owner_name:   Option<String>,
    pub pub_date:     Option<i64>,
    pub tracks:       Vec<IngestTrackData>,
}

#[derive(Debug, Deserialize)]
pub struct IngestTrackData {
    pub track_guid:        String,
    pub title:             String,
    pub pub_date:          Option<i64>,
    pub duration_secs:     Option<i64>,
    pub enclosure_url:     Option<String>,
    pub enclosure_type:    Option<String>,
    pub enclosure_bytes:   Option<i64>,
    pub track_number:      Option<i64>,
    pub season:            Option<i64>,
    pub explicit:          bool,
    pub description:       Option<String>,
    /// Per-track author override — some feeds have different artist per track
    pub author_name:       Option<String>,
    pub payment_routes:    Vec<IngestPaymentRoute>,
    pub value_time_splits: Vec<IngestValueTimeSplit>,
}

#[derive(Debug, Deserialize)]
pub struct IngestPaymentRoute {
    pub recipient_name: Option<String>,
    pub route_type:     RouteType,
    pub address:        String,
    pub custom_key:     Option<String>,
    pub custom_value:   Option<String>,
    pub split:          i64,
    pub fee:            bool,
}

#[derive(Debug, Deserialize)]
pub struct IngestValueTimeSplit {
    pub start_time_secs:  i64,
    pub duration_secs:    Option<i64>,
    pub remote_feed_guid: String,
    pub remote_item_guid: String,
    pub split:            i64,
}

#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub accepted:       bool,
    pub reason:         Option<String>,
    pub events_emitted: Vec<String>,
    pub no_change:      bool,
    pub warnings:       Vec<String>,
}
