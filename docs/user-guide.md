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
data. Those container feeds do not participate in resolver-driven canonical
output.

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

There are also two data layers inside the database:

- source layer: what specific feeds and items claimed
- canonical layer: what `stophammer` currently believes are the merged
  artists, releases, and recordings

That split matters because user-facing discovery should usually be canonical,
while review and debugging often need the original source facts.

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

The public read surface is now canonical-first:

- `/v1/search` returns canonical `artist`, `release`, and `recording` results by default
- `/v1/recent` returns canonical releases
- `/v1/releases/{id}` and `/v1/recordings/{id}` are the main detail endpoints
- `/v1/releases/{id}/sources` and `/v1/recordings/{id}/sources` expose source/platform drill-down

Source endpoints still matter:

- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`

Those are useful for provenance, source claims, platform links, and audit views.
`GET /v1/feeds/{guid}?include=remote_items,publisher` is the RSS-truth
debug view for publisher relationships. It only shows publisher-as-artist
signals after identity is actually confirmed; it does not expose speculative
guesses for unresolved feeds.

### I want to inspect or repair canonical data

The shipped utility binaries are:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- `backfill_canonical`
- `backfill_artist_identity`
- `review_artist_identity`
- `review_artist_identity_tui`
- `backfill_wallets`
- `review_wallet_identity`
- `review_wallet_identity_tui`
- `review_source_claims_tui`

`review_source_claims_tui` is no longer just a raw evidence browser. It now
has an operator workflow layer for:

- queue overview
- backlog playbook
- feed hotspots
- selected-feed summary
- selected-track claim-family mix
- selected-feed conflicts
- selected-feed claim-family mix
- same-family feed and track jumps

See:

- [operations.md](operations.md)
- [stophammer-resolverd.1](../man/stophammer-resolverd.1)
- [stophammer-resolverctl.1](../man/stophammer-resolverctl.1)
- [review_artist_identity.1](../man/review_artist_identity.1)
- [review_artist_identity_tui.1](../man/review_artist_identity_tui.1)
- [backfill_canonical.1](../man/backfill_canonical.1)
- [backfill_artist_identity.1](../man/backfill_artist_identity.1)
- [backfill_wallets.1](../man/backfill_wallets.1)
- [review_wallet_identity.1](../man/review_wallet_identity.1)
- [review_wallet_identity_tui.1](../man/review_wallet_identity_tui.1)
- [review_source_claims_tui.1](../man/review_source_claims_tui.1)

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

This automatically coordinates with `stophammer-resolverd` via
`resolver_state.backfill_active` while it runs.

### Run the resolver worker

```bash
cargo run --bin stophammer-resolverd
```

Run `stophammer-resolverd` on the primary only. Community nodes now follow the signed
resolved-state events emitted by the primary and should not run their own
resolver worker.

If you need to disable resolved-state event emission temporarily, use
`RESOLVER_EMIT_RESOLVED_STATE_EVENTS=false`.

### Pause resolver draining during a bulk import

```bash
cargo run --bin stophammer-resolverctl -- import-active
# run import
cargo run --bin stophammer-resolverctl -- import-idle
```

If you are using the bundled crawler importer on the same host, set
`RESOLVER_DB_PATH=/path/to/stophammer.db` and it will bracket the import and
refresh the pause heartbeat automatically.

The backfill binaries coordinate with `stophammer-resolverd` automatically and do not
need manual `stophammer-resolverctl import-active` / `import-idle` bracketing.

Promoted artist IDs, source feed/track search rows, source quality scores, and
canonical source rows are now background-derived from the primary resolver. If
you ingest fresh data and immediately inspect `/v1/search`, `/v1/recent`,
canonical releases, canonical recordings, promoted `external_ids`, or
`entity_source`, run `stophammer-resolverd` on the primary or wait for it to drain the
queue first.

You can check that backlog directly:

```bash
curl http://127.0.0.1:8008/v1/resolver/status
```

Look at:

- `resolver.caught_up`
- `resolver.queue.total`
- `resolver.queue.failed`

That endpoint also tells you which API surfaces are immediate source-layer
reads and which are resolver-backed canonical views.

The original feed/track data and staged source claims remain the preserved RSS
layer. `stophammer-resolverd` enriches canonical views on top of that data; it does not
replace the source rows.

For a quick resolver-aware load check:

```bash
FEED_GUID=feed-guid-here ./tests/load_test.sh
FEED_GUID=feed-guid-here SEARCH_QUERY=artist-name WAIT_FOR_RESOLVER=1 ./tests/load_test.sh
```

That script treats source reads and resolver-backed search as separate layers.

### Review ambiguous artist splits

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --limit 20
cargo run --bin review_artist_identity -- --db ./stophammer.db --feed-guid feed-guid-here
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-feeds --limit 20
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-reviews --limit 20
cargo run --bin review_artist_identity -- --db ./stophammer.db --show-review 17
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --merge-review 17 --target-artist artist-123 --note "same artist, operator confirmed"
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --reject-review 17 --note "different projects sharing one name"
```

Stored review items let you keep the automatic resolver conservative. It can
continue merging deterministic cases while ambiguous feed-scoped candidate
groups get a durable review row and an optional operator override.

Pending artist review rows also expose deterministic review metadata:

- `confidence`
- `explanation`
- `supporting_sources` for scored sources such as `likely_same_artist`
- `score`
- `score_breakdown`

Inside `review_artist_identity_tui`, use:

- `n/N` to move within one source family
- `g/G` to jump through `high_confidence` review items first
- `H` to open a high-confidence-only review list
- queue summary / overview dialogs to inspect score bands
- selected review panes to inspect `score_breakdown`

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
