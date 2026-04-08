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

Resolver status, diagnostics, and review endpoints were retired in Phase 1 of
the v4v music metadata refactor. This reference documents the surviving HTTP
surface.

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

## 5. Queries -- Feeds

### GET /v1/feeds/{guid}

Returns a single feed by its `podcast:guid`.

- **Authentication:** None
- **Include options:** `tracks`, `payment_routes`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_platforms`, `source_release_claims`, `remote_items`, `publisher`

**Response (`200 OK`):**

```json
{
  "data": {
    "feed_guid": "uuid",
    "feed_url": "https://...",
    "title": "Feed Title",
    "raw_medium": "music",
    "release_artist": "Artist Name",
    "release_artist_sort": null,
    "release_date": 1710288000,
    "release_kind": "unknown",
    "description": "...",
    "image_url": "https://...",
    "publisher_text": "Wavlake",
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
    ]
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

`GET /v1/feeds/recent` is the public recency listing for source-first v1.
The older canonical `/v1/recent` route has been retired.

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
- **Include options:** `payment_routes`, `value_time_splits`, `tags`, `source_links`, `source_ids`, `source_contributors`, `source_release_claims`, `source_enclosures`

**Response (`200 OK`):**

```json
{
  "data": {
    "track_guid": "uuid",
    "feed_guid": "uuid",
    "title": "Track Title",
    "track_artist": "Artist Name",
    "track_artist_sort": null,
    "pub_date": 1710288000,
    "duration_secs": 240,
    "image_url": "https://example.com/track.jpg",
    "language": "en",
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
    ]
  },
  "pagination": { "cursor": null, "has_more": false },
  "meta": { "api_version": "v1", "node_pubkey": "..." }
}
```

Notes:

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

## 7. Queries -- Search

### GET /v1/search

Full-text search using SQLite FTS5.

Default search includes:

- `feed`
- `track`

Search is source-first in the current runtime. Feed and track search results
align with the same public IDs exposed by the direct read endpoints.

- **Authentication:** None

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `q` | string | **required** | Search query (FTS5 syntax) |
| `type` | string | feed/track | Filter by entity type: `feed`, `track` |
| `limit` | i64 | 20 | Max results (capped at 100) |
| `cursor` | string | none | Keyset pagination cursor |

**Response (`200 OK`):**

```json
{
  "data": [
    {
      "entity_type": "track",
      "entity_id": "track-guid",
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
  "entity_types": ["feed", "track", "wallet"],
  "include_params": {
    "feed": ["tracks", "payment_routes", "tags", "source_links", "source_ids", "source_contributors", "source_platforms", "source_release_claims", "remote_items", "publisher"],
    "track": ["payment_routes", "value_time_splits", "tags", "source_links", "source_ids", "source_contributors", "source_release_claims", "source_enclosures"]
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
        "evidence_entity_type": "feed_alias",
        "evidence_entity_id": "feed-guid",
        "evidence_explanation": "wallet alias exactly matched the feed artist credit on the same feed"
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
- `artist_links[*].evidence_entity_type` currently distinguishes direct
  same-feed alias matches (`feed_alias`) from dominant non-Wavlake route
  matches (`feed_dominant_route`).
- `artist_links[*].evidence_explanation` provides a short human-readable reason
  for that link provenance.

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

### Retired Resolver/Review Endpoints

The resolver status endpoint plus the review and diagnostics endpoints were
removed during Phase 1 resolver retirement. They no longer exist in the
runtime API.

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
