# Stophammer API Reference

Base URL: `http(s)://<node>:<port>`

All responses are JSON unless otherwise noted. Error responses use the format:

```json
{"error": "human-readable message"}
```

Rate limiting applies to all endpoints except `/health`. When exceeded, the server returns `429 Too Many Requests`:

```json
{"error": "rate limit exceeded"}
```

Artist-identity review and override workflows are intentionally CLI-only for
now. They are handled through `review_artist_identity`, not through HTTP API
mutation endpoints.

---

## 1. Health & Info

### GET /health

Liveness probe. Returns plain text `ok`.

- **Authentication:** None
- **Response:** `200 OK` with body `ok` (text/plain)

---

### GET /node/info

Returns this node's ed25519 public key.

- **Authentication:** None
- **Response:**

```json
{
  "node_pubkey": "0805c402f021e6e0dfbb6b2f5d34628f7b166b075a0170e6e5e293c50b3b55e2"
}
```

| Code | Meaning |
|------|---------|
| 200  | Success |

---

### GET /v1/resolver/status

Returns resolver queue status and explicitly documents the source-vs-canonical
read boundary for this node.

- **Authentication:** None
- **Available on:** Primary and community nodes

This endpoint is the quickest way to answer two operator questions:

- are canonical/enriched views caught up yet?
- which API surfaces are immediate source-layer reads versus resolver-backed?

Resolver-backed canonical endpoints may lag fresh ingest until `stophammer-resolverd` has
drained `resolver_queue`.

Interpretation notes:

- This is a local-node status endpoint. It reports the resolver queue and
  pause-state view of the node you are querying, not a cluster-wide or
  primary-authoritative global watermark.
- On primaries, this reflects live `stophammer-resolverd` queue state and
  coordinated import/backfill pause state.
- On community nodes, the route still exists, but communities do not run local
  resolver batches. They apply primary-authored resolved events, so this
  endpoint should be read as local database/status metadata rather than “what
  the primary is currently resolving.”
- `caught_up` means the local node currently has no queued resolver work and no
  active coordinated import/backfill pause. It does not guarantee that every
  entity a client cares about has already been resolved, nor does it expose a
  separate resolved-version watermark.

- **Response:**

```json
{
  "api_version": "v1",
  "node_pubkey": "0805c402f021e6e0dfbb6b2f5d34628f7b166b075a0170e6e5e293c50b3b55e2",
  "source_layer": {
    "authoritative": true,
    "preserved": true,
    "immediate_endpoints": [
      "/v1/feeds/{guid}",
      "/v1/tracks/{guid}",
      "/v1/feeds/recent"
    ]
  },
  "resolver": {
    "caught_up": false,
    "import_active": false,
    "import_stale": false,
    "import_heartbeat_at": 1773883813,
    "backfill_active": false,
    "backfill_stale": false,
    "backfill_heartbeat_at": null,
    "queue": {
      "total": 12,
      "ready": 12,
      "locked": 0,
      "failed": 0
    },
    "resolver_backed_endpoints": [
      "/v1/search",
      "/v1/search?type=feed",
      "/v1/search?type=track",
      "/v1/recent",
      "/v1/artists/{id}",
      "/v1/releases/{id}",
      "/v1/recordings/{id}"
    ]
  }
}
```

| Code | Meaning |
|------|---------|
| 200  | Success |

---

## 2. Ingest

### POST /ingest/feed

Crawler submission endpoint. Validates the feed through the verifier chain and, on success, writes the feed, tracks, payment routes, and events atomically.

- **Authentication:** Crawl token (in request body as `crawl_token`)
- **Available on:** Primary only
- **Max body size:** 2 MiB
- **Max tracks per request:** 500

**Request body:**

```json
{
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
    "remote_items": [
      {
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": "artist-feed-guid",
        "remote_feed_url": "https://example.com/artist.xml"
      }
    ],
    "persons": [
      {
        "position": 0,
        "name": "Artist Name",
        "role": "vocals",
        "group_name": null,
        "href": "https://example.com/artist",
        "img": null
      }
    ],
    "entity_ids": [
      {
        "position": 0,
        "scheme": "nostr_npub",
        "value": "npub1..."
      }
    ],
    "links": [
      {
        "position": 0,
        "link_type": "website",
        "url": "https://example.com/artist",
        "extraction_path": "feed.link"
      }
    ],
    "feed_payment_routes": [
      {
        "recipient_name": "Artist Name",
        "route_type": "node",
        "address": "02abc...lightning-pubkey",
        "custom_key": "7629169",
        "custom_value": "podcast-guid",
        "split": 100,
        "fee": false
      }
    ],
    "tracks": [
      {
        "track_guid": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
        "title": "Track Title",
        "pub_date": 1710288000,
        "duration_secs": 240,
        "enclosure_url": "https://example.com/track.mp3",
        "enclosure_type": "audio/mpeg",
        "enclosure_bytes": 3840000,
        "alternate_enclosures": [
          {
            "position": 1,
            "url": "https://example.com/track.flac",
            "mime_type": "audio/flac",
            "bytes": 12000000,
            "rel": "alternate",
            "title": "Lossless",
            "extraction_path": "track.podcast:alternateEnclosure[0]"
          }
        ],
        "track_number": 1,
        "season": 1,
        "explicit": false,
        "description": "A great track",
        "author_name": "Track Artist",
        "persons": [],
        "entity_ids": [],
        "links": [],
        "payment_routes": [],
        "value_time_splits": []
      }
    ],
    "live_items": [
      {
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
        "value_time_splits": []
      }
    ]
  }
}
```

`feed_data` is `null` when the crawler could not parse the feed (e.g. HTTP error). The verifier chain still runs to record the rejection.

`remote_items` carries feed-level `podcast:remoteItem` references exactly as
seen in RSS. For a music feed that points to a publisher feed, the relation
hint is typically `medium="publisher"`. `persons`, `entity_ids`, and `links`
carry staged source claims from the parser. Track and live-item payloads also
support `alternate_enclosures`. `live_items` carries parsed `podcast:liveItem`
entries; `pending` and `live` rows are staged in `live_events`, while `ended`
rows with enclosures are promoted into normal tracks.

**Response (`200 OK`):**

```json
{
  "accepted": true,
  "no_change": false,
  "reason": null,
  "events_emitted": [
    "uuid-1",
    "uuid-2",
    "uuid-3"
  ],
  "warnings": ["[enclosure_type] track 'xyz' has video enclosure type 'video/mp4'"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `accepted` | bool | `true` when the feed was written to the database |
| `no_change` | bool | `true` when the content hash matched the cache (no write performed) |
| `reason` | string? | Rejection reason when `accepted` is `false` |
| `events_emitted` | string[] | UUIDs of events emitted, in emission order |
| `warnings` | string[] | Non-fatal verifier warnings stored with the events |

| Code | Meaning |
|------|---------|
| 200  | Accepted, rejected, or no-change (check `accepted` and `no_change` fields) |
| 400  | Missing `feed_data`, or track count exceeds 500 |
| 429  | Rate limit exceeded |
| 500  | Internal error |

---

## 3. Sync Protocol

### GET /sync/events

Paginated event log for community nodes to poll.

- **Authentication:** `X-Sync-Token`
- **Available on:** Primary and community

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `after_seq` | i64 | 0 | Return events with `seq > after_seq` |
| `limit` | i64 | 500 | Max events to return (capped at 1000) |

**Response (`200 OK`):**

```json
{
  "events": [
    {
      "event_id": "uuid",
      "event_type": "feed_upserted",
      "payload": { "type": "feed_upserted", "data": { "..." } },
      "subject_guid": "feed-guid",
      "signed_by": "hex-pubkey",
      "signature": "hex-ed25519-signature",
      "seq": 42,
      "created_at": 1710288000,
      "warnings": [],
      "payload_json": "{...}"
    }
  ],
  "has_more": false,
  "next_seq": 42
}
```

| Field | Type | Description |
|-------|------|-------------|
| `events` | Event[] | Events after the cursor, ordered by `seq` |
| `has_more` | bool | `true` if more events exist beyond this page |
| `next_seq` | i64 | `seq` of the last returned event (use as next `after_seq`) |

| Code | Meaning |
|------|---------|
| 200  | Events returned successfully |
| 403  | Invalid or missing `X-Sync-Token`, or `SYNC_TOKEN` is not configured |

---

### POST /sync/register

Community nodes announce their push URL with the primary. The primary stores the peer and begins pushing new events to it.

- **Authentication:** `X-Sync-Token`
- **Available on:** Primary only
- **Validation:**
  - `node_url` must end with `/sync/push`
  - the primary fetches same-origin `GET /node/info` without following redirects and requires its `node_pubkey` to match the signed payload
  - `signed_at` must fall within the primary's allowed clock-skew window

**Request body:**

```json
{
  "node_pubkey": "hex-ed25519-pubkey",
  "node_url": "https://community-node:8008/sync/push",
  "signed_at": 1773849600,
  "signature": "hex-ed25519-signature"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `node_pubkey` | string | Yes | Ed25519 public key identifying the community node |
| `node_url` | string | Yes | Push endpoint URL the primary will POST `/sync/push` events to. Must end with `/sync/push` |
| `signed_at` | i64 | Yes | Unix timestamp included in the signed registration payload. Must be fresh enough to fall within the primary's allowed skew window |
| `signature` | string | Yes | Ed25519 signature over `{node_pubkey,node_url,signed_at}` using the community node's signing key |

**Response (`200 OK`):**

```json
{
  "ok": true
}
```

| Code | Meaning |
|------|---------|
| 200  | Registered successfully |
| 400  | Missing `signed_at` / `signature` pair, or `signed_at` outside the allowed skew window |
| 403  | Invalid or missing `X-Sync-Token`, or `SYNC_TOKEN` is not configured on the primary |
| 403  | Invalid registration signature |
| 422  | `node_url` rejected by SSRF validation, does not end with `/sync/push`, or fails same-origin `GET /node/info` ownership verification |

---

### POST /sync/push

Receives pushed events from the primary. Community nodes expose this endpoint; the primary calls it during fan-out.

- **Authentication:** None (events are verified by ed25519 signature against the known primary pubkey)
- **Available on:** Community only
- **Max body size:** 2 MiB
- **Max events per request:** 1,000

**Request body:**

```json
{
  "events": [ { "...Event..." } ]
}
```

**Response (`200 OK`):**

```json
{
  "applied": 5,
  "rejected": 0,
  "duplicate": 2
}
```

| Code | Meaning |
|------|---------|
| 200  | Batch processed |
| 400  | Batch exceeds 1,000 events |

---

### GET /sync/peers

Returns all known active peer nodes. Acts as a built-in tracker -- a new node only needs the primary URL to discover the entire network.

- **Authentication:** `X-Sync-Token`
- **Available on:** Primary and community

**Response (`200 OK`):**

```json
{
  "nodes": [
    {
      "node_pubkey": "hex-ed25519-pubkey",
      "node_url": "http://community:8008/sync/push",
      "last_push_at": 1710288000
    }
  ]
}
```

| Code | Meaning |
|------|---------|
| 200  | Peer list returned successfully |
| 403  | Invalid or missing `X-Sync-Token`, or `SYNC_TOKEN` is not configured |

---

### POST /sync/reconcile

Set-diff catch-up for nodes rejoining after downtime. The community node sends the event IDs it already holds; the primary returns only what it is missing and flags any events unknown to the primary.

- **Authentication:** Same as `POST /sync/register` (`X-Sync-Token` required)
- **Available on:** Primary only
- **Max `have` entries:** 10,000

**Request body:**

```json
{
  "node_pubkey": "hex-ed25519-pubkey",
  "have": [
    { "event_id": "uuid-1", "seq": 10 },
    { "event_id": "uuid-2", "seq": 11 }
  ],
  "since_seq": 0
}
```

**Response (`200 OK`):**

```json
{
  "send_to_node": [ { "...Event..." } ],
  "unknown_to_us": [ { "event_id": "uuid-x", "seq": 99 } ],
  "has_more": false,
  "next_seq": 99
}
```

| Field | Type | Description |
|-------|------|-------------|
| `send_to_node` | Event[] | Events the requesting node is missing |
| `unknown_to_us` | EventRef[] | Events the node reported that the primary does not recognize (anomaly) |
| `has_more` | bool | `true` when the response is truncated and reconcile pagination should continue |
| `next_seq` | i64 | Cursor to use as the next `since_seq` when `has_more` is `true` |

| Code | Meaning |
|------|---------|
| 200  | Success |
| 400  | `have` array exceeds 10,000 entries |
| 403  | Invalid or missing `X-Sync-Token`, or `SYNC_TOKEN` is not configured |

---

## 4. Queries -- Artists

All query endpoints are read-only and available on both primary and community nodes. Responses use a common envelope:

```json
{
  "data": "...",
  "pagination": {
    "cursor": "base64url-encoded-cursor-or-null",
    "has_more": false
  },
  "meta": {
    "api_version": "v1",
    "node_pubkey": "hex-pubkey"
  }
}
```

### Common query parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `cursor` | string | none | Opaque base64url-encoded pagination cursor |
| `limit` | i64 | 50 | Results per page (clamped to 1--200) |
| `include` | string | none | Comma-separated list of nested data to include |

---

### GET /v1/artists/{id}

Returns a single artist by ID. Follows `artist_id_redirect` automatically (merged artists redirect transparently).

- **Authentication:** None
- **Include options:** `aliases`, `credits`, `tags`, `relationships`

**Response (`200 OK`):**

```json
{
  "data": {
    "artist_id": "uuid",
    "name": "Artist Name",
    "sort_name": "Name, Artist",
    "area": "US",
    "img_url": "https://...",
    "url": "https://...",
    "begin_year": 2020,
    "end_year": null,
    "created_at": 1710288000,
    "updated_at": 1710288000,
    "aliases": ["alternate name"],
    "credits": [{ "id": 1, "display_name": "Artist Name", "names": [...] }],
    "tags": ["rock", "indie"],
    "relationships": [{ "artist_id_a": "...", "artist_id_b": "...", "role": "member_of", "begin_year": null, "end_year": null }]
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Artist not found |

---

### GET /v1/artists/{id}/feeds

Lists feeds attributed to an artist, paginated by title (ascending).

- **Authentication:** None
- **Query parameters:**
  - `medium` — optional feed medium filter; defaults to `music`

**Response (`200 OK`):** Paginated array of feed objects (same structure as `GET /v1/feeds/{guid}` without includes).

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Artist not found |

---

### GET /v1/artists/{id}/releases

Lists canonical releases attributed to an artist, paginated by normalized title
(ascending).

- **Authentication:** None

**Response (`200 OK`):** Paginated array of canonical release objects (same
top-level structure as `GET /v1/releases/{id}` without includes).

| Code | Meaning |
|------|---------|
| 200  | Success |

---

### GET /v1/artists/{id}/resolution

Returns review/debug evidence for one canonical artist.

This is an operator-facing inspection endpoint. It exposes:

- old artist IDs that redirect into this canonical artist
- canonical artist external IDs already promoted onto the artist row
- source feeds currently credited to the artist
- source tracks currently credited to the artist
- staged feed-level IDs, links, platform claims, and publisher `remoteItem` refs
- staged track-level IDs, links, and contributor claims
- canonical release mappings for those feeds when present
- canonical recording mappings for those tracks when present

- **Authentication:** None

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Artist not found |

---

## 5. Queries -- Feeds

### GET /v1/feeds/{guid}

Returns a single feed by its `podcast:guid`.

- **Authentication:** None
- **Include options:** `tracks`, `payment_routes`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_platforms`, `source_release_claims`, `remote_items`, `publisher`, `canonical`

**Response (`200 OK`):**

```json
{
  "data": {
    "feed_guid": "uuid",
    "feed_url": "https://...",
    "title": "Feed Title",
    "raw_medium": "music",
    "artist_credit": {
      "id": 1,
      "display_name": "Artist Name",
      "names": [
        { "artist_id": "uuid", "position": 0, "name": "Artist Name", "join_phrase": "" }
      ]
    },
    "description": "...",
    "image_url": "https://...",
    "language": "en",
    "explicit": false,
    "episode_count": 12,
    "newest_item_at": 1710288000,
    "oldest_item_at": 1700000000,
    "created_at": 1710288000,
    "updated_at": 1710288000,
    "tracks": [
      { "track_guid": "uuid", "title": "Track", "pub_date": 1710288000, "duration_secs": 240 }
    ],
    "payment_routes": [
      {
        "recipient_name": "Artist",
        "route_type": "keysend",
        "address": "02abc...",
        "custom_key": "7629169",
        "custom_value": "...",
        "split": 100,
        "fee": false
      }
    ],
    "tags": ["rock"],
    "source_links": [
      {
        "entity_type": "feed",
        "entity_id": "uuid",
        "position": 0,
        "link_type": "website",
        "url": "https://artist.example.com",
        "source": "rss_link",
        "extraction_path": "feed.link",
        "observed_at": 1710288000
      }
    ],
    "source_ids": [
      {
        "entity_type": "feed",
        "entity_id": "uuid",
        "position": 0,
        "scheme": "nostr_npub",
        "value": "npub1...",
        "source": "podcast_txt",
        "extraction_path": "feed.podcast:txt[@purpose='npub']",
        "observed_at": 1710288000
      }
    ],
    "source_contributors": [],
    "source_platforms": [
      {
        "platform_key": "wavlake",
        "url": "https://wavlake.com/feed/...",
        "owner_name": "Wavlake",
        "source": "feed_url",
        "extraction_path": "request.canonical_url",
        "observed_at": 1710288000
      }
    ],
    "source_release_claims": [
      {
        "entity_type": "feed",
        "entity_id": "uuid",
        "position": 0,
        "claim_type": "release_date",
        "claim_value": "1710288000",
        "source": "rss_pub_date",
        "extraction_path": "feed.pubDate",
        "observed_at": 1710288000
      }
    ],
    "remote_items": [
      {
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": "publisher-feed-guid",
        "remote_feed_url": "https://example.com/publisher.xml",
        "source": "podcast_remote_item"
      }
    ],
    "publisher": [
      {
        "direction": "music_to_publisher",
        "remote_feed_guid": "publisher-feed-guid",
        "remote_feed_url": "https://example.com/publisher.xml",
        "remote_feed_medium": "publisher",
        "publisher_feed_guid": "publisher-feed-guid",
        "publisher_feed_url": "https://example.com/publisher.xml",
        "music_feed_guid": "uuid",
        "music_feed_url": "https://...",
        "reciprocal_declared": true,
        "reciprocal_medium": "music",
        "two_way_validated": true,
        "artist_signal": "confirmed_artist"
      }
    ],
    "canonical": {
      "release_id": "release-uuid",
      "match_type": "exact_release_signature_v1",
      "confidence": 95
    }
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Feed not found |

`raw_medium` is the verbatim channel-level `podcast:medium` value from RSS.
`remote_items` is the stored source-truth snapshot of feed-level
`podcast:remoteItem` declarations.

For `musicL` container feeds, `raw_medium` is still stored and `remote_items`
remain visible, but local tracks are intentionally not materialized into the
`tracks` table or resolver layer.

`publisher` is a derived read-only view over those declarations. It
reports direction and reciprocal validation exactly from RSS, but only emits
`artist_signal` when the publisher feed and music feed already resolve to the
same single canonical artist. There is no speculative value for unreviewed or
unconfirmed cases.

---

### GET /v1/recent

Lists canonical releases ordered by recency. The sort key is:

1. newest mapped source-feed `newest_item_at`
2. otherwise canonical `release_date`
3. otherwise canonical `created_at`

Paginated with a composite `timestamp:release_id` cursor.

This endpoint is resolver-backed. Fresh ingests may not appear here until
`stophammer-resolverd` has drained the queue for those feeds.

- **Authentication:** None

**Response:** Paginated array of canonical release list objects.

| Code | Meaning |
|------|---------|
| 200  | Success |

---

### GET /v1/feeds/recent

Lists source feeds ordered by `newest_item_at` descending (most recently updated first). This preserves the older source-oriented recent view for provenance/debugging workflows.

- **Authentication:** None
- **Query parameters:**
  - `medium` — optional feed medium filter; defaults to `music`

**Response:** Paginated array of feed objects.

| Code | Meaning |
|------|---------|
| 200  | Success |

---

## 6. Queries -- Tracks

### GET /v1/tracks/{guid}

Returns a single track by its `track_guid`.

- **Authentication:** None
- **Include options:** `payment_routes`, `value_time_splits`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`, `canonical`

**Response (`200 OK`):**

```json
{
  "data": {
    "track_guid": "uuid",
    "feed_guid": "uuid",
    "title": "Track Title",
    "artist_credit": { "id": 1, "display_name": "...", "names": [...] },
    "pub_date": 1710288000,
    "duration_secs": 240,
    "enclosure_url": "https://example.com/track.mp3",
    "enclosure_type": "audio/mpeg",
    "enclosure_bytes": 3840000,
    "track_number": 1,
    "season": 1,
    "explicit": false,
    "description": "...",
    "created_at": 1710288000,
    "updated_at": 1710288000,
    "payment_routes": [...],
    "value_time_splits": [
      {
        "start_time_secs": 0,
        "duration_secs": 60,
        "remote_feed_guid": "uuid",
        "remote_item_guid": "uuid",
        "split": 50
      }
    ],
    "tags": ["electronic"],
    "source_links": [
      {
        "entity_type": "track",
        "entity_id": "uuid",
        "position": 0,
        "link_type": "web_page",
        "url": "https://artist.example.com/song",
        "source": "rss_link",
        "extraction_path": "entity.link",
        "observed_at": 1710288000
      }
    ],
    "source_ids": [],
    "source_contributors": [
      {
        "entity_type": "track",
        "entity_id": "uuid",
        "position": 0,
        "name": "Artist Name",
        "role": "Vocals",
        "role_norm": "vocals",
        "group_name": null,
        "href": null,
        "img": null,
        "source": "podcast_person",
        "extraction_path": "track.podcast:person[0]",
        "observed_at": 1710288000
      }
    ],
    "source_release_claims": [],
    "source_enclosures": [
      {
        "entity_type": "track",
        "entity_id": "uuid",
        "position": 0,
        "url": "https://example.com/track.mp3",
        "mime_type": "audio/mpeg",
        "bytes": 3840000,
        "rel": null,
        "title": null,
        "is_primary": true,
        "source": "enclosure",
        "extraction_path": "track.enclosure",
        "observed_at": 1710288000
      },
      {
        "entity_type": "track",
        "entity_id": "uuid",
        "position": 1,
        "url": "https://example.com/track.flac",
        "mime_type": "audio/flac",
        "bytes": 12000000,
        "rel": "alternate",
        "title": "Lossless",
        "is_primary": false,
        "source": "podcast_alternate_enclosure",
        "extraction_path": "track.podcast:alternateEnclosure[0]",
        "observed_at": 1710288000
      }
    ],
    "canonical": {
      "recording_id": "recording-uuid",
      "match_type": "exact_recording_signature_v1",
      "confidence": 95
    }
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

Notes:

- `artist_credit` is the normalized top-level track credit used by the source
  and canonical models. It is not the same thing as the staged contributor
  claim list.
- `source_contributors` is preserved RSS-truth evidence from Podcast Namespace
  `person` extraction and related source parsing.
- If a track has no track-level contributor claims of its own, the API falls
  back to the parent feed's contributor claims. The inherited rows keep their
  original `entity_type` / `entity_id`, so clients can tell whether the claim
  came from the track or the feed.
- Stophammer does not yet expose a canonical contributor graph for tracks or
  recordings. `source_contributors` is a staged evidence layer, not a resolved
  contributor-identity model.

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Track not found |

---

## 7. Queries -- Canonical Releases and Recordings

### GET /v1/releases/{id}

Returns a single canonical release by `release_id`.

This endpoint is resolver-backed. Fresh ingests may not expose a canonical
release immediately until `stophammer-resolverd` has rebuilt the feed's canonical state.
Source feed and track endpoints remain the immediate preserved-RSS layer.

- **Authentication:** None
- **Include options:** `tracks`, `sources`

**Response (`200 OK`):**

```json
{
  "data": {
    "release_id": "release-uuid",
    "title": "Release Title",
    "artist_credit": { "id": 1, "display_name": "Artist Name", "names": [...] },
    "description": "...",
    "image_url": "https://example.com/release.jpg",
    "release_date": 1710288000,
    "created_at": 1710288000,
    "updated_at": 1710288000,
    "tracks": [
      {
        "position": 1,
        "recording_id": "recording-uuid",
        "title": "Track Title",
        "duration_secs": 240,
        "source_track_guid": "track-guid"
      }
    ],
    "sources": [
      {
        "feed_guid": "feed-guid",
        "feed_url": "https://example.com/feed.xml",
        "title": "Release Title",
        "match_type": "exact_release_signature_v1",
        "confidence": 95,
        "platforms": ["wavlake"],
        "links": ["https://artist.example.com/release"]
      }
    ]
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

Notes:

- `artist_credit` is the normalized top-level release credit. It is not a full
  contributor list for the album.
- Stophammer does not yet expose a first-class canonical contributor graph on
  release entities.
- If a client needs contributor evidence for a release, the current path is:
  1. load the release's `tracks`
  2. inspect the mapped source tracks through `GET /v1/recordings/{id}/sources`
     with `include=source_contributors`
  3. or inspect individual source tracks through `GET /v1/tracks/{guid}?include=source_contributors`

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Release not found |

---

### GET /v1/releases/{id}/resolution

Returns review/debug evidence for one canonical release.

This is an operator-facing inspection endpoint. It exposes each mapped source
feed together with:

- `match_type`
- `confidence`
- staged source IDs
- staged links
- platform claims
- staged release claims
- feed-level `podcast:remoteItem` refs

Contributor claims are not exposed here because release resolution is currently
feed-based. Contributor evidence remains attached to source feed and source
track rows, not the canonical release entity itself.

- **Authentication:** None

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Release not found |

---

### GET /v1/releases/{id}/sources

Returns the mapped source feeds for one canonical release.

- **Authentication:** None
- **Include options:** same as `GET /v1/feeds/{guid}` (`tracks`, `payment_routes`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_platforms`, `source_release_claims`, `remote_items`, `publisher`, `canonical`)

Use this when a client wants the full source/platform rows behind one canonical
release instead of the lighter `sources` summary embedded in `GET /v1/releases/{id}`.

**Response:** paginated-style envelope with `data` as an array of feed objects.

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Release not found |

---

### GET /v1/recordings/{id}

Returns a single canonical recording by `recording_id`.

This endpoint is resolver-backed. Fresh ingests may not expose a canonical
recording immediately until `stophammer-resolverd` has rebuilt the feed's canonical state.
Source feed and track endpoints remain the immediate preserved-RSS layer.

- **Authentication:** None
- **Include options:** `sources`, `releases`

**Response (`200 OK`):**

```json
{
  "data": {
    "recording_id": "recording-uuid",
    "title": "Track Title",
    "artist_credit": { "id": 1, "display_name": "Artist Name", "names": [...] },
    "duration_secs": 240,
    "created_at": 1710288000,
    "updated_at": 1710288000,
    "sources": [
      {
        "track_guid": "track-guid",
        "feed_guid": "feed-guid",
        "title": "Track Title",
        "match_type": "exact_recording_signature_v1",
        "confidence": 95,
        "primary_enclosure_url": "https://example.com/track.mp3",
        "enclosure_urls": [
          "https://example.com/track.mp3",
          "https://example.com/track.flac"
        ],
        "links": ["https://artist.example.com/song"]
      }
    ],
    "releases": [
      {
        "release_id": "release-uuid",
        "title": "Release Title",
        "position": 1
      }
    ]
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

Notes:

- `artist_credit` is the normalized top-level recording credit, not a full
  contributor list.
- This endpoint does not expose staged contributor claims directly.
- To inspect contributor evidence for a recording, use
  `GET /v1/recordings/{id}/sources?include=source_contributors` or
  `GET /v1/recordings/{id}/resolution`.

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Recording not found |

---

### GET /v1/recordings/{id}/resolution

Returns review/debug evidence for one canonical recording.

This is an operator-facing inspection endpoint. It exposes each mapped source
track together with:

- `match_type`
- `confidence`
- staged source IDs
- staged links
- staged contributor claims
- staged release claims
- all known enclosure variants

- **Authentication:** None

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Recording not found |

---

### GET /v1/recordings/{id}/sources

Returns the mapped source tracks for one canonical recording.

- **Authentication:** None
- **Include options:** same as `GET /v1/tracks/{guid}` (`payment_routes`, `value_time_splits`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`, `canonical`)

Use this when a client wants full source-track detail, including all known
enclosure variants, for one canonical recording.

**Response:** paginated-style envelope with `data` as an array of track objects.

| Code | Meaning |
|------|---------|
| 200  | Success |
| 404  | Recording not found |

---

## 8. Queries -- Search

### GET /v1/search

Full-text search using SQLite FTS5.

Default search includes:

- `artist`
- `release`
- `recording`
- `feed`

Source `track` rows remain directly readable by ID and can still be requested
explicitly with `type=track`, but they are no longer part of the default public
search surface.

All search surfaces are resolver-backed now. Fresh ingests may not appear
under `artist`, `release`, `recording`, `feed`, or `track` search until
`stophammer-resolverd` has drained the queue for the touched feeds.

- **Authentication:** None

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `q` | string | **required** | Search query (FTS5 syntax) |
| `type` | string | artist/release/recording/feed | Filter by entity type: `artist`, `release`, `recording`, `feed`, `track` |
| `limit` | i64 | 20 | Max results (capped at 100) |
| `cursor` | string | none | Keyset pagination cursor |

**Response (`200 OK`):**

```json
{
  "data": [
    {
      "entity_type": "recording",
      "entity_id": "recording-guid",
      "rank": -1.5,
      "quality_score": 0
    }
  ],
  "pagination": { "cursor": "cursor-token", "has_more": true },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

| Code | Meaning |
|------|---------|
| 200  | Success |
| 400  | Invalid FTS5 query syntax |

---

### GET /v1/node/capabilities

Returns the node's capabilities, supported entity types, and valid `include` parameters.

- **Authentication:** None

**Response (`200 OK`):**

```json
{
  "api_version": "v1",
  "node_pubkey": "hex-pubkey",
  "capabilities": ["query", "search", "sync", "push"],
  "entity_types": ["artist", "feed", "track", "release", "recording"],
  "include_params": {
    "artist": ["aliases", "credits", "tags", "relationships"],
    "feed": ["tracks", "payment_routes", "tags", "source_links", "source_ids", "source_contributors", "source_platforms", "source_release_claims", "remote_items", "publisher", "canonical"],
    "track": ["payment_routes", "value_time_splits", "tags", "source_links", "source_ids", "source_contributors", "source_release_claims", "source_enclosures", "canonical"],
    "release": ["tracks", "sources"],
    "recording": ["sources", "releases"]
  }
}
```

---

### GET /v1/wallets/{id}

Returns one wallet entity, including normalized endpoints, historical aliases,
and any resolved artist links. If the requested wallet ID has been merged, the
endpoint follows the redirect and returns the surviving wallet.

- **Authentication:** None

**Response (`200 OK`):**

```json
{
  "data": {
    "wallet_id": "wallet-123",
    "display_name": "Alice",
    "wallet_class": "unknown",
    "class_confidence": "provisional",
    "endpoints": [
      {
        "id": 1,
        "route_type": "keysend",
        "normalized_address": "abc123",
        "custom_key": "7629169",
        "custom_value": "pod1"
      }
    ],
    "aliases": [
      {
        "alias": "Alice",
        "first_seen_at": 1710288000,
        "last_seen_at": 1710288000
      }
    ],
    "artist_links": [
      {
        "artist_id": "artist-123",
        "artist_name": "Alice",
        "confidence": "reviewed",
        "evidence_entity_type": "feed",
        "evidence_entity_id": "feed-guid"
      }
    ],
    "created_at": 1710288000,
    "updated_at": 1710288000
  },
  "meta": { "api_version": "v1", "node_pubkey": "hex-pubkey" }
}
```

| Code | Meaning |
|------|---------|
| 200  | Wallet found |
| 404  | Wallet ID not found |

Notes:

- `custom_key` and `custom_value` are returned as empty strings when the stored
  route omits them.
- `artist_links` is empty when no artist relationship has been resolved for the
  wallet yet.

### GET /v1/peers

Lists all known peer nodes from the `peer_nodes` table.

- **Authentication:** None

**Response (`200 OK`):**

```json
[
  {
    "node_pubkey": "hex-pubkey",
    "node_url": "http://community:8008/sync/push",
    "last_push_at": 1710288000
  }
]
```

---

## 9. Mutations -- Proof-of-Possession

The proof-of-possession flow allows feed owners to authorize mutations without an account system. It follows an ACME-inspired (RFC 8555) challenge-assert pattern. The feed owner publishes a `<podcast:txt>` element in their RSS feed containing a token binding, proving they control the feed URL.

### POST /v1/proofs/challenge

Creates a new proof-of-possession challenge.

- **Authentication:** None
- **Available on:** Primary only

**Request body:**

```json
{
  "feed_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "scope": "feed:write",
  "requester_nonce": "at-least-16-chars-random-string"
}
```

| Field | Constraints |
|-------|-------------|
| `scope` | Must be `"feed:write"` (only supported scope) |
| `requester_nonce` | 16--256 characters |

**Response (`201 Created`):**

```json
{
  "challenge_id": "uuid",
  "token_binding": "base64url-token.base64url-sha256-nonce-hash",
  "state": "pending",
  "expires_at": 1710374400
}
```

The feed owner must add a `<podcast:txt>` element to their RSS feed at channel level containing:

```
stophammer-proof <token_binding>
```

Challenges expire after 24 hours. Creating a new challenge for the same
`feed_guid` + `scope` invalidates any older pending challenge for that pair.
The server also enforces a global cap of 5,000 pending challenges.

| Code | Meaning |
|------|---------|
| 201  | Challenge created |
| 400  | Unsupported scope, nonce too short or too long |
| 404  | Feed not found in the database |
| 429  | Too many pending challenges globally (limit: 5,000) |

---

### POST /v1/proofs/assert

Asserts a previously created challenge. Fetches the RSS feed, verifies the `podcast:txt` element contains the token binding, and issues an access token on success.

- **Authentication:** None
- **Available on:** Primary only
- **SSRF protection:** Feed URLs targeting private/reserved IP ranges are rejected

**Request body:**

```json
{
  "challenge_id": "uuid",
  "requester_nonce": "the-same-nonce-from-challenge"
}
```

**Response (`200 OK`):**

```json
{
  "access_token": "base64url-128bit-token",
  "scope": "feed:write",
  "subject_feed_guid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "expires_at": 1710291600,
  "proof_level": "rss_only"
}
```

Access tokens expire after 1 hour.

| Code | Meaning |
|------|---------|
| 200  | Token issued |
| 400  | Nonce mismatch, feed URL rejected by SSRF validation, challenge already resolved, or `podcast:txt` not found |
| 404  | Challenge not found or expired |
| 409  | Feed URL changed during verification; retry the flow |
| 503  | RSS fetch failed |

---

## 10. Mutations -- PATCH

PATCH endpoints use RFC 7396 JSON Merge Patch semantics. They require either an admin token or a bearer token obtained through proof-of-possession.

### PATCH /v1/feeds/{guid}

Updates a feed's mutable fields. Currently supports `feed_url` only.

- **Authentication:** Admin token (`X-Admin-Token`) or Bearer token (`Authorization: Bearer <token>` with `feed:write` scope for this feed)
- **Available on:** Primary only

**Request body:**

```json
{
  "feed_url": "https://new-feed-url.example.com/feed.xml"
}
```

**Response:** `204 No Content` on success. Emits a `FeedUpserted` event and fans out to peers.

| Code | Meaning |
|------|---------|
| 204  | Updated |
| 401  | Missing `Authorization` header (with `WWW-Authenticate: Bearer realm="stophammer"`) |
| 403  | Invalid admin token, or bearer token scoped to a different feed |
| 404  | Feed not found |

---

### PATCH /v1/tracks/{guid}

Updates a track's mutable fields. Currently supports `enclosure_url` only. Bearer token scope is validated against the track's parent feed.

- **Authentication:** Admin token (`X-Admin-Token`) or Bearer token (`Authorization: Bearer <token>` with `feed:write` scope for the track's parent feed)
- **Available on:** Primary only

**Request body:**

```json
{
  "enclosure_url": "https://new-cdn.example.com/track.mp3"
}
```

**Response:** `204 No Content` on success. Emits a `TrackUpserted` event and fans out to peers.

| Code | Meaning |
|------|---------|
| 204  | Updated |
| 401  | Missing `Authorization` header |
| 403  | Invalid admin token, or bearer token scoped to a different feed |
| 404  | Track not found |

---

## 11. Admin and Diagnostics

Write-side admin endpoints require the `X-Admin-Token` header. The token is compared in constant time (SHA-256 hash comparison via `subtle::ConstantTimeEq`).

If `ADMIN_TOKEN` is not configured on the node, write-side admin endpoints return `403`.

### POST /admin/artists/merge

Merges two artist records. All feeds, tracks, credits, and aliases from the source artist are transferred to the target artist. The source artist is deleted.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Request body:**

```json
{
  "source_artist_id": "uuid-to-absorb",
  "target_artist_id": "uuid-to-keep"
}
```

**Response (`200 OK`):**

```json
{
  "merged": true,
  "events_emitted": ["uuid-of-artist-merged-event"]
}
```

| Code | Meaning |
|------|---------|
| 200  | Merged |
| 403  | Invalid or missing `X-Admin-Token` |

---

### POST /admin/artists/alias

Adds an alias to an artist (used for fuzzy matching during artist resolution).

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Request body:**

```json
{
  "artist_id": "uuid",
  "alias": "alternate spelling or name"
}
```

**Response (`200 OK`):**

```json
{
  "ok": true
}
```

| Code | Meaning |
|------|---------|
| 200  | Alias added |
| 403  | Invalid or missing `X-Admin-Token` |

---

### POST /admin/artist-identity/reviews/{id}/resolve

Applies one durable action to an artist-identity review item, then reruns the
feed-scoped artist resolver for that review's feed so the stored review state
converges immediately.

Supported actions:

- `merge` — requires `target_artist_id`
- `do_not_merge` — must not include `target_artist_id`

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Request body:**

```json
{
  "action": "merge",
  "target_artist_id": "uuid-to-keep",
  "note": "operator rationale"
}
```

**Response (`200 OK`):**

```json
{
  "review": {
    "review_id": 123,
    "feed_guid": "feed-guid",
    "source": "track_feed_name_variant",
    "name_key": "heycitizen",
    "evidence_key": "feed-guid",
    "status": "merged",
    "artist_ids": ["uuid-a", "uuid-b"],
    "artist_names": ["HeyCitizen", "Hey Citizen"],
    "override_type": "merge",
    "target_artist_id": "uuid-to-keep",
    "note": "operator rationale",
    "created_at": 1710288000,
    "updated_at": 1710288015
  },
  "resolve_stats": {
    "seed_artists": 2,
    "candidate_groups": 1,
    "groups_processed": 1,
    "merges_applied": 1,
    "merge_events_emitted": 0,
    "pending_reviews": 0,
    "blocked_reviews": 0
  }
}
```

| Code | Meaning |
|------|---------|
| 200  | Review action stored and feed-scoped resolver rerun |
| 400  | Unsupported action, or invalid `target_artist_id` usage for the chosen action |
| 403  | Invalid or missing `X-Admin-Token` |
| 404  | Review item not found |

---

### POST /admin/wallet-identity/reviews/{id}/resolve

Applies one durable action to a wallet-identity review item.

Supported actions:

- `merge` — requires `target_wallet_id`
- `do_not_merge` — must not include `target_wallet_id`, `target_artist_id`, or `value`
- `force_class` — requires `value`
- `force_artist_link` — requires `target_artist_id`
- `block_artist_link` — requires `target_artist_id`

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Request body:**

```json
{
  "action": "merge",
  "target_wallet_id": "wallet-id-to-keep"
}
```

**Response (`200 OK`):**

```json
{
  "review": {
    "id": 77,
    "wallet_id": "wallet-id-to-merge",
    "source": "cross_wallet_alias",
    "evidence_key": "shared wallet alias",
    "wallet_ids": ["wallet-id-to-keep", "wallet-id-to-merge"],
    "endpoint_summary": [],
    "status": "merged",
    "created_at": 1710288000,
    "updated_at": 1710288015
  }
}
```

| Code | Meaning |
|------|---------|
| 200  | Review action stored |
| 400  | Unsupported action, or invalid target/value fields for the chosen action |
| 403  | Invalid or missing `X-Admin-Token` |
| 404  | Review item not found |

---

### GET /admin/artist-identity/reviews/pending

Returns the current pending artist-identity review queue.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`)

**Response (`200 OK`):**

```json
{
  "reviews": [
    {
      "review_id": 123,
      "feed_guid": "feed-guid",
      "title": "Everything Is Lit",
      "source": "track_feed_name_variant",
      "confidence": "review_required",
      "explanation": "Feed and track artist credits collapse to the same normalized name on one feed but remain separate artist rows.",
      "name_key": "heycitizen",
      "evidence_key": "feed-guid",
      "artist_count": 2
    }
  ]
}
```

Each pending artist review also includes:

- `confidence`
- `explanation`

### GET /admin/artist-identity/reviews/pending/stale

Returns pending artist-identity review items older than `min_age_days`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`), `min_age_days` (default `7`)

### GET /admin/artist-identity/reviews/pending/recent

Returns pending artist-identity review items newer than `max_age_days`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`), `max_age_days` (default `1`)

### GET /admin/wallet-identity/reviews/pending

Returns the current pending wallet-identity review queue.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`)

**Response (`200 OK`):**

```json
{
  "reviews": [
    {
      "id": 77,
      "wallet_id": "wallet-id",
      "display_name": "Shared Wallet Alias",
      "wallet_class": "unknown",
      "class_confidence": "provisional",
      "source": "cross_wallet_alias",
      "confidence": "review_required",
      "explanation": "Multiple wallets share the same normalized alias across feed evidence, but ownership is still ambiguous.",
      "evidence_key": "shared wallet alias",
      "wallet_ids": ["wallet-a", "wallet-b"],
      "endpoint_summary": [],
      "created_at": 1710288000
    }
  ]
}
```

Each pending wallet review also includes:

- `confidence`
- `explanation`

### GET /admin/wallet-identity/reviews/pending/stale

Returns pending wallet-identity review items older than `min_age_days`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`), `min_age_days` (default `7`)

### GET /admin/wallet-identity/reviews/pending/recent

Returns pending wallet-identity review items newer than `max_age_days`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`), `max_age_days` (default `1`)

---

### GET /admin/artist-identity/reviews/pending/summary

Returns counts of pending artist-identity review items grouped by `source`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Response (`200 OK`):**

```json
{
  "summary": [
    { "source": "track_feed_name_variant", "count": 7 },
    { "source": "collaboration_credit", "count": 3 }
  ]
}
```

### GET /admin/wallet-identity/reviews/pending/summary

Returns counts of pending wallet-identity review items grouped by `source`.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Response (`200 OK`):**

```json
{
  "summary": [
    { "source": "cross_wallet_alias", "count": 12 }
  ]
}
```

---

### GET /admin/reviews/pending/age-summary

Returns age buckets for pending artist and wallet review queues.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only

**Response (`200 OK`):**

```json
{
  "artist_identity": {
    "total": 10,
    "created_last_24h": 4,
    "older_than_7d": 2,
    "oldest_created_at": 1710000000
  },
  "wallet_identity": {
    "total": 6,
    "created_last_24h": 1,
    "older_than_7d": 0,
    "oldest_created_at": 1710200000
  }
}
```

---

### GET /admin/reviews/dashboard

Returns the main operator dashboard payload for pending review work.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `hotspot_limit` (default `20`)

**Response (`200 OK`):**

```json
{
  "artist_identity_summary": [
    { "source": "track_feed_name_variant", "count": 7 }
  ],
  "wallet_identity_summary": [
    { "source": "cross_wallet_alias", "count": 12 }
  ],
  "age_summary": {
    "artist_identity": {
      "total": 10,
      "created_last_24h": 4,
      "older_than_7d": 2,
      "oldest_created_at": 1710000000
    },
    "wallet_identity": {
      "total": 6,
      "created_last_24h": 1,
      "older_than_7d": 0,
      "oldest_created_at": 1710200000
    }
  },
  "feed_hotspots": [
    {
      "feed_guid": "feed-guid",
      "title": "Everything Is Lit",
      "artist_review_count": 2,
      "wallet_review_count": 1,
      "total_review_count": 3
    }
  ]
}
```

---

### GET /admin/reviews/feeds/hotspots

Returns feeds ordered by combined pending artist and wallet review load.

- **Authentication:** Admin token (`X-Admin-Token`)
- **Available on:** Primary only
- **Query params:** `limit` (default `100`)

**Response (`200 OK`):**

```json
{
  "feeds": [
    {
      "feed_guid": "feed-guid",
      "title": "Everything Is Lit",
      "feed_url": "https://example.com/feed.xml",
      "artist_review_count": 2,
      "wallet_review_count": 1,
      "total_review_count": 3
    }
  ]
}
```

---

### GET /v1/diagnostics/feeds/{guid}

Returns a primary-only diagnostics bundle for one feed.

This endpoint is intended for public debugging tools, feed-author explainers,
and operator review UIs. It exposes the
current feed artist credit, track artist credits, feed-scoped artist identity
plan, stored review items, and wallet-linked evidence touching the feed.

- **Authentication:** None
- **Available on:** Primary only
- **Compatibility:** `/admin/diagnostics/feeds/{guid}` remains as a read-only alias for now

**Response (`200 OK`):**

```json
{
  "feed_guid": "feed-guid",
  "title": "Feed Title",
  "feed_url": "https://example.com/feed.xml",
  "artist_credit": {
    "id": 123,
    "display_name": "HeyCitizen",
    "names": [
      {
        "artist_id": "artist-id",
        "position": 0,
        "name": "HeyCitizen",
        "join_phrase": ""
      }
    ]
  },
  "tracks": [
    {
      "track_guid": "track-guid",
      "title": "Autistic Girl",
      "artist_credit": {
        "id": 456,
        "display_name": "Hey Citizen",
        "names": []
      }
    }
  ],
  "artist_identity_plan": {
    "feed_guid": "feed-guid",
    "seed_artists": [],
    "candidate_groups": [
      {
        "source": "wallet_name_variant",
        "name_key": "heycitizen",
        "evidence_key": "wallet-id",
        "artist_ids": ["artist-a", "artist-b"],
        "artist_names": ["HeyCitizen", "Hey Citizen"],
        "review_id": 42,
        "review_status": "pending",
        "override_type": null,
        "target_artist_id": null,
        "note": null
      }
    ]
  },
  "artist_identity_reviews": [],
  "wallets": [
    {
      "wallet": {
        "wallet_id": "wallet-id",
        "display_name": "HeyCitizen",
        "wallet_class": "unknown",
        "class_confidence": "provisional",
        "created_at": 1700000000,
        "updated_at": 1700000000,
        "endpoints": [],
        "aliases": [],
        "artist_links": [
          {
            "artist_id": "artist-id",
            "confidence": "high_confidence",
            "evidence_entity_type": "feed",
            "evidence_entity_id": "feed-guid"
          }
        ],
        "feed_guids": ["feed-guid"],
        "overrides": []
      },
      "claim_feed": {
        "feed_guid": "feed-guid",
        "title": "Feed Title",
        "feed_url": "https://example.com/feed.xml",
        "routes": [],
        "contributor_claims": [],
        "entity_id_claims": [],
        "link_claims": [],
        "release_claims": [],
        "platform_claims": []
      }
    }
  ]
}
```

Each `artist_identity_reviews` row includes deterministic review metadata:

- `confidence`
- `explanation`

| Code | Meaning |
|------|---------|
| 200  | Diagnostics returned |
| 404  | Feed not found |

---

### GET /v1/diagnostics/artists/{id}

Returns a primary-only diagnostics bundle for one artist.

This endpoint is intended for public debugging tools, feed-author explainers,
and operator review UIs. It exposes the
current surviving artist row, redirected source IDs, credits, feeds, tracks,
wallet links, unlinked feed-touching wallets, and feed-scoped review items
that currently involve the artist.

- **Authentication:** None
- **Available on:** Primary only
- **Compatibility:** `/admin/diagnostics/artists/{id}` remains as a read-only alias for now

**Response (`200 OK`):**

```json
{
  "requested_artist_id": "artist-id",
  "artist": {
    "artist_id": "artist-id",
    "name": "HeyCitizen"
  },
  "redirected_from": ["old-artist-id"],
  "credits": [],
  "feeds": [],
  "tracks": [],
  "wallets": [],
  "unlinked_feed_wallets": [],
  "reviews": [
    {
      "feed_guid": "feed-guid",
      "feed_title": "Feed Title",
      "review": {
        "source": "wallet_name_variant",
        "name_key": "heycitizen",
        "review_id": 42
      }
    }
  ]
}
```

`wallets` contains wallets directly linked to the artist through
`wallet_artist_links`.

`unlinked_feed_wallets` contains wallets seen on one or more of the artist's
feeds that are not currently linked to the artist. This is useful when a feed
has payment routes, but the resolver does not consider them attributable to the
artist.

| Code | Meaning |
|------|---------|
| 200  | Diagnostics returned |
| 404  | Artist not found |

---

### GET /v1/diagnostics/wallets/{id}

Returns a primary-only diagnostics bundle for one wallet.

This endpoint is intended for public debugging tools, feed-author explainers,
and operator review UIs. It exposes the
current surviving wallet row, redirected source IDs, wallet review rows,
claim-feed evidence, and alias peers that currently share one normalized alias.

- **Authentication:** None
- **Available on:** Primary only
- **Compatibility:** `/admin/diagnostics/wallets/{id}` remains as a read-only alias for now

**Response (`200 OK`):**

```json
{
  "requested_wallet_id": "wallet-id",
  "wallet": {
    "wallet_id": "wallet-id",
    "display_name": "Shared Wallet Alias",
    "wallet_class": "unknown",
    "class_confidence": "provisional",
    "artist_links": []
  },
  "redirected_from": ["old-wallet-id"],
  "reviews": [
    {
      "source": "cross_wallet_alias",
      "confidence": "review_required",
      "explanation": "Multiple wallets share the same normalized alias across feed evidence, but ownership is still ambiguous.",
      "evidence_key": "shared-wallet-alias",
      "status": "pending"
    }
  ],
  "claim_feeds": [],
  "alias_peers": []
}
```

Each wallet review row also includes:

- `confidence`
- `explanation`

| Code | Meaning |
|------|---------|
| 200  | Diagnostics returned |
| 404  | Wallet not found |

---

### DELETE /v1/feeds/{guid}

Retires a feed, cascade-deleting all its tracks, payment routes, and search index entries. Emits a `FeedRetired` event.

- **Authentication:** Admin token (`X-Admin-Token`) or Bearer token (`Authorization: Bearer <token>` with `feed:write` scope)
- **Available on:** Primary only

**Response:** `204 No Content`

| Code | Meaning |
|------|---------|
| 204  | Feed retired |
| 401  | Missing `Authorization` header |
| 403  | Invalid admin token or insufficient scope |
| 404  | Feed not found |

---

### DELETE /v1/feeds/{guid}/tracks/{track_guid}

Removes a single track from a feed. Emits a `TrackRemoved` event.

- **Authentication:** Admin token (`X-Admin-Token`) or Bearer token (`Authorization: Bearer <token>` with `feed:write` scope for the parent feed)
- **Available on:** Primary only

**Response:** `204 No Content`

| Code | Meaning |
|------|---------|
| 204  | Track removed |
| 401  | Missing `Authorization` header |
| 403  | Invalid admin token or insufficient scope |
| 404  | Track not found, or track does not belong to the specified feed |

---

## 12. SSE Events

### GET /v1/events

Server-Sent Events stream for real-time notifications. Subscribe to events for specific artist IDs.

- **Authentication:** None
- **Available on:** Primary and community

**Query parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `artists` | string | Comma-separated list of artist IDs to follow (max 50) |

**Headers:**

| Header | Description |
|--------|-------------|
| `Last-Event-ID` | Resume from the last received event (replays from ring buffer) |

**SSE frame format:**

```
event: track_upserted
id: track-guid
data: {"event_type":"track_upserted","subject_guid":"track-guid","payload":{...}}
```

**Limits:**

- Max 1,000 concurrent SSE connections server-wide
- Max 50 artist IDs per connection
- Max 10,000 unique artist entries in the SSE registry
- Ring buffer: 100 most recent events per artist (for `Last-Event-ID` replay)
- Keep-alive: every 30 seconds

| Code | Meaning |
|------|---------|
| 200  | SSE stream opened |
| 503  | Too many concurrent SSE connections |

---

## Event Types

Events are the atomic unit of replication. Each event is ed25519-signed by the primary node.

| Event Type | Subject GUID | Description |
|------------|-------------|-------------|
| `feed_upserted` | feed_guid | Feed created or metadata updated |
| `feed_retired` | feed_guid | Feed permanently removed |
| `track_upserted` | track_guid | Track created or metadata/routes changed |
| `track_removed` | track_guid | Track deleted from a feed |
| `artist_upserted` | artist_id | Artist created or display name changed |
| `routes_replaced` | track_guid | Track payment routes atomically replaced |
| `artist_merged` | target_artist_id | Two artists merged |
| `artist_credit_created` | artist_id | Multi-artist credit created |
| `feed_routes_replaced` | feed_guid | Feed-level payment routes replaced |
| `feed_remote_items_replaced` | feed_guid | Feed-level `podcast:remoteItem` snapshot replaced |
| `live_events_replaced` | feed_guid | Feed-level live-item snapshot replaced |
| `source_contributor_claims_replaced` | feed_guid | Feed-level staged contributor claims replaced |
| `source_entity_ids_replaced` | feed_guid | Feed-level staged entity IDs replaced |
| `source_entity_links_replaced` | feed_guid | Feed-level staged entity links replaced |
| `source_release_claims_replaced` | feed_guid | Feed-level staged release claims replaced |
| `source_item_enclosures_replaced` | feed_guid | Feed-level staged item enclosure snapshot replaced |
| `source_platform_claims_replaced` | feed_guid | Feed-level staged platform claims replaced |

---

## Authentication Summary

| Method | Header / Field | Used By |
|--------|---------------|---------|
| Crawl token | `crawl_token` in request body | `POST /ingest/feed` |
| Sync token | `X-Sync-Token` header | `GET /sync/events`, `GET /sync/peers`, `POST /sync/register`, `POST /sync/reconcile` |
| Admin token | `X-Admin-Token` header | write-side `/admin/*`, `DELETE /v1/feeds/*`, `DELETE /v1/feeds/*/tracks/*`, `PATCH /v1/feeds/*`, `PATCH /v1/tracks/*` |
| Bearer token | `Authorization: Bearer <token>` | `DELETE /v1/feeds/{guid}`, `DELETE /v1/feeds/{guid}/tracks/{track_guid}`, `PATCH /v1/feeds/{guid}`, `PATCH /v1/tracks/{guid}` |

Bearer tokens are obtained through the proof-of-possession flow (`POST /v1/proofs/challenge` + `POST /v1/proofs/assert`). They are scoped to a specific feed and expire after 1 hour.

When both `X-Admin-Token` and `Authorization: Bearer` are present, the admin token takes precedence.

RFC 6750 compliance: `401 Unauthorized` responses include a `WWW-Authenticate: Bearer realm="stophammer"` header. `403 Forbidden` for scope violations includes `WWW-Authenticate: Bearer realm="stophammer", error="insufficient_scope"`.
