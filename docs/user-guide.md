# User Guide

This guide is the shortest path to understanding what `stophammer` does, who it
is for, and how to use it without reading the whole repo.

## What Stophammer Is

`stophammer` is a quality-gated V4V music index with a small source-layer
exception for container feeds.

It accepts music RSS feeds from crawlers, verifies that they look like real
Podcasting 2.0 music feeds, stores the accepted data in SQLite, and republishes
the accepted mutations as a signed event log.

It is not:

- a general podcast directory
- a web crawler by itself
- a Lightning payment sender
- a generic metadata warehouse for every audio feed on the internet

It is primarily for feeds that declare `podcast:medium=music` and carry usable
V4V payment routes.

It also preserves `publisher` and `musicL` container feeds as source-layer API
data. Those container feeds remain visible in the source-first v1 model, but
they do not produce a separate canonical release/recording layer.

## Who Should Read What

If you are:

- an app developer: start here, then read [API.md](API.md)
- a primary/community operator: start here, then read [operations.md](operations.md)
- a data curator/reviewer: start here, then read [schema-reference.md](schema-reference.md)
- a verifier maintainer: start here, then read [verifier-guide.md](verifier-guide.md)

## The Basic Mental Model

There are three moving parts:

1. Crawlers discover or replay feeds and submit them to a primary node.
2. The primary verifies and stores accepted feeds, then signs resulting events.
3. Community nodes replicate those signed events and serve the same read API.

There are also two practical layers inside the database:

- source-first feed and track rows used by the public API
- preserved source evidence such as links, IDs, contributors, remote items,
  platform claims, and enclosure variants

The old canonical release/recording public layer has been retired. Review and
debugging now happen directly against the source-first rows and preserved
evidence.

## Common Roles and Workflows

### I want to run a primary

Read the primary sections in [README.md](../README.md)
and [operations.md](operations.md).

You will need:

- a persistent `DB_PATH`
- a persistent `KEY_PATH`
- `CRAWL_TOKEN`
- `SYNC_TOKEN`
- optional `ADMIN_TOKEN`

The primary:

- accepts `POST /ingest/feed`
- signs accepted events
- fans out sync traffic to community nodes
- serves the full read API

If you need to keep a known-bad feed out of the index, add `feed_blocklist`
to `VERIFIER_CHAIN` and set exact-match `BLOCKED_FEED_GUIDS` and/or
`BLOCKED_FEED_URLS` on the primary. Community nodes follow the primary's
decision and do not re-run verifiers locally.

### I want to run a community replica

Read the community sections in [README.md](../README.md)
and [operations.md](operations.md).

You will need:

- `NODE_MODE=community`
- `PRIMARY_URL`
- `NODE_ADDRESS`
- `SYNC_TOKEN`
- a persistent `DB_PATH`
- a persistent `KEY_PATH`

Community nodes do not ingest feeds directly. They replicate signed events from
the primary and serve a read-only version of the API.

### I want to build a client

Start with [API.md](API.md).

The public read surface is source-first:

- `/v1/search`
- `/v1/feeds/recent`
- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`

Source endpoints are the main endpoints:

- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`

Those are useful for provenance, source claims, platform links, and audit views.
`GET /v1/feeds/{guid}?include=remote_items,publisher` is the RSS-truth
debug view for publisher relationships. The `publisher` include reports
direction and reciprocal validation directly from RSS. For stored
`publisher_text`, non-Wavlake feeds only promote a publisher title after a
reciprocal publisher/music remote-item pair is present. Wavlake is the narrow
exception where the linked publisher feed may also provide artist text while
the stored publisher remains `"Wavlake"`.

### I want to inspect or repair canonical data

The Phase 1 resolver retirement removed the dedicated resolver, backfill, and
review binaries. For now, inspect source-truth state through the main HTTP API
and keep schema/canonical planning in the vision and ADR documents:

- [operations.md](operations.md)
- [v4v-music-metadata-vision.md](vision/v4v-music-metadata-vision.md)
- [0032-retire-resolver-and-review-runtime.md](adr/0032-retire-resolver-and-review-runtime.md)

## The Data Model in One Page

The main source entities are:

- `feed`
- `track`
- staged source claims such as links, IDs, contributors, release claims,
  platform claims, and enclosure variants

The main canonical entities are:

- `artist`
- `release`
- `recording`

Mappings connect the two:

- `source_feed_release_map`
- `source_item_recording_map`

So the general user flow is:

1. discover a canonical release or recording
2. inspect source/platform variants for that canonical entity
3. inspect source claims if something looks wrong

## Everyday Commands

### Build the main binary

```bash
cargo build --release
./target/release/stophammer
```

### Start a primary locally

```bash
DB_PATH=./stophammer.db \
KEY_PATH=./signing.key \
CRAWL_TOKEN=change-me \
SYNC_TOKEN=change-me \
BIND=127.0.0.1:8008 \
./target/release/stophammer
```

### Inspect the node pubkey

```bash
curl http://127.0.0.1:8008/node/info
```

### Resolver-era maintenance tools

Resolver, backfill, and review binaries were retired. The current source-first
runtime keeps feed, track, and source-claim rows directly in the main node
database and exposes them through the main HTTP API.

For a quick source/search load check:

```bash
FEED_GUID=feed-guid-here ./tests/load_test.sh
FEED_GUID=feed-guid-here SEARCH_QUERY=artist-name ./tests/load_test.sh
```

### Run the strict local quality gate

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --tests
```

## Documentation Map

Read next depending on what you need:

- quick architecture and startup: [README.md](../README.md)
- operator deployment and maintenance: [operations.md](operations.md)
- HTTP routes and payloads: [API.md](API.md)
- schema and source/canonical tables: [schema-reference.md](schema-reference.md)
- verifier chain behavior: [verifier-guide.md](verifier-guide.md)
- architecture history: [docs/adr](adr)
- wiki-style navigation: [wiki/Home.md](wiki/Home.md)
- manpages: [man](../man)
