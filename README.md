# Stophammer

A quality-gated V4V music index with preserved source-layer container feeds.

## Documentation

- quick start and architecture: this README
- narrative introduction: [docs/user-guide.md](docs/user-guide.md)
- operator deployment and maintenance: [docs/operations.md](docs/operations.md)
- cross-cutting security and trust-boundary rules:
  [docs/security-guidelines.md](docs/security-guidelines.md)
- HTTP API reference: [docs/API.md](docs/API.md)
- schema and source/canonical model: [docs/schema-reference.md](docs/schema-reference.md)
- resolver refactor plan and phases: [docs/resolver-refactor-plan.md](docs/resolver-refactor-plan.md)
- packaging and distribution plan: [docs/packaging-plan.md](docs/packaging-plan.md)
- verifier behavior: [docs/verifier-guide.md](docs/verifier-guide.md)
- wiki-style navigation: [docs/wiki/Home.md](docs/wiki/Home.md)
- manpages:
  - [man/stophammer.1](man/stophammer.1)
  - [man/stophammer-resolverd.1](man/stophammer-resolverd.1)
  - [man/stophammer-resolverctl.1](man/stophammer-resolverctl.1)
  - [man/backfill_canonical.1](man/backfill_canonical.1)
  - [man/backfill_artist_identity.1](man/backfill_artist_identity.1)
  - [man/review_artist_identity.1](man/review_artist_identity.1)
  - [man/review_artist_identity_tui.1](man/review_artist_identity_tui.1)
  - [man/backfill_wallets.1](man/backfill_wallets.1)
  - [man/review_wallet_identity.1](man/review_wallet_identity.1)
  - [man/review_wallet_identity_tui.1](man/review_wallet_identity_tui.1)
  - [man/review_source_claims_tui.1](man/review_source_claims_tui.1)

## Ecosystem

| Repository | Description |
|---|---|
| **[stophammer](https://github.com/dardevelin/stophammer)** | Primary / community node (this repo) |
| [stophammer-crawler](https://github.com/dardevelin/stophammer-crawler) | Unified feed crawler — one-shot crawl, PodcastIndex import, and gossip-listener SSE/archive ingestion |
| [stophammer-parser](https://github.com/dardevelin/stophammer-parser) | Declarative RSS/Podcast XML extraction engine (Rust library) |
| [stophammer-tracker](https://github.com/dardevelin/stophammer-tracker) | Cloudflare Workers peer tracker (optional bootstrap) |

## The problem

The Lightning Network enables direct music payments — listeners send sats to artists
as they listen, DJs split payments to the tracks they play. This works beautifully
*if* you can look up a track and trust that its payment routes are real and working.

PodcastIndex is the de-facto feed directory for Podcasting 2.0. It's comprehensive,
but it doesn't enforce V4V participation — it indexes every podcast regardless of
whether it has payment routes, whether those routes are valid, or whether it's
actually music.

## What Stophammer is

Stophammer is a **verified V4V music index**. Its primary acceptance target is
RSS feeds that:

- declare `podcast:medium=music`
- carry at least one structurally valid `podcast:value` payment route
  (non-empty address with a positive split — the verifier checks metadata
  presence, not Lightning node reachability or payment delivery)

It also preserves two container/source-layer mediums:

- `publisher` feeds, which group or reference music feeds
- `musicL` feeds, which act as playlist/container feeds for remote public items

Those container feeds are stored for source-truth API use, but they do not
participate in resolver-driven canonical output.

Every entry has been crawled and passed through a verifier chain. The index is an
**append-only signed event log** — you can verify the integrity of every feed
addition, replicate the full index to your own node, and serve it locally with no
dependency on a central server.

## Who it's for

- **App developers** building V4V music players or DJ tools that need a trustworthy
  source of feed GUIDs and payment routes
- **Node operators** who want a local, independently-verifiable copy of the index
- **Contributors** running crawlers (one-shot crawl, gossip real-time/archive replay, or PodcastIndex bulk
  import) to grow the index

## Architecture

```
[crawler]  →  POST /ingest/feed  →  [primary]  →  POST /sync/push  →  [community nodes]
                                        ↑
                                   verifier chain
                                   signs events
                                   GET /sync/peers   ← primary is its own tracker
```

- **Primary node** — Rust + SQLite. Crawlers POST feeds to `/ingest/feed`.
  A verifier chain gates acceptance. Accepted feeds are written atomically and
  recorded in a signed append-only event log. On each commit the primary
  immediately fans out the new events to all registered community nodes.
- **Community nodes** — receive pushed events from the primary, verify the
  ed25519 signature, apply idempotently. Fall back to polling if no push
  arrives within `PUSH_TIMEOUT_SECS` (default 90s). Register their push URL
  with the primary on startup.
- **Primary as tracker** — authenticated `GET /sync/peers` on the primary
  returns all known community nodes. A new node only needs the primary URL
  plus sync credentials to bootstrap.
  The Cloudflare tracker worker is optional and only needed before the primary
  URL is publicly known.
- **Crawlers** — independent untrusted processes, authenticated by `CRAWL_TOKEN`.
  Stophammer does **not** run or schedule crawlers — that is the operator's
  responsibility (cron, systemd timer, external service). Current crawler
  implementations: one-shot `crawl`, long-running `gossip`, and PodcastIndex
  snapshot `import`.
  Treat crawlers as the SSRF-exposed fetch tier: they should be low-privilege,
  network-restricted processes that can reach public feed hosts and the primary's ingest
  endpoint, but not arbitrary internal services or primary secrets.

## Verifier chain

Every ingest runs through an ordered chain of verifiers on the **primary only**.
Community nodes verify the ed25519 signature and trust the result — they do not
re-run verifiers. The verifier warnings from the primary are stored in each event
as an audit trail and replicated to all nodes.

Default chain:

```
crawl_token → content_hash → feed_blocklist → medium_music → feed_guid → v4v_payment → enclosure_type
```

| Verifier | Effect |
|---|---|
| `crawl_token` | Rejects invalid crawl tokens |
| `content_hash` | Short-circuits unchanged feeds (no DB write, no event) |
| `feed_blocklist` | Rejects exact-match blocked feed GUIDs and URLs from `BLOCKED_FEED_GUIDS` / `BLOCKED_FEED_URLS` |
| `medium_music` | Rejects feeds whose `podcast:medium` is absent or outside the accepted set: `music`, `publisher`, `musicL` |
| `feed_guid` | Rejects malformed or known-bad `podcast:guid` values |
| `v4v_payment` | Rejects `music` feeds with no structurally valid V4V payment routes (container mediums `publisher` and `musicL` are exempt) |
| `enclosure_type` | Warns on video MIME type enclosures |

Override at runtime with `VERIFIER_CHAIN=crawl_token,content_hash,...`.
Changing the chain on a primary does not affect community nodes.

To block a specific feed without changing crawler inputs:

```bash
BLOCKED_FEED_GUIDS=27293ad7-c199-5047-8135-a864fb546492,27293ad7-c199-5047-8135-a864fb546491
BLOCKED_FEED_URLS=https://feeds.podcastindex.org/100retro.xml,https://feeds.podcastindex.org/100retro_test.xml
```

---

## Running a primary node

### Build or install the binary

Choose one:

```bash
# Build from source
cargo build --release
./target/release/stophammer
```

```bash
# Or install the latest published Linux binary
sh install.sh
stophammer
```

```bash
# Or build a container image
docker build -t stophammer .
```

The repo also now ships versioned deployment assets:

- production-oriented compose file: [docker-compose.yml](docker-compose.yml)
- packaging asset index: [packaging/README.md](packaging/README.md)
- release assembly layout: [packaging/releases/README.md](packaging/releases/README.md)
- systemd units: [packaging/systemd](packaging/systemd)
- env examples: [packaging/env](packaging/env)
- service-user/state-dir definitions:
  - [packaging/sysusers.d](packaging/sysusers.d)
  - [packaging/tmpfiles.d](packaging/tmpfiles.d)

`install.sh` still exists for direct binary installs, but it is now the legacy
path. The packaged env/unit assets and container images are the intended
operator-facing direction.

Release tarball assembly is now driven by:

- [scripts/assemble-release.sh](scripts/assemble-release.sh)
- [scripts/publish-release.sh](scripts/publish-release.sh)
- [scripts/verify-release.sh](scripts/verify-release.sh)
- [scripts/build-arch-packages.sh](scripts/build-arch-packages.sh)
- [scripts/verify-arch-packages.sh](scripts/verify-arch-packages.sh)
- [packaging/releases/README.md](packaging/releases/README.md)

Tagged releases also publish multi-arch OCI images to GHCR for the three role
names:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

Tagged releases also build the Arch split packages and attach them to the
GitHub release as `.pkg.tar.zst` assets plus an Arch checksum file.

The compose file uses sample env files under [packaging/env](packaging/env):

- `compose-primary.env`
- `compose-resolverd.env`
- `compose-crawler-gossip.env`

Edit those before using the compose stack.

Container contract:

- `stophammer` image contains the full indexer-role binary set
- `stophammer` defaults to running `stophammer`
- `stophammer-resolverd` is selected by overriding the container command
- `stophammer-crawler` defaults to `stophammer-crawler gossip`
- both images use `/data` as the runtime working directory / volume root
- tagged releases publish:
  - `ghcr.io/<owner>/stophammer-indexer`
  - `ghcr.io/<owner>/stophammer-node`
  - `ghcr.io/<owner>/stophammer-crawler`
- `stophammer-indexer` and `stophammer-node` come from separate targets in the
  same root Dockerfile

### Credentials

The primary generates an ed25519 signing key at `KEY_PATH` on first start.
**Back this file up.** All events in the network are signed with this key.
If you lose it and restart with a new key, community nodes will reject the new
events (signature mismatch against the stored `signed_by` pubkey). The network
does not break immediately — existing events remain valid — but new events will
be unverifiable by nodes that trusted the old key.

To recover credentials across restarts (Docker, redeployment, etc.), mount
`KEY_PATH` from a persistent volume or bind-mount, and `DB_PATH` similarly.

### Minimal setup

```bash
# Generate a signing key and start a primary
DB_PATH=./stophammer.db \
KEY_PATH=./signing.key \
CRAWL_TOKEN=change-me \
BIND=0.0.0.0:8008 \
./target/release/stophammer
```

The primary exposes:

| Endpoint | Description |
|---|---|
| `POST /ingest/feed` | Crawler submission |
| `GET /sync/events` | Paginated event log (requires sync auth) |
| `POST /sync/reconcile` | Set-diff catch-up for rejoining nodes |
| `POST /sync/register` | Community nodes announce their push URL |
| `GET /sync/peers` | Returns known active peers (requires sync auth) |
| `GET /node/info` | Returns this node's pubkey |
| `POST /admin/artists/merge` | Requires `X-Admin-Token` |
| `POST /admin/artists/alias` | Requires `X-Admin-Token` |
| `GET /health` | Liveness probe |

### Get the primary's pubkey

```bash
curl http://your-primary:8008/node/info
# {"node_pubkey":"0805c402..."}
```

Community nodes auto-fetch this on startup. You only need it manually if you
want to pre-configure `PRIMARY_PUBKEY` on community nodes for extra hardening.

---

## Running a community node

Community nodes are read-only replicas. They receive pushed events from the
primary, verify signatures, and serve the same read API.

### Minimal setup

```bash
NODE_MODE=community \
DB_PATH=./stophammer.db \
KEY_PATH=./signing.key \
BIND=0.0.0.0:8008 \
PRIMARY_URL=http://your-primary:8008 \
NODE_ADDRESS=http://this-node-public-url:8008 \
SYNC_TOKEN=change-me \
./target/release/stophammer
```

On startup, the community node:
1. Fetches `GET {PRIMARY_URL}/node/info` to auto-discover the primary's pubkey
   (retries up to 10 times with 2s delay — handles primary still booting)
2. Registers its push URL with the primary: `POST {PRIMARY_URL}/sync/register`
   and signs that registration payload with its node key
3. Does an initial fallback poll to catch up from the current cursor
4. Enters the push-receive + fallback-poll loop

If `PRIMARY_URL` is plain `http://`, auto-discovery is rejected unless you either:

- set `PRIMARY_PUBKEY=...`, or
- set `ALLOW_INSECURE_PUBKEY_DISCOVERY=true` for local development / Docker only

In production, use HTTPS for `PRIMARY_URL` or pin `PRIMARY_PUBKEY` explicitly.

### Versioned service assets

The shipped role units are:

- `stophammer-primary.service`
- `stophammer-community.service`
- `stophammer-resolverd.service`
- `stophammer-gossip.service`

The repository also includes example one-shot units for operator-scheduled work:

- `stophammer-import.service` + `stophammer-import.timer`
- `stophammer-crawl.service` + `stophammer-crawl.timer`

These packaged assets are the base for later distro packaging and should be the
starting point for local systemd installs.

### Credentials

Same rule as primary: mount `KEY_PATH` and `DB_PATH` from persistent storage.
The community node's signing key identifies it in `GET /sync/peers`. If the key
changes, the old peer row in the primary's `peer_nodes` table becomes orphaned
(push still works — the node re-registers with its new pubkey and a new row is
created). The old row stays dormant until it accumulates 5 failures and is evicted.

### Fallback poll

If the primary is down or slow, the community node falls back to polling after
`PUSH_TIMEOUT_SECS` (default 90) of silence. The poll interval is `POLL_INTERVAL_SECS`
(default 300). Worst-case catch-up latency after a primary restart: 300 seconds.

### Optional: pin the primary pubkey

If you want community nodes to reject events signed by any key other than the
known primary (stronger trust model):

```bash
PRIMARY_PUBKEY=<primary-node-pubkey-from-/node/info>
```

Without this, the pubkey is auto-discovered from `/node/info` at startup.

---

## Running local test environments (Docker)

```bash
cd /path/to/stophammer

# Plain-HTTP end-to-end test stack:
# primary + 2 community nodes + mock RSS server
docker compose -f docker-compose.e2e.yml up -d --build --wait

# Run the repo's end-to-end smoke script against that stack
./tests/e2e_docker_compose_tests.sh

# Tear it down
docker compose -f docker-compose.e2e.yml down -v
```

For TLS/ACME testing against Pebble:

```bash
docker compose -f docker-compose.e2e-tls.yml up -d --build --wait
curl -k https://localhost:14000/dir
docker compose -f docker-compose.e2e-tls.yml down -v
```

These compose files are still the dedicated E2E test environments. For a
production-oriented reference stack, use the root
[docker-compose.yml](docker-compose.yml).

### Override the verifier chain (dev/test)

```bash
# Skip medium_music for feeds that don't set podcast:medium yet
CRAWL_TOKEN=secret SYNC_TOKEN=test-sync-token \
  VERIFIER_CHAIN=crawl_token,content_hash,v4v_payment,enclosure_type \
  docker compose -f docker-compose.e2e.yml up -d primary
```

```bash
# Block exact feed GUIDs / URLs before any enrichment work
CRAWL_TOKEN=secret SYNC_TOKEN=test-sync-token \
  BLOCKED_FEED_GUIDS=27293ad7-c199-5047-8135-a864fb546492 \
  BLOCKED_FEED_URLS=https://feeds.podcastindex.org/100retro.xml \
  VERIFIER_CHAIN=crawl_token,content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type \
  docker compose -f docker-compose.e2e.yml up -d primary
```

### Persistent credentials across compose restarts

By default, `docker compose -f docker-compose.e2e.yml up` preserves named volumes
(`primary-e2e`, `community1-e2e`, etc.) across restarts — signing keys and databases
survive `down` and `up` cycles.

To fully reset (wipe all state):
```bash
docker compose -f docker-compose.e2e.yml down -v   # removes volumes too
```

To back up a signing key from a running E2E container:
```bash
docker compose -f docker-compose.e2e.yml cp primary:/data/signing.key ./primary-signing.key.bak
```

---

## Running crawlers

Stophammer does not run or schedule crawlers. Crawlers are separate processes
that authenticate with `CRAWL_TOKEN` and POST to `/ingest/feed`.

Crawler deployments should be sandboxed or network-isolated from the primary as much as
practical. They need access to the public internet and to the primary's ingest endpoint,
but they should not have broad access to internal services, metadata endpoints, or admin
credentials. Plain-HTTP feed fetches also remain weaker against DNS poisoning and
on-path tampering than HTTPS fetches, even when crawler SSRF blast radius is reduced.

### RSS crawler

All crawling modes live in [stophammer-crawler](https://github.com/dardevelin/stophammer-crawler).
From this checkout, run them via the crawler submodule manifest:

```bash
# One-shot: crawl specific feed URLs
CRAWL_TOKEN=secret \
INGEST_URL=http://127.0.0.1:8008/ingest/feed \
cargo run --manifest-path stophammer-crawler/Cargo.toml -- \
  crawl https://feeds.rssblue.com/stereon-music

# From a file
CRAWL_TOKEN=secret \
INGEST_URL=http://127.0.0.1:8008/ingest/feed \
cargo run --manifest-path stophammer-crawler/Cargo.toml -- \
  crawl feeds.txt
```

## Maintenance utilities

This repo also ships local maintenance binaries for rebuilding and reviewing
derived state from an existing database:

```bash
# Drain the durable canonical resolver queue
cargo run --bin stophammer-resolverd

# Inspect or toggle resolver import pause state, plus backfill pause status
cargo run --bin stophammer-resolverctl -- status
cargo run --bin stophammer-resolverctl -- import-active
cargo run --bin stophammer-resolverctl -- import-idle

# Wipe all resolved state and re-queue every feed for re-resolution (destructive)
cargo run --bin stophammer-resolverctl -- re-resolve

# Rebuild canonical releases / recordings and mapping tables
# This automatically coordinates with stophammer-resolverd via resolver_state.backfill_active.
cargo run --bin backfill_canonical -- --db ./stophammer.db

# Re-run artist identity merges from current source evidence
# This automatically coordinates with stophammer-resolverd via resolver_state.backfill_active.
cargo run --bin backfill_artist_identity -- --db ./stophammer.db

# Review unresolved duplicate artist-name groups and their source evidence
cargo run --bin review_artist_identity -- --db ./stophammer.db --limit 20

# Inspect one duplicate-name group in detail
cargo run --bin review_artist_identity -- --db ./stophammer.db --name mooky

# Inspect the targeted artist-identity plan for one feed
cargo run --bin review_artist_identity -- --db ./stophammer.db --feed-guid feed-guid-here

# List feeds whose targeted artist-identity plan still has candidate groups
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-feeds --limit 20

# List stored pending resolver review items
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-reviews --limit 20

# Inspect one stored review item
cargo run --bin review_artist_identity -- --db ./stophammer.db --show-review 17

# Store a durable merge override for one review item
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --merge-review 17 --target-artist artist-123 --note "same artist, operator confirmed"

# Store a durable do-not-merge override for one review item
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --reject-review 17 --note "different projects sharing one name"

# Review pending artist identity items in the TUI
cargo run --bin review_artist_identity_tui -- --db ./stophammer.db --limit 200

# Rebuild wallet endpoints, classifications, and artist links
# This automatically coordinates with stophammer-resolverd via resolver_state.backfill_active.
cargo run --bin backfill_wallets -- --db ./stophammer.db
# Re-derive display names and generate review items (refresh pass)
cargo run --bin backfill_wallets -- --db ./stophammer.db --refresh

# Review pending wallet identity items
cargo run --bin review_wallet_identity -- --db ./stophammer.db
cargo run --bin review_wallet_identity -- --db ./stophammer.db --show-review 42
cargo run --bin review_wallet_identity_tui -- --db ./stophammer.db --limit 200

# Inspect source-claim and resolved-promotion evidence in the TUI
cargo run --bin review_source_claims_tui -- --db ./stophammer.db --limit 200
```

These do not fetch from the network. They operate on an existing local DB file.

`stophammer-resolverd` is the background worker for durable derived-state rebuilds. It
drains `resolver_queue` incrementally and pauses while either a fresh
`resolver_state.import_active` heartbeat or a coordinated
`resolver_state.backfill_active` heartbeat is present. Stale heartbeats are
ignored so a crashed importer or backfill cannot wedge the queue forever.
Source feed/track search, source quality scores, canonical release/recording
state, canonical-first search, promotions, and targeted artist identity now
all converge through `stophammer-resolverd` on the primary.

The artist-identity review tool persists feed-scoped review items and durable
merge or do-not-merge overrides on top of that automatic resolver path.

`stophammer-resolverd` consumes preserved source facts. It does not rewrite feed rows,
track rows, or staged source claims; those remain the authoritative extracted
RSS layer.

Primary-authority resolved replication is now the default `stophammer-resolverd`
behavior on primaries. Unless you explicitly disable it with
`RESOLVER_EMIT_RESOLVED_STATE_EVENTS=false`, the worker emits signed:

```bash
source_feed_read_models_resolved
canonical_feed_state_replaced
canonical_feed_promotions_replaced
artist_identity_feed_resolved
artist_merged
```

Replicas apply those signed resolved events directly. Community nodes no
longer run `stophammer-resolverd`; they follow the primary once its resolver has finished
making changes. This is documented in
[ADR 0029](docs/adr/0029-primary-resolved-replication-authority.md)
and the rollout notes in
[docs/primary-resolved-replication-plan.md](docs/primary-resolved-replication-plan.md).

To see whether canonical views are caught up, query:

```bash
curl http://127.0.0.1:8008/v1/resolver/status
```

That endpoint reports resolver queue counts, import/backfill pause heartbeat
state, and which API surfaces are immediate source-layer reads versus
resolver-backed canonical views.

For a resolver-aware load check, use:

```bash
FEED_GUID=feed-guid-here ./tests/load_test.sh
FEED_GUID=feed-guid-here SEARCH_QUERY=artist-name WAIT_FOR_RESOLVER=1 ./tests/load_test.sh
```

That script treats source feed/track reads and resolver-backed search as
separate layers instead of assuming search freshness is part of the ingest
path.

Use `stophammer-resolverctl import-active` before a large bulk import and
`stophammer-resolverctl import-idle` after it finishes so the queue can drain again. The
backfill binaries coordinate automatically and do not need manual
`stophammer-resolverctl` bracketing. When the crawler importer runs with
`RESOLVER_DB_PATH=/path/to/stophammer.db`, it does the import bracketing
automatically and refreshes the import heartbeat while the import is still
running.

Schedule with cron:
```
0 */6 * * *  CRAWL_TOKEN=secret INGEST_URL=http://primary:8008/ingest/feed \
             stophammer-crawler crawl /etc/stophammer/feeds.txt
```

### Gossip listener

Consumes a local `gossip-listener` SSE stream and optionally replays its
archive database for durable catch-up:

```bash
CRAWL_TOKEN=secret \
INGEST_URL=http://127.0.0.1:8008/ingest/feed \
cargo run --manifest-path stophammer-crawler/Cargo.toml -- \
  gossip --archive-db /var/lib/podping-alpha-gossip-listener/archive.db --concurrency 3
```

### PodcastIndex snapshot importer

Bulk-imports from a PodcastIndex database snapshot:

```bash
CRAWL_TOKEN=secret \
INGEST_URL=http://127.0.0.1:8008/ingest/feed \
cargo run --manifest-path stophammer-crawler/Cargo.toml -- \
  import --db /path/to/podcastindex_feeds.db
```

### Cached corpus replay and analysis

The crawler repo also ships local analysis binaries for replaying cached RSS data
without refetching the internet:

```bash
# Audit feeds into NDJSON
cargo run --manifest-path stophammer-crawler/Cargo.toml --bin feed_audit -- \
  --db ./stophammer-crawler/analysis/data/stophammer-feeds.db \
  --output ./stophammer-crawler/analysis/data/feed_audit.ndjson

# Re-analyze the NDJSON corpus
cargo run --manifest-path stophammer-crawler/Cargo.toml --bin audit_analyzer -- \
  --input ./stophammer-crawler/analysis/data/feed_audit.ndjson

# Replay cached feeds into the primary without refetching them
CRAWL_TOKEN=secret \
INGEST_URL=http://127.0.0.1:8008/ingest/feed \
cargo run --manifest-path stophammer-crawler/Cargo.toml --bin audit_import -- \
  --input ./stophammer-crawler/analysis/data/feed_audit.ndjson \
  --reset
```

---

## What community nodes do NOT do

- **Do not re-run verifiers.** The verifier chain runs on the primary only.
  Community nodes verify the ed25519 signature — if it's valid and signed by
  the known primary key, the event is accepted. The `warnings` field in each
  event carries the primary's verifier output as an audit trail.
- **Do not ingest feeds.** The `POST /ingest/feed` endpoint is primary-only.
- **Do not sign events.** Community nodes have a signing key for identity
  (peer registration) but never sign events. All events in the log are signed
  by the primary.
- **Do not run `stophammer-resolverd`.** Community nodes now wait for the primary to emit
  signed source-read-model, canonical-state, promotion, and artist-identity
  resolver events.
