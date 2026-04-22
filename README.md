# Stophammer

A quality-gated V4V music index with a source-first v1 data model.

## Documentation

- quick start and architecture: this README
- narrative introduction: [docs/user-guide.md](docs/user-guide.md)
- operator deployment and maintenance: [docs/operations.md](docs/operations.md)
- HTTP API reference: [docs/API.md](docs/API.md)
- runtime API explorer: `GET /api` (backed by `GET /openapi.json`)
- schema reference: [docs/schema-reference.md](docs/schema-reference.md)
- verifier behavior: [docs/verifier-guide.md](docs/verifier-guide.md)
- source-first music schema ADR: [docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md](docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)

## What Stophammer Is

Stophammer accepts and indexes RSS feeds that:

- declare `podcast:medium=music`
- carry at least one structurally valid `podcast:value` route

It also preserves two container/source-layer feed kinds:

- `publisher` feeds
- `musicL` feeds

The public v1 model is source-first:

- `feeds` are the release-shaped rows
- `tracks` are the track-shaped rows
- source claims, links, IDs, remote items, enclosure variants, and transcripts are preserved
- the old canonical release/recording public API is retired

Publisher handling is intentionally strict:

- `publisher` means publisher by default
- non-Wavlake publisher text is only promoted from `podcast:remoteItem` links when the publisher/music relation is reciprocal
- Wavlake is a narrow compatibility exception where the linked publisher feed may supply artist text for the music feed while the stored publisher remains `"Wavlake"`
- track rows inherit that same publisher truth in `tracks.publisher`

## Architecture

```text
[crawler] -> POST /ingest/feed -> [primary] -> POST /sync/push -> [community nodes]
                                   ^
                              verifier chain
                              signs events
```

- Primary nodes ingest feeds, run verifiers, write SQLite, and sign events.
- Community nodes replicate the signed event log and serve read APIs.
- Crawlers are external untrusted processes. This repo does not schedule them.

The crawler runtime and importer live in the separate
[stophammer-crawler](stophammer-crawler/README.md) package directory, which is
also published as its own release artifact.

## Build From Source

```bash
cargo build --release
```

Common local runs:

```bash
./target/release/stophammer
NODE_MODE=community ./target/release/stophammer
```

Useful checks:

```bash
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Container Images

Build the main images:

```bash
docker buildx build --load --target stophammer-indexer -t stophammer-indexer .
docker buildx build --load --target stophammer-node -t stophammer-node .
```

If `buildx` is unavailable:

```bash
docker build --target stophammer-indexer -t stophammer-indexer .
docker build --target stophammer-node -t stophammer-node .
```

## Reference Compose Stack

The root [docker-compose.yml](docker-compose.yml) defines the current packaged stack:

- `primary`
- `podping-listener`
- `gossip`
- `import`
- `import-wavlake`
- `stophammer-crawler` (tools profile, one-shot feed crawl)

Copy the sample env files you actually use:

```bash
cp packaging/env/primary.compose.env.example packaging/env/primary.compose.env
cp packaging/env/podping-listener.compose.env.example packaging/env/podping-listener.compose.env
cp packaging/env/crawler-feed.compose.env.example packaging/env/crawler-feed.compose.env
cp packaging/env/crawler-gossip.compose.env.example packaging/env/crawler-gossip.compose.env
cp packaging/env/crawler-import.compose.env.example packaging/env/crawler-import.compose.env
cp packaging/env/crawler-import-wavlake.compose.env.example packaging/env/crawler-import-wavlake.compose.env
```

Primary configuration usually needs:

```bash
CRAWL_TOKEN=change-me
SYNC_TOKEN=change-me-too
ADMIN_TOKEN=optional-admin-token
```

Start the primary:

```bash
docker compose up -d --build primary
```

Optional bundled crawler services:

```bash
docker compose up -d podping-listener gossip
docker compose run --rm import
docker compose run --rm import-wavlake
docker compose --profile tools run --rm stophammer-crawler feed https://example.com/feed.xml
```

If you are updating an older resolver-era deployment, use:

```bash
docker compose up -d --build --remove-orphans
```

That removes the retired `resolverd` container after the VPS has the updated repo state.

## Persistent Data

Keep these paths on persistent storage:

- `DB_PATH` for `stophammer.db`
- `KEY_PATH` for the node signing key

Do not discard the signing key unless you intend to rotate node identity. Community nodes verify pushed events against that key.

## Community Nodes

Community nodes:

- do not ingest feeds
- do not run verifiers
- do not sign feed events
- do not run a resolver worker

They replicate signed events from the primary and serve the read API from local state.

## Current Public API Shape

Public source-first reads:

- `GET /v1/feeds/{guid}`
- `GET /v1/tracks/{guid}`
- `GET /v1/feeds/recent`
- `GET /v1/search`
- `GET /v1/publishers`
- `GET /v1/publishers/{publisher}`
- `GET /v1/node/capabilities`
- `GET /v1/peers`

Useful provenance/debug includes on feed reads:

- `remote_items`
- `publisher`
- `source_links`
- `source_ids`
- `source_contributors`
- `source_platforms`
- `source_release_claims`

See [docs/API.md](docs/API.md) for exact payloads.
