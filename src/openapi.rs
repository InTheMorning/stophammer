#![expect(
    clippy::unreadable_literal,
    reason = "OpenAPI examples preserve raw timestamp and byte values to match the published API docs"
)]

//! `OpenAPI` document for the custom API explorer.
//!
//! The runtime docs UI is intentionally custom (`api.html`) so it can match the
//! project's existing visual design. This module generates the `OpenAPI` document
//! that powers that UI.

use serde_json::{Value, json};

use crate::{api, ingest, query, sync};

/// Returns the primary-node `OpenAPI` document.
#[must_use]
pub fn primary_document() -> utoipa::openapi::OpenApi {
    document(DocMode::Primary)
}

/// Returns the community/read-only `OpenAPI` document.
#[must_use]
pub fn readonly_document() -> utoipa::openapi::OpenApi {
    document(DocMode::Readonly)
}

/// Returns the embedded API explorer HTML.
#[must_use]
pub const fn api_explorer_html() -> &'static str {
    include_str!("../api.html")
}

#[derive(Clone, Copy)]
enum DocMode {
    Primary,
    Readonly,
}

fn document(mode: DocMode) -> utoipa::openapi::OpenApi {
    let mut doc: utoipa::openapi::OpenApi =
        serde_json::from_value(spec_value(mode)).expect("static OpenAPI document must be valid");
    let feed_track_item: utoipa::openapi::path::PathItem =
        serde_json::from_value(feed_track_path_item(mode))
            .expect("feed-scoped track path item must be valid");
    doc.paths.paths.insert(
        "/v1/feeds/{guid}/tracks/{track_guid}".into(),
        feed_track_item,
    );
    doc
}

#[expect(
    clippy::too_many_lines,
    reason = "The static OpenAPI path map is easiest to audit as one contiguous definition"
)]
fn spec_value(mode: DocMode) -> Value {
    let mut paths = serde_json::Map::new();
    paths.insert(
        "/health".into(),
        json!({
            "get": operation(
                "Health check",
                "Liveness probe that returns plain text `ok`.",
                "Core",
                vec![],
                None,
                json!({
                    "200": text_response("Server is healthy.", "ok")
                }),
                None
            )
        }),
    );
    paths.insert(
        "/node/info".into(),
        json!({
            "get": operation(
                "Node public key",
                "Returns this node's ed25519 public key.",
                "Core",
                vec![],
                None,
                json!({
                    "200": json_response(
                        "Node information.",
                        json!({
                            "node_pubkey": "0805c402f021e6e0dfbb6b2f5d34628f7b166b075a0170e6e5e293c50b3b55e2"
                        })
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/sync/events".into(),
        json!({
            "get": operation(
                "Poll incremental events",
                "Paginated event log for community nodes to poll.",
                "Sync",
                vec![
                    query_param("after_seq", "integer", Some("int64"), false, "Return events with `seq > after_seq`."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum events to return (capped at 1000).")
                ],
                None,
                json!({
                    "200": json_response(
                        "Incremental events page.",
                        json!({
                            "events": [event_example()],
                            "has_more": false,
                            "next_seq": 42
                        })
                    ),
                    "403": error_response("Missing or invalid sync token.")
                }),
                Some(sync_security())
            )
        }),
    );
    paths.insert(
        "/sync/peers".into(),
        json!({
            "get": operation(
                "List sync peers",
                "Returns all known active peer nodes.",
                "Sync",
                vec![],
                None,
                json!({
                    "200": json_response(
                        "Known sync peers.",
                        json!({
                            "nodes": [{
                                "node_pubkey": "hex-ed25519-pubkey",
                                "node_url": "https://community-node.example.com/sync/push",
                                "last_push_at": 1710288000
                            }]
                        })
                    ),
                    "403": error_response("Missing or invalid sync token.")
                }),
                Some(sync_security())
            )
        }),
    );
    paths.insert(
        "/v1/feeds/recent".into(),
        json!({
            "get": operation(
                "List recent feeds",
                "Lists source feeds in recent-source order for provenance and debugging workflows.",
                "Feeds",
                vec![
                    query_param("cursor", "string", None, false, "Opaque pagination cursor."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum rows to return."),
                    query_param("include", "string", None, false, "Comma-separated include list. Supports `tracks`."),
                    query_param("medium", "string", None, false, "Optional feed medium filter. Defaults to `music`. Use `all` for every medium.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Paginated recent feeds.",
                        query_envelope_example(json!([
                            {
                                "feed_guid": "feed-guid",
                                "feed_url": "https://example.com/feed.xml",
                                "title": "Recent Feed",
                                "raw_medium": "music"
                            }
                        ]))
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/feeds/{guid}/route-history".into(),
        json!({
            "get": operation(
                "Get feed route history",
                "Returns each change of the payment recipients of a feed and its tracks, read from the signed event log (ADR 0053 section 4).",
                "Feeds",
                vec![
                    path_param("guid", "string", "Feed GUID.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Route-history entries, in `seq` order, the newest last.",
                        query_envelope_example(json!([
                            {
                                "subject": "feed",
                                "track_guid": null,
                                "event_id": "event-id-1",
                                "seq": 10,
                                "changed_at": 1710288000,
                                "old_recipients": null,
                                "new_recipients": [{ "address": "a@ln.example", "split": 100 }]
                            },
                            {
                                "subject": "feed",
                                "track_guid": null,
                                "event_id": "event-id-2",
                                "seq": 15,
                                "changed_at": 1710300000,
                                "old_recipients": [{ "address": "a@ln.example", "split": 100 }],
                                "new_recipients": [{ "address": "b@ln.example", "split": 100 }]
                            }
                        ]))
                    ),
                    "404": error_response("No event names a route set of this feed.")
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/feeds/{guid}/copies".into(),
        json!({
            "get": operation(
                "Get feed copies",
                "Returns each row of the ADR 0058 feed-copy summary of a feed: the URL, the two differences against the current record, guid_origin, and the resolution when one exists.",
                "Feeds",
                vec![
                    path_param("guid", "string", "Feed GUID.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Feed-copy rows, in `first_seen` order.",
                        json!({
                            "data": [
                                {
                                    "url": "https://mirror.example.com/feed.xml",
                                    "first_seen": 1710288000,
                                    "last_seen": 1710300000,
                                    "title": "Mirror Feed",
                                    "differs_tracks": false,
                                    "differs_recipients": true,
                                    "guid_origin": false,
                                    "open": true,
                                    "feed_recipients": [{ "address": "attacker@ln.example", "split": 100 }],
                                    "track_recipients": {},
                                    "resolution": null
                                }
                            ],
                            "pagination": { "cursor": null, "has_more": false },
                            "meta": { "api_version": "v1", "node_pubkey": "hex-pubkey" },
                            "copies_over_limit": 0
                        })
                    ),
                    "404": error_response("Feed not found.")
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/copies".into(),
        json!({
            "get": operation(
                "List records with an open copy",
                "Returns each record that has at least one open ADR 0058 copy, or a copies_over_limit counter above zero, newest first.",
                "Feeds",
                vec![
                    query_param("cursor", "string", None, false, "Opaque pagination cursor."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum rows to return, at most 100.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Paginated list of records with an open copy.",
                        query_envelope_example(json!([
                            {
                                "feed_guid": "feed-guid",
                                "feed_url": "https://example.com/feed.xml",
                                "title": "Victim Feed",
                                "copy_count": 1,
                                "copies_over_limit": 0,
                                "newest_first_seen": 1710300000
                            }
                        ]))
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/guid-changes".into(),
        json!({
            "get": operation(
                "List pending GUID changes",
                "Returns each pending GUID change with no `reject` decision, newest `first_seen` first (ADR 0052 section 4).",
                "Feeds",
                vec![
                    query_param("cursor", "string", None, false, "Opaque pagination cursor."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum rows to return, at most 100.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Paginated list of pending GUID changes.",
                        query_envelope_example(json!([
                            {
                                "source_url": "https://feeds.example.com/my-music-feed",
                                "old_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                                "new_guid": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
                                "first_seen": 1710288000,
                                "last_seen": 1710300000,
                                "decision": null
                            }
                        ]))
                    )
                }),
                None
            )
        }),
    );
    paths.insert("/v1/feeds/{guid}".into(), feed_path_item(mode));
    paths.insert("/v1/tracks/{guid}".into(), track_path_item(mode));
    paths.insert(
        "/v1/tracks".into(),
        json!({
            "get": operation(
                "List tracks by artist",
                "Returns all tracks whose `track_artist` matches the given name (case-insensitive). Paginated newest-first.",
                "Tracks",
                vec![
                    query_param("artist", "string", None, true, "Artist name to filter by (case-insensitive exact match)."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum rows to return."),
                    query_param("cursor", "string", None, false, "Opaque pagination cursor.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Paginated artist tracks.",
                        query_envelope_example(json!([
                            {
                                "track_guid": "track-guid",
                                "feed_guid": "feed-guid",
                                "title": "Track Title",
                                "track_artist": "Artist Name",
                                "track_artist_sort": "Name, Artist",
                                "pub_date": 1710288000,
                                "duration_secs": 210,
                                "image_url": "https://example.com/cover.jpg",
                                "track_image_url": null,
                                "feed_image_url": "https://example.com/cover.jpg",
                                "track_number": 1,
                                "feed_title": "Album Title",
                                "release_artist": "Artist Name",
                                "release_artist_source": "itunes_author",
                                "created_at": 1710288000
                            }
                        ]))
                    ),
                    "400": error_response("Missing or invalid `artist` parameter.")
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/search".into(),
        json!({
            "get": operation(
                "Search feeds and tracks",
                "Full-text search using SQLite FTS5.",
                "Search",
                vec![
                    query_param("q", "string", None, true, "Search query (FTS5 syntax)."),
                    query_param("type", "string", None, false, "Filter by entity type: `feed` or `track`."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum results to return."),
                    query_param("cursor", "string", None, false, "Opaque keyset pagination cursor.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Search results.",
                        query_envelope_example(json!([
                            {
                                "entity_type": "track",
                                "entity_id": "track-guid",
                                "feed_guid": "feed-guid",
                                "href": "/v1/feeds/feed-guid/tracks/track-guid",
                                "rank": -1.5,
                                "quality_score": 0,
                                "title": "Track Title",
                                "feed_title": "Album Title",
                                "track_image_url": null,
                                "feed_image_url": "https://example.com/cover.jpg",
                                "pub_date": 1710288000
                            }
                        ]))
                    ),
                    "400": error_response("Invalid FTS5 query syntax.")
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/node/capabilities".into(),
        json!({
            "get": operation(
                "Read node capabilities",
                "Returns API version, capabilities, supported entity types, and valid include parameters.",
                "Node",
                vec![],
                None,
                json!({
                    "200": json_response(
                        "Node capability document.",
                        json!({
                            "api_version": "v1",
                            "node_pubkey": "hex-pubkey",
                            "capabilities": ["query", "search", "sync", "push"],
                            "entity_types": ["feed", "track"],
                            "include_params": {
                                "feed": ["tracks", "payment_routes", "source_links", "source_ids", "source_contributors", "source_platforms", "source_release_claims", "remote_items", "publisher"],
                                "track": ["payment_routes", "value_time_splits", "source_links", "source_ids", "source_contributors", "source_release_claims", "source_enclosures", "source_transcripts"]
                            }
                        })
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/peers".into(),
        json!({
            "get": operation(
                "List public peers",
                "Lists all known peer nodes from the public peer table.",
                "Node",
                vec![],
                None,
                json!({
                    "200": json_response(
                        "Public peers.",
                        json!([
                            {
                                "node_pubkey": "hex-pubkey",
                                "node_url": "https://community-node.example.com/sync/push",
                                "last_push_at": 1710288000
                            }
                        ])
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/publishers".into(),
        json!({
            "get": operation(
                "Search publisher text",
                "Lists non-empty publisher text values with feed and track counts.",
                "Publishers",
                vec![
                    query_param("q", "string", None, false, "Optional substring filter."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum publishers returned."),
                    query_param("case_sensitive", "boolean", None, false, "Set to `true` for case-sensitive matching. Defaults to `false`.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Publisher facet results.",
                        query_envelope_example(json!([
                            {
                                "publisher_text": "Wavlake",
                                "feed_count": 42,
                                "track_count": 500
                            }
                        ]))
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/publishers/{publisher}".into(),
        json!({
            "get": operation(
                "Get publisher detail",
                "Returns feeds and tracks whose stored publisher text contains the path parameter. Matching is partial (substring); case-insensitive by default.",
                "Publishers",
                vec![
                    path_param("publisher", "string", "Publisher text to match (substring)."),
                    query_param("limit", "integer", Some("int64"), false, "Maximum feeds and tracks returned."),
                    query_param("case_sensitive", "boolean", None, false, "Set to `true` for case-sensitive matching. Defaults to `false`.")
                ],
                None,
                json!({
                    "200": json_response(
                        "Publisher detail response.",
                        query_envelope_example(json!({
                            "publisher_text": "Wavlake",
                            "feeds": [{
                                "feed_guid": "feed-guid",
                                "feed_url": "https://example.com/feed.xml",
                                "title": "Feed Title",
                                "image_url": "https://example.com/cover.jpg",
                                "episode_count": 12,
                                "raw_medium": "music"
                            }],
                            "tracks": [{
                                "track_guid": "track-guid",
                                "feed_guid": "feed-guid",
                                "title": "Track Title",
                                "image_url": "https://example.com/track.jpg",
                                "track_image_url": "https://example.com/track.jpg",
                                "feed_image_url": "https://example.com/cover.jpg",
                                "duration_secs": 240,
                                "track_number": 1
                            }]
                        }))
                    )
                }),
                None
            )
        }),
    );
    paths.insert(
        "/v1/publisher-links/stats".into(),
        json!({
            "get": operation(
                "Get publisher link statistics",
                "Gives the number of listed publisher links, by resolution: `guid`, `feed_url` or `unresolved`. ADR 0049 section 8.",
                "Publishers",
                vec![],
                None,
                json!({
                    "200": json_response(
                        "Publisher link statistics.",
                        query_envelope_example(json!({
                            "listed_links": 40,
                            "resolved_by_guid": 10,
                            "resolved_by_feed_url": 25,
                            "unresolved": 5
                        }))
                    )
                }),
                None
            )
        }),
    );
    if matches!(mode, DocMode::Primary) {
        paths.insert(
            "/ingest/feed".into(),
            json!({
                "post": operation(
                    "Submit crawl data",
                    "Crawler submission endpoint. Validates the feed through the verifier chain and writes accepted changes atomically. The crawl token is supplied in the JSON body.",
                    "Ingest",
                    vec![],
                    Some(json_request_body(
                        "Crawler submission payload.",
                        ingest_request_example()
                    )),
                    json!({
                        "200": json_response(
                            "Ingest result. A `source_conflict` reason (ADR 0051 section 5) also carries `source_url`, the stored source URL of the held record. A `guid_change_pending` or `guid_change_rejected` reason (ADR 0052 section 4) means the source URL declares a GUID other than the one the record holds. A GUID-change transition (ADR 0052 section 5), run either for the UUIDv5 of the source URL or for an operator approval, names its old and new GUID in `warnings`: `GUID changed from <old> to <new> (ADR 0052 UUIDv5)` or `(ADR 0052 approved)`.",
                            json!({
                                "accepted": true,
                                "reason": null,
                                "events_emitted": ["uuid-1", "uuid-2", "uuid-3"],
                                "no_change": false,
                                "warnings": [],
                                "source_url": null
                            })
                        )
                    }),
                    None
                )
            }),
        );
        paths.insert(
            "/sync/register".into(),
            json!({
                "post": operation(
                    "Register push endpoint",
                    "Community nodes announce their push URL to the primary. The primary verifies same-origin ownership through `GET /node/info` and stores the peer for future fan-out.",
                    "Sync",
                    vec![],
                    Some(json_request_body(
                        "Community-node registration payload.",
                        json!({
                            "node_pubkey": "hex-ed25519-pubkey",
                            "node_url": "https://community-node.example.com/sync/push",
                            "signed_at": 1773849600,
                            "signature": "hex-ed25519-signature"
                        })
                    )),
                    json!({
                        "200": json_response("Peer registered.", json!({ "ok": true })),
                        "400": error_response("Invalid signed payload or timestamp."),
                        "403": error_response("Missing or invalid sync token."),
                        "422": error_response("Rejected node URL or ownership verification failed.")
                    }),
                    Some(sync_security())
                )
            }),
        );
        paths.insert(
            "/sync/reconcile".into(),
            json!({
                "post": operation(
                    "Reconcile diverged state",
                    "Set-diff catch-up for nodes rejoining after downtime.",
                    "Sync",
                    vec![],
                    Some(json_request_body(
                        "Reconcile request payload.",
                        json!({
                            "node_pubkey": "hex-ed25519-pubkey",
                            "have": [
                                { "event_id": "uuid-1", "seq": 10 },
                                { "event_id": "uuid-2", "seq": 11 }
                            ],
                            "since_seq": 0
                        })
                    )),
                    json!({
                        "200": json_response(
                            "Reconcile result.",
                            json!({
                                "send_to_node": [event_example()],
                                "unknown_to_us": [{ "event_id": "uuid-x", "seq": 99 }],
                                "has_more": false,
                                "next_seq": 99
                            })
                        ),
                        "400": error_response("Request exceeds reconcile limits."),
                        "403": error_response("Missing or invalid sync token.")
                    }),
                    Some(sync_security())
                )
            }),
        );
        paths.insert(
            "/v1/feeds/{guid}/tracks/{track_guid}".into(),
            feed_track_path_item(mode),
        );
        paths.insert(
            "/v1/blocks".into(),
            json!({
                "post": operation(
                    "Create a block",
                    "Blocks a feed GUID or an exact feed URL from ingest (ADR 0053 section 1). A pair the table already holds changes nothing and answers `409` with the existing `block_id`.",
                    "Blocks",
                    vec![],
                    Some(json_request_body(
                        "Block creation payload.",
                        json!({
                            "kind": "guid",
                            "value": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                            "reason": "reported as spam"
                        })
                    )),
                    json!({
                        "201": json_response(
                            "Block created.",
                            json!({
                                "block_id": "uuid",
                                "kind": "guid",
                                "value": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                                "reason": "reported as spam",
                                "blocked_at": 1710288000
                            })
                        ),
                        "400": error_response("Empty `value` or `reason` after trim."),
                        "403": error_response("Missing or invalid admin token."),
                        "409": json_response(
                            "The kind/value pair already has a block.",
                            json!({ "block_id": "uuid" })
                        )
                    }),
                    Some(admin_only_security())
                ),
                "get": operation(
                    "List blocks",
                    "Lists every feed-block row, in `blocked_at` order.",
                    "Blocks",
                    vec![],
                    None,
                    json!({
                        "200": json_response(
                            "Block rows.",
                            json!({
                                "blocks": [{
                                    "block_id": "uuid",
                                    "kind": "guid",
                                    "value": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                                    "reason": "reported as spam",
                                    "blocked_at": 1710288000
                                }]
                            })
                        ),
                        "403": error_response("Missing or invalid admin token.")
                    }),
                    Some(admin_only_security())
                )
            }),
        );
        paths.insert(
            "/v1/blocks/{block_id}".into(),
            json!({
                "delete": operation(
                    "Delete a block",
                    "Removes a `feed_blocks` row and signs a `FeedUnblocked` event (ADR 0053 section 1). A missing row signs nothing.",
                    "Blocks",
                    vec![path_param("block_id", "string", "Block ID (UUID).")],
                    None,
                    json!({
                        "204": no_content_response("Block removed."),
                        "403": error_response("Missing or invalid admin token."),
                        "404": error_response("Block not found.")
                    }),
                    Some(admin_only_security())
                )
            }),
        );
        paths.insert(
            "/v1/feeds/{guid}/copies/resolve".into(),
            json!({
                "post": operation(
                    "Resolve a feed copy",
                    "The operator keeps the source or relocates the record to the copy's URL (ADR 0058 section 4). `relocate` calls the same relocation as `PATCH /v1/feeds/{guid}`: it clears `last_build_date` and `declared_self_url`, and signs a `FeedUpserted` event before the `FeedCopyResolved` event.",
                    "Feeds",
                    vec![path_param("guid", "string", "Feed GUID.")],
                    Some(json_request_body(
                        "Resolution payload.",
                        json!({
                            "url": "https://mirror.example.com/feed.xml",
                            "decision": "keep_source",
                            "reason": "confirmed this is an impersonation; keeping the held record"
                        })
                    )),
                    json!({
                        "200": json_response(
                            "Resolution applied. `event_ids` has one entry for `keep_source`, and two — `FeedUpserted` then `FeedCopyResolved` — for `relocate`.",
                            json!({ "event_ids": ["uuid-1", "uuid-2"] })
                        ),
                        "400": error_response("Empty reason, or decision is not \"keep_source\" or \"relocate\"."),
                        "403": error_response("Missing or invalid admin token."),
                        "404": error_response("Feed not found, or it has no copy row at that url."),
                        "409": error_response("decision is \"relocate\" and another record already holds the copy's url as its source URL.")
                    }),
                    Some(admin_only_security())
                )
            }),
        );
        paths.insert(
            "/v1/feeds/{guid}/guid-change".into(),
            json!({
                "post": operation(
                    "Decide a pending GUID change",
                    "The operator approves or rejects a pending GUID change (ADR 0052 section 4). `{guid}` is the old GUID the pending row names. The node does not keep the body of the submission that opened the row: `approve` only sets the decision, and the transition runs at the next submission of the new GUID from the source URL.",
                    "Feeds",
                    vec![path_param("guid", "string", "The old GUID a pending row names.")],
                    Some(json_request_body(
                        "Decision payload.",
                        json!({
                            "decision": "approve",
                            "reason": "confirmed with the publisher"
                        })
                    )),
                    json!({
                        "200": json_response(
                            "Decision applied.",
                            json!({ "event_id": "uuid" })
                        ),
                        "400": error_response("Empty reason, or decision is not \"approve\" or \"reject\"."),
                        "403": error_response("Missing or invalid admin token."),
                        "404": error_response("No pending GUID change names this GUID as its old GUID.")
                    }),
                    Some(admin_only_security())
                )
            }),
        );
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Stophammer API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Generated OpenAPI document for the custom Stophammer API explorer."
        },
        "tags": [
            { "name": "Core", "description": "Health and node identity endpoints." },
            { "name": "Ingest", "description": "Crawler ingestion endpoint." },
            { "name": "Sync", "description": "Primary/community replication protocol." },
            { "name": "Feeds", "description": "Feed query and mutation endpoints." },
            { "name": "Tracks", "description": "Track query and mutation endpoints." },
            { "name": "Search", "description": "Full-text search endpoints." },
            { "name": "Node", "description": "Node capability and public metadata endpoints." },
            { "name": "Publishers", "description": "Publisher facet search and detail endpoints." },
            { "name": "Blocks", "description": "Feed and URL block administration (ADR 0053 section 1)." }
        ],
        "components": {
            "securitySchemes": {
                "AdminToken": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Admin-Token"
                },
                "SyncToken": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Sync-Token"
                }
            },
            "schemas": Value::Object(schemas_object())
        },
        "paths": Value::Object(paths)
    })
}

// ── `components.schemas` (ADR 0044) ─────────────────────────────────────────
//
// Each `ToSchema` type below is a JSON response type from `query`, `api`,
// `ingest` or `sync`. No response in this document references a schema by
// `$ref` yet; that comes in a later task. This section only publishes the
// schemas so a client can compare its own field expectations against them.

/// Builds `components.schemas` from every documented response type's
/// `utoipa::ToSchema` derive.
fn schemas_object() -> serde_json::Map<String, Value> {
    let mut schemas = Vec::new();
    schemas.extend(query::response_schemas());
    schemas.extend(api::response_schemas());
    schemas.extend(ingest::response_schemas());
    schemas.extend(sync::response_schemas());

    let mut map = serde_json::Map::new();
    for (name, schema) in schemas {
        let value = serde_json::to_value(schema).expect("utoipa schema serializes");
        map.insert(name, value);
    }
    map
}

fn feed_path_item(mode: DocMode) -> Value {
    let mut item = serde_json::Map::new();
    item.insert(
        "get".into(),
        operation(
            "Get feed by GUID",
            "Returns a single feed by its `feed_guid`.",
            "Feeds",
            vec![
                path_param("guid", "string", "Feed GUID."),
                query_param("cursor", "string", None, false, "Opaque pagination cursor for included nested collections."),
                query_param("limit", "integer", Some("int64"), false, "Maximum nested rows to return."),
                query_param("include", "string", None, false, "Comma-separated include list. Supports `tracks`, `payment_routes`, `source_links`, `source_ids`, `source_contributors`, `source_platforms`, `source_release_claims`, `remote_items`, `publisher`."),
                query_param("medium", "string", None, false, "Optional medium override used by shared query parsing.")
            ],
            None,
            json!({
                "200": json_response(
                    "Feed detail response.",
                    query_envelope_example(json!({
                        "feed_guid": "feed-guid",
                        "feed_url": "https://example.com/feed.xml",
                        "title": "My Music Feed",
                        "release_artist": "Artist Name",
                        "release_artist_source": "itunes_author",
                        "publisher_feed_title": "Publisher Feed Title",
                        "copy_count": 0,
                        "pending_guid_change": null
                    }))
                ),
                "404": json_response(
                    "Feed not found. Carries `superseded_by`, the GUID that replaced this one, when a GUID-change transition retired it (ADR 0052 section 5).",
                    json!({ "error": "feed not found", "superseded_by": "new-feed-guid" })
                )
            }),
            None,
        ),
    );

    if matches!(mode, DocMode::Primary) {
        item.insert(
            "patch".into(),
            operation(
                "Patch feed",
                "Updates a feed's mutable fields. Currently supports `feed_url` only, which relocates the record (ADR 0058 section 5). A relocation needs `reason`, and clears `last_build_date` and `declared_self_url`.",
                "Feeds",
                vec![path_param("guid", "string", "Feed GUID.")],
                Some(json_request_body(
                    "JSON Merge Patch payload. `reason` is required when `feed_url` is present.",
                    json!({
                        "feed_url": "https://new-feed-url.example.com/feed.xml",
                        "reason": "confirmed move to the new host"
                    }),
                )),
                json!({
                    "204": no_content_response("Feed updated."),
                    "400": error_response("feed_url is present with no reason, or an empty reason."),
                    "403": error_response("Missing or invalid admin token."),
                    "404": error_response("Feed not found."),
                    "409": error_response("Another record already holds feed_url as its source URL.")
                }),
                Some(admin_only_security()),
            ),
        );
        item.insert(
            "delete".into(),
            operation(
                "Retire feed",
                "Retires a feed and cascade-deletes its dependent data. In the same transaction, this also blocks the feed GUID and the stored feed URL (ADR 0053 section 1), unless `block=false` asks for no block.",
                "Feeds",
                vec![
                    path_param("guid", "string", "Feed GUID."),
                    query_param("block", "boolean", None, false, "Defaults to `true`. When `false`, retires with no block. Only the admin token may send `block=false`.")
                ],
                None,
                json!({
                    "204": no_content_response("Feed retired."),
                    "403": error_response("Missing or invalid admin token."),
                    "404": error_response("Feed not found.")
                }),
                Some(admin_only_security()),
            ),
        );
    }

    Value::Object(item)
}

fn track_path_item(mode: DocMode) -> Value {
    let mut item = serde_json::Map::new();
    item.insert(
        "get".into(),
        operation(
            "Get track by GUID",
            "Compatibility lookup by raw `track_guid`. Returns `409 Conflict` if multiple feeds publish the same track GUID; callers should retry the canonical feed-scoped route.",
            "Tracks",
            vec![
                path_param("guid", "string", "Track GUID."),
                query_param("include", "string", None, false, "Comma-separated include list. Supports `payment_routes`, `value_time_splits`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`, `source_transcripts`, `remote_items`, `publisher`."),
                query_param("limit", "integer", Some("int64"), false, "Maximum nested rows to return."),
                query_param("cursor", "string", None, false, "Opaque pagination cursor for included nested collections.")
            ],
            None,
            json!({
                "200": json_response(
                    "Track detail response.",
                    query_envelope_example(json!({
                        "track_guid": "track-guid",
                        "feed_guid": "feed-guid",
                        "title": "Track Title",
                        "publisher_text": "Wavlake",
                        "track_artist": "Artist Name"
                    }))
                ),
                "404": error_response("Track not found."),
                "409": json_response(
                    "Track GUID is ambiguous across feeds.",
                    json!({
                        "error": "track_guid track-guid is ambiguous; retry with the canonical feed-scoped route",
                        "code": "ambiguous_track_guid",
                        "track_guid": "track-guid",
                        "candidates": [{
                            "feed_guid": "feed-guid",
                            "href": "/v1/feeds/feed-guid/tracks/track-guid"
                        }]
                    })
                )
            }),
            None,
        ),
    );

    if matches!(mode, DocMode::Primary) {
        item.insert(
            "patch".into(),
            operation(
                "Patch track",
                "Compatibility mutation by raw `track_guid`. Returns `409 Conflict` if the GUID is ambiguous across feeds; callers should retry the canonical feed-scoped route.",
                "Tracks",
                vec![path_param("guid", "string", "Track GUID.")],
                Some(json_request_body(
                    "JSON Merge Patch payload.",
                    json!({ "enclosure_url": "https://new-cdn.example.com/track.mp3" }),
                )),
                json!({
                    "204": no_content_response("Track updated."),
                    "403": error_response("Missing or invalid admin token."),
                    "404": error_response("Track not found."),
                    "409": json_response(
                        "Track GUID is ambiguous across feeds.",
                        json!({
                            "error": "track_guid track-guid is ambiguous; retry with the canonical feed-scoped route",
                            "code": "ambiguous_track_guid",
                            "track_guid": "track-guid",
                            "candidates": [{
                                "feed_guid": "feed-guid",
                                "href": "/v1/feeds/feed-guid/tracks/track-guid"
                            }]
                        })
                    )
                }),
                Some(admin_only_security()),
            ),
        );
    }

    Value::Object(item)
}

fn feed_track_path_item(mode: DocMode) -> Value {
    let mut item = serde_json::Map::new();
    item.insert(
        "get".into(),
        operation(
            "Get track by feed and GUID",
            "Canonical track lookup by parent `feed_guid` plus raw source `track_guid`.",
            "Tracks",
            vec![
                path_param("guid", "string", "Parent feed GUID."),
                path_param("track_guid", "string", "Track GUID."),
                query_param("include", "string", None, false, "Comma-separated include list. Supports `payment_routes`, `value_time_splits`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`, `source_transcripts`, `remote_items`, `publisher`."),
                query_param("limit", "integer", Some("int64"), false, "Maximum nested rows to return."),
                query_param("cursor", "string", None, false, "Opaque pagination cursor for included nested collections.")
            ],
            None,
            json!({
                "200": json_response(
                    "Track detail response.",
                    query_envelope_example(json!({
                        "track_guid": "track-guid",
                        "feed_guid": "feed-guid",
                        "title": "Track Title",
                        "publisher_text": "Wavlake",
                        "track_artist": "Artist Name"
                    }))
                ),
                "404": error_response("Track not found in the specified feed.")
            }),
            None,
        ),
    );

    if matches!(mode, DocMode::Primary) {
        item.insert(
            "patch".into(),
            operation(
                "Patch track by feed and GUID",
                "Canonical mutation route for a track scoped by parent `feed_guid` and raw source `track_guid`. Currently supports `enclosure_url` only.",
                "Tracks",
                vec![
                    path_param("guid", "string", "Parent feed GUID."),
                    path_param("track_guid", "string", "Track GUID.")
                ],
                Some(json_request_body(
                    "JSON Merge Patch payload.",
                    json!({ "enclosure_url": "https://new-cdn.example.com/track.mp3" }),
                )),
                json!({
                    "204": no_content_response("Track updated."),
                    "403": error_response("Missing or invalid admin token."),
                    "404": error_response("Track not found in the specified feed.")
                }),
                Some(admin_only_security()),
            ),
        );
        item.insert(
            "delete".into(),
            operation(
                "Remove track from feed",
                "Deletes a single track from its parent feed.",
                "Tracks",
                vec![
                    path_param("guid", "string", "Parent feed GUID."),
                    path_param("track_guid", "string", "Track GUID."),
                ],
                None,
                json!({
                    "204": no_content_response("Track removed."),
                    "403": error_response("Missing or invalid admin token."),
                    "404": error_response("Track not found or does not belong to the feed.")
                }),
                Some(admin_only_security()),
            ),
        );
    }

    Value::Object(item)
}

fn operation(
    summary: &str,
    description: &str,
    tag: &str,
    parameters: Vec<Value>,
    request_body: Option<Value>,
    responses: Value,
    security: Option<Value>,
) -> Value {
    let mut operation = serde_json::Map::new();
    operation.insert("tags".into(), json!([tag]));
    operation.insert("summary".into(), json!(summary));
    operation.insert("description".into(), json!(description));
    operation.insert("responses".into(), responses);
    if !parameters.is_empty() {
        operation.insert("parameters".into(), Value::Array(parameters));
    }
    if let Some(request_body) = request_body {
        operation.insert("requestBody".into(), request_body);
    }
    if let Some(security) = security {
        operation.insert("security".into(), security);
    }
    Value::Object(operation)
}

fn path_param(name: &str, schema_type: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "description": description,
        "schema": { "type": schema_type }
    })
}

fn query_param(
    name: &str,
    schema_type: &str,
    format: Option<&str>,
    required: bool,
    description: &str,
) -> Value {
    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), json!(schema_type));
    if let Some(format) = format {
        schema.insert("format".into(), json!(format));
    }
    json!({
        "name": name,
        "in": "query",
        "required": required,
        "description": description,
        "schema": Value::Object(schema)
    })
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "OpenAPI helper calls mostly pass temporary JSON values built inline"
)]
fn json_request_body(description: &str, example: Value) -> Value {
    json!({
        "required": true,
        "description": description,
        "content": {
            "application/json": {
                "schema": { "type": "object" },
                "example": &example
            }
        }
    })
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "OpenAPI helper calls mostly pass temporary JSON values built inline"
)]
fn json_response(description: &str, example: Value) -> Value {
    json!({
        "description": description,
        "content": {
            "application/json": {
                "schema": { "type": "object" },
                "example": &example
            }
        }
    })
}

fn text_response(description: &str, example: &str) -> Value {
    json!({
        "description": description,
        "content": {
            "text/plain": {
                "schema": { "type": "string" },
                "example": example
            }
        }
    })
}

fn no_content_response(description: &str) -> Value {
    json!({
        "description": description
    })
}

fn error_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": {
            "application/json": {
                "schema": { "type": "object" },
                "example": { "error": description }
            }
        }
    })
}

fn sync_security() -> Value {
    json!([{ "SyncToken": [] }])
}

fn admin_only_security() -> Value {
    json!([{ "AdminToken": [] }])
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "OpenAPI helper calls mostly pass temporary JSON values built inline"
)]
fn query_envelope_example(data: Value) -> Value {
    json!({
        "data": &data,
        "pagination": {
            "cursor": null,
            "has_more": false
        },
        "meta": {
            "api_version": "v1",
            "node_pubkey": "hex-pubkey"
        }
    })
}

fn event_example() -> Value {
    json!({
        "event_id": "uuid",
        "event_type": "feed_upserted",
        "payload": {
            "type": "feed_upserted",
            "data": {
                "feed": {
                    "feed_guid": "feed-guid",
                    "feed_url": "https://example.com/feed.xml",
                    "title": "My Music Feed"
                }
            }
        },
        "subject_guid": "feed-guid",
        "signed_by": "hex-pubkey",
        "signature": "hex-ed25519-signature",
        "seq": 42,
        "created_at": 1710288000,
        "warnings": [],
        "payload_json": "{\"feed\":{\"feed_guid\":\"feed-guid\"}}"
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "The ingest example mirrors the documented request shape and is clearer inline"
)]
fn ingest_request_example() -> Value {
    json!({
        "canonical_url": "https://feeds.example.com/my-music-feed",
        "source_url": "https://feeds.example.com/my-music-feed",
        "crawl_token": "your-crawl-token",
        "http_status": 200,
        "content_hash": "sha256-hex-of-feed-body",
        "redirects": [
            { "url": "https://feeds.example.com/my-music-feed", "status": 301 }
        ],
        "feed_data": {
            "feed_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "title": "My Music Feed",
            "description": "A music podcast feed",
            "image_url": "https://example.com/cover.jpg",
            "language": "en",
            "explicit": false,
            "itunes_type": "serial",
            "raw_medium": "music",
            "author_name": "Artist Name",
            "owner_name": "Artist Name",
            "pub_date": 1710288000,
            "new_feed_url": null,
            "locked": false,
            "locked_owner": null,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": "artist-feed-guid",
                "remote_feed_url": "https://example.com/artist.xml",
                "rel": null
            }],
            "persons": [{
                "position": 0,
                "name": "Artist Name",
                "role": "vocals",
                "group_name": null,
                "href": "https://example.com/artist",
                "img": null,
                "npub": "npub1..."
            }],
            "entity_ids": [{
                "position": 0,
                "scheme": "nostr_npub",
                "value": "npub1..."
            }],
            "links": [{
                "position": 0,
                "link_type": "website",
                "url": "https://example.com/artist",
                "extraction_path": "feed.link"
            }],
            "feed_payment_routes": [{
                "recipient_name": "Artist Name",
                "route_type": "node",
                "address": "02abc...lightning-pubkey",
                "custom_key": "7629169",
                "custom_value": "podcast-guid",
                "split": 100,
                "fee": false
            }],
            "tracks": [{
                "track_guid": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
                "title": "Track Title",
                "pub_date": 1710288000,
                "duration_secs": 240,
                "enclosure_url": "https://example.com/track.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 3840000,
                "alternate_enclosures": [{
                    "position": 1,
                    "url": "https://example.com/track.flac",
                    "mime_type": "audio/flac",
                    "bytes": 12000000,
                    "rel": "alternate",
                    "title": "Lossless",
                    "extraction_path": "track.podcast:alternateEnclosure[0]"
                }],
                "track_number": 1,
                "season": 1,
                "explicit": false,
                "description": "A great track",
                "author_name": "Track Artist",
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": [],
                "transcripts": []
            }],
            "live_items": [{
                "live_item_guid": "live-item-guid",
                "title": "Tonight's Listening Party",
                "status": "pending",
                "start_at": 1710291600,
                "end_at": 1710298800,
                "content_link": "https://example.com/stream",
                "pub_date": 1710291600,
                "duration_secs": null,
                "enclosure_url": null,
                "enclosure_type": null,
                "enclosure_bytes": null,
                "alternate_enclosures": [],
                "track_number": null,
                "season": null,
                "explicit": false,
                "description": "Live premiere stream",
                "author_name": "Artist Name",
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": [],
                "transcripts": []
            }]
        }
    })
}
