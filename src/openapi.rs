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
    serde_json::from_value(spec_value(mode)).expect("static OpenAPI document must be valid")
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
                    query_param("medium", "string", None, false, "Optional feed medium filter. Defaults to `music`.")
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
                                "image_url": null,
                                "track_number": 1,
                                "feed_title": "Album Title",
                                "release_artist": "Artist Name",
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
                                "rank": -1.5,
                                "quality_score": 0
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
                            "Ingest result.",
                            json!({
                                "accepted": true,
                                "reason": null,
                                "events_emitted": ["uuid-1", "uuid-2", "uuid-3"],
                                "no_change": false,
                                "warnings": []
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
            json!({
                "delete": operation(
                    "Remove track from feed",
                    "Deletes a single track from its parent feed.",
                    "Tracks",
                    vec![
                        path_param("guid", "string", "Parent feed GUID."),
                        path_param("track_guid", "string", "Track GUID.")
                    ],
                    None,
                    json!({
                        "204": no_content_response("Track removed."),
                        "401": error_response("Missing bearer token."),
                        "403": error_response("Invalid admin token or insufficient bearer scope."),
                        "404": error_response("Track not found or does not belong to the feed.")
                    }),
                    Some(bearer_or_admin_security())
                )
            }),
        );
        paths.insert(
            "/v1/proofs/challenge".into(),
            json!({
                "post": operation(
                    "Create proof challenge",
                    "Creates a new proof-of-possession challenge for `feed:write`.",
                    "Proofs",
                    vec![],
                    Some(json_request_body(
                        "Challenge request.",
                        json!({
                            "feed_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                            "scope": "feed:write",
                            "requester_nonce": "at-least-16-chars-random-string"
                        })
                    )),
                    json!({
                        "201": json_response(
                            "Challenge created.",
                            json!({
                                "challenge_id": "uuid",
                                "token_binding": "base64url-token.base64url-sha256-nonce-hash",
                                "state": "pending",
                                "expires_at": 1710374400
                            })
                        ),
                        "400": error_response("Unsupported scope or invalid nonce."),
                        "404": error_response("Feed not found."),
                        "429": error_response("Too many pending challenges.")
                    }),
                    None
                )
            }),
        );
        paths.insert(
            "/v1/proofs/assert".into(),
            json!({
                "post": operation(
                    "Assert proof challenge",
                    "Fetches the RSS feed, verifies the published `podcast:txt` token binding, and issues an access token on success.",
                    "Proofs",
                    vec![],
                    Some(json_request_body(
                        "Proof assertion request.",
                        json!({
                            "challenge_id": "uuid",
                            "requester_nonce": "the-same-nonce-from-challenge"
                        })
                    )),
                    json!({
                        "200": json_response(
                            "Access token issued.",
                            json!({
                                "access_token": "base64url-128bit-token",
                                "scope": "feed:write",
                                "subject_feed_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                                "expires_at": 1710291600,
                                "proof_level": "rss_only"
                            })
                        ),
                        "400": error_response("Assertion failed."),
                        "404": error_response("Challenge not found or expired."),
                        "409": error_response("Feed URL changed during verification."),
                        "503": error_response("RSS fetch failed.")
                    }),
                    None
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
            { "name": "Proofs", "description": "Proof-of-possession challenge/assert flow." }
        ],
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "Opaque access token"
                },
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
            }
        },
        "paths": Value::Object(paths)
    })
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
                        "release_artist": "Artist Name"
                    }))
                ),
                "404": error_response("Feed not found.")
            }),
            None,
        ),
    );

    if matches!(mode, DocMode::Primary) {
        item.insert(
            "patch".into(),
            operation(
                "Patch feed",
                "Updates a feed's mutable fields. Currently supports `feed_url` only.",
                "Feeds",
                vec![path_param("guid", "string", "Feed GUID.")],
                Some(json_request_body(
                    "JSON Merge Patch payload.",
                    json!({ "feed_url": "https://new-feed-url.example.com/feed.xml" }),
                )),
                json!({
                    "204": no_content_response("Feed updated."),
                    "401": error_response("Missing bearer token."),
                    "403": error_response("Invalid admin token or insufficient bearer scope."),
                    "404": error_response("Feed not found.")
                }),
                Some(bearer_or_admin_security()),
            ),
        );
        item.insert(
            "delete".into(),
            operation(
                "Retire feed",
                "Retires a feed and cascade-deletes its dependent data.",
                "Feeds",
                vec![path_param("guid", "string", "Feed GUID.")],
                None,
                json!({
                    "204": no_content_response("Feed retired."),
                    "401": error_response("Missing bearer token."),
                    "403": error_response("Invalid admin token or insufficient bearer scope."),
                    "404": error_response("Feed not found.")
                }),
                Some(bearer_or_admin_security()),
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
            "Returns a single track by its `track_guid`.",
            "Tracks",
            vec![
                path_param("guid", "string", "Track GUID."),
                query_param("include", "string", None, false, "Comma-separated include list. Supports `payment_routes`, `value_time_splits`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`, `source_transcripts`."),
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
                "404": error_response("Track not found.")
            }),
            None,
        ),
    );

    if matches!(mode, DocMode::Primary) {
        item.insert(
            "patch".into(),
            operation(
                "Patch track",
                "Updates a track's mutable fields. Currently supports `enclosure_url` only.",
                "Tracks",
                vec![path_param("guid", "string", "Track GUID.")],
                Some(json_request_body(
                    "JSON Merge Patch payload.",
                    json!({ "enclosure_url": "https://new-cdn.example.com/track.mp3" }),
                )),
                json!({
                    "204": no_content_response("Track updated."),
                    "401": error_response("Missing bearer token."),
                    "403": error_response("Invalid admin token or insufficient bearer scope."),
                    "404": error_response("Track not found.")
                }),
                Some(bearer_or_admin_security()),
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

fn bearer_or_admin_security() -> Value {
    json!([{ "BearerAuth": [] }, { "AdminToken": [] }])
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
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": "artist-feed-guid",
                "remote_feed_url": "https://example.com/artist.xml"
            }],
            "persons": [{
                "position": 0,
                "name": "Artist Name",
                "role": "vocals",
                "group_name": null,
                "href": "https://example.com/artist",
                "img": null
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
