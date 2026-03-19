# User Guide

This guide is the shortest path to understanding what `stophammer` does, who it
is for, and how to use it without reading the whole repo.

## What Stophammer Is

`stophammer` is a quality-gated V4V music index.

It accepts music RSS feeds from crawlers, verifies that they look like real
Podcasting 2.0 music feeds, stores the accepted data in SQLite, and republishes
the accepted mutations as a signed event log.

It is not:

- a general podcast directory
- a web crawler by itself
- a Lightning payment sender
- a generic metadata warehouse for every audio feed on the internet

It is specifically for feeds that declare `podcast:medium=music` and carry
usable V4V payment routes.

## Who Should Read What

If you are:

- an app developer: start here, then read [API.md](/home/citizen/build/stophammer/docs/API.md)
- a primary/community operator: start here, then read [operations.md](/home/citizen/build/stophammer/docs/operations.md)
- a data curator/reviewer: start here, then read [schema-reference.md](/home/citizen/build/stophammer/docs/schema-reference.md)
- a verifier maintainer: start here, then read [verifier-guide.md](/home/citizen/build/stophammer/docs/verifier-guide.md)

## The Basic Mental Model

There are three moving parts:

1. Crawlers discover or replay feeds and submit them to a primary node.
2. The primary verifies and stores accepted feeds, then signs resulting events.
3. Community nodes replicate those signed events and serve the same read API.

There are also two data layers inside the database:

- source layer: what specific feeds and items claimed
- canonical layer: what `stophammer` currently believes are the merged
  artists, releases, and recordings

That split matters because user-facing discovery should usually be canonical,
while review and debugging often need the original source facts.

## Common Roles and Workflows

### I want to run a primary

Read the primary sections in [README.md](/home/citizen/build/stophammer/README.md)
and [operations.md](/home/citizen/build/stophammer/docs/operations.md).

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

### I want to run a community replica

Read the community sections in [README.md](/home/citizen/build/stophammer/README.md)
and [operations.md](/home/citizen/build/stophammer/docs/operations.md).

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

Start with [API.md](/home/citizen/build/stophammer/docs/API.md).

The public read surface is now canonical-first:

- `/v1/search` returns canonical `artist`, `release`, and `recording` results by default
- `/v1/recent` returns canonical releases
- `/v1/releases/{id}` and `/v1/recordings/{id}` are the main detail endpoints
- `/v1/releases/{id}/sources` and `/v1/recordings/{id}/sources` expose source/platform drill-down

Source endpoints still matter:

- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`

Those are useful for provenance, source claims, platform links, and audit views.

### I want to inspect or repair canonical data

The shipped utility binaries are:

- `resolverd`
- `resolverctl`
- `backfill_canonical`
- `backfill_artist_identity`
- `review_artist_identity`

See:

- [operations.md](/home/citizen/build/stophammer/docs/operations.md)
- [resolverd.1](/home/citizen/build/stophammer/man/resolverd.1)
- [resolverctl.1](/home/citizen/build/stophammer/man/resolverctl.1)
- [review_artist_identity.1](/home/citizen/build/stophammer/man/review_artist_identity.1)
- [backfill_canonical.1](/home/citizen/build/stophammer/man/backfill_canonical.1)
- [backfill_artist_identity.1](/home/citizen/build/stophammer/man/backfill_artist_identity.1)

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

### Run the canonical backfill

```bash
cargo run --bin backfill_canonical -- --db ./stophammer.db
```

### Run the resolver worker

```bash
cargo run --bin resolverd
```

### Pause resolver draining during a bulk import

```bash
cargo run --bin resolverctl -- import-active
# run import
cargo run --bin resolverctl -- import-idle
```

If you are using the bundled crawler importer on the same host, set
`RESOLVER_DB_PATH=/path/to/stophammer.db` and it will bracket the import and
refresh the pause heartbeat automatically.

Promoted artist IDs and canonical source rows are now background-derived. If
you ingest fresh data and immediately inspect promoted `external_ids` or
`entity_source`, run `resolverd` or wait for it to drain the queue first.

### Review ambiguous artist splits

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --limit 20
cargo run --bin review_artist_identity -- --db ./stophammer.db --feed-guid feed-guid-here
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-feeds --limit 20
```

### Run the strict local quality gate

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --tests
```

## Documentation Map

Read next depending on what you need:

- quick architecture and startup: [README.md](/home/citizen/build/stophammer/README.md)
- operator deployment and maintenance: [operations.md](/home/citizen/build/stophammer/docs/operations.md)
- HTTP routes and payloads: [API.md](/home/citizen/build/stophammer/docs/API.md)
- schema and source/canonical tables: [schema-reference.md](/home/citizen/build/stophammer/docs/schema-reference.md)
- verifier chain behavior: [verifier-guide.md](/home/citizen/build/stophammer/docs/verifier-guide.md)
- architecture history: [docs/adr](/home/citizen/build/stophammer/docs/adr)
- wiki-style navigation: [wiki/Home.md](/home/citizen/build/stophammer/docs/wiki/Home.md)
- manpages: [man](/home/citizen/build/stophammer/man)
