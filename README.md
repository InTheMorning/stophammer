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

## Ecosystem

| Repository | Description |
|---|---|
| **[stophammer](https://github.com/inthemorning/stophammer)** | Primary / community node (this repo) |
| [stophammer-crawler](https://github.com/inthemorning/stophammer-crawler) | Unified feed crawler — one-shot crawl, PodcastIndex import, gossip-listener SSE/archive ingestion, and crawler-side analysis tools |
| [stophammer-parser](https://github.com/inthemorning/stophammer-parser) | Declarative RSS/Podcast XML extraction engine (Rust library) |
| [stophammer-tracker](https://github.com/inthemorning/stophammer-tracker) | Cloudflare Workers peer tracker (optional bootstrap) |

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
- **Contributors** running the external crawler stack to grow the index

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
  responsibility (cron, systemd timer, external service). The crawler runtime,
  modes, and analysis tools live in the separate
  [stophammer-crawler](https://github.com/inthemorning/stophammer-crawler) repo.
  Treat crawlers as the SSRF-exposed fetch tier: they should be low-privilege,
  network-restricted processes that can reach public feed hosts and the primary's ingest
  endpoint, but not arbitrary internal services or primary secrets.

Verifier behavior now lives in the dedicated
[Verifier Guide](docs/verifier-guide.md), including the default chain,
`VERIFIER_CHAIN` examples, exact-feed blocklisting via `feed_blocklist`, and
the accepted medium rules for `music`, `publisher`, and `musicL`.

## Running a primary node

### Build from source

Build the main node/runtime binaries from the repo root:

```bash
# Main node plus resolver and maintenance binaries
cargo build --release --bins
```

That produces:

- `target/release/stophammer`
- `target/release/stophammer-resolverd`
- `target/release/stophammer-resolverctl`
- the backfill/review tools

Typical local runs after building:

```bash
./target/release/stophammer
NODE_MODE=community ./target/release/stophammer
./target/release/stophammer-resolverd
```

Crawler build and runtime instructions live in the separate
[stophammer-crawler README](https://github.com/inthemorning/stophammer-crawler).

### Install published release artifacts

Tagged releases publish three role tarballs:

- `stophammer-indexer-<version>.tar.gz`
- `stophammer-node-<version>.tar.gz`
- `stophammer-crawler-<version>.tar.gz`

Install shape:

- copy `bin/*` into your binary path
- copy `systemd/*` into `/usr/lib/systemd/system/`
- copy `env/*.example` to `/etc/stophammer/` and remove the `.example` suffix
- copy `sysusers.d/*` and `tmpfiles.d/*` into the matching system locations

The release-bundle contents for each role are documented in:

- [packaging/releases/stophammer-indexer.README.md](packaging/releases/stophammer-indexer.README.md)
- [packaging/releases/stophammer-node.README.md](packaging/releases/stophammer-node.README.md)
- [packaging/releases/stophammer-crawler.README.md](packaging/releases/stophammer-crawler.README.md)

`install.sh` still exists for legacy single-binary installs, but it is no
longer the primary packaging path.

### Build or pull container images

Build the role images explicitly:

```bash
# Preferred when the Docker buildx plugin is installed
docker buildx build --load --target stophammer-indexer -t stophammer-indexer .
docker buildx build --load --target stophammer-node -t stophammer-node .
```

`--load` imports the built image into your local Docker image store. Omit it if
you are only producing remote/pushed artifacts in CI.

If your Docker CLI does not have the `buildx` plugin yet, the plain legacy
builder still works for local builds:

```bash
docker build --target stophammer-indexer -t stophammer-indexer .
docker build --target stophammer-node -t stophammer-node .
```

The crawler image is built and released from the separate
[stophammer-crawler README](https://github.com/inthemorning/stophammer-crawler).

Tagged releases also publish OCI images to GHCR:

- `ghcr.io/<owner>/stophammer-indexer`
- `ghcr.io/<owner>/stophammer-node`
- `ghcr.io/<owner>/stophammer-crawler`

The repo also now ships versioned deployment assets:

- production-oriented compose file: [docker-compose.yml](docker-compose.yml)
- packaging asset index: [packaging/README.md](packaging/README.md)
- release assembly layout: [packaging/releases/README.md](packaging/releases/README.md)
- systemd units: [packaging/systemd](packaging/systemd)
- env examples: [packaging/env](packaging/env)
- service-user/state-dir definitions:
  - [packaging/sysusers.d](packaging/sysusers.d)
  - [packaging/tmpfiles.d](packaging/tmpfiles.d)

The packaged env/unit assets, role tarballs, Arch packages, and container
images are the intended operator-facing distribution paths.

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

- `primary.compose.env.example`
- `resolverd.compose.env.example`
- `podping.compose.env.example`
- `crawler-gossip.compose.env.example`
- `crawler-import.compose.env.example`

Additional Docker-specific templates are also shipped for custom compose
services that are not part of the default root stack:

- `community.compose.env.example`
- `crawler-crawl.compose.env.example`

Copy them once into local ignored `*.compose.env` files, then edit those:

```bash
cp packaging/env/primary.compose.env.example packaging/env/primary.compose.env
cp packaging/env/resolverd.compose.env.example packaging/env/resolverd.compose.env
cp packaging/env/podping.compose.env.example packaging/env/podping.compose.env
cp packaging/env/crawler-gossip.compose.env.example packaging/env/crawler-gossip.compose.env
cp packaging/env/crawler-import.compose.env.example packaging/env/crawler-import.compose.env
```

Container contract:

- `stophammer` image contains the full indexer-role binary set
- `stophammer` defaults to running `stophammer`
- `stophammer-resolverd` is selected by overriding the container command
- `stophammer-crawler` defaults to `stophammer-crawler gossip`
- both images use `/data` as the runtime working directory / volume root
- `stophammer-indexer` and `stophammer-node` come from separate targets in the
  same root Dockerfile

### Configure a containerized primary

For a primary/indexer container you normally need:

- a persistent `/data` volume for `stophammer.db` and `signing.key`
- `CRAWL_TOKEN` for crawler submissions
- `SYNC_TOKEN` for node-to-node sync endpoints
- optionally `ADMIN_TOKEN` for admin routes
- optionally `TRUST_PROXY=true` if TLS is terminated by nginx/Caddy in front of
  the container

Create the persistent volume once:

```bash
docker volume create stophammer-indexer-data
```

The preferred container workflow is to edit the shipped env files and use the
root compose stack, rather than typing long `docker run` commands repeatedly.

Edit:

- `packaging/env/primary.compose.env`
- `packaging/env/resolverd.compose.env`

Basic settings for the primary are:

```bash
CRAWL_TOKEN=change-me
SYNC_TOKEN=change-me-too
ADMIN_TOKEN=optional-admin-token
```
Generating a long random hex string is ideal for these.
```
openssl rand -hex 32
```

Then start the reference stack:

```bash
docker compose up -d --build primary resolverd
```

`primary` builds the shared `stophammer-indexer` image. `resolverd` now reuses
that same image with a different command, so Compose no longer performs a
second root image build just for the resolver worker.

If you also want the bundled podping listener plus gossip crawler, edit:

- `packaging/env/podping.compose.env`
- `packaging/env/crawler-gossip.compose.env`

then run:

```bash
docker compose up -d podping gossip
```

The Compose stack now initializes the `podping-data` and `crawler-data` named
volumes automatically on first boot so the non-root containers can write their
state files without a manual `chown`.

To run the one-shot PodcastIndex importer, edit
`packaging/env/crawler-import.compose.env`, then run:

```bash
docker compose run --rm import
```

### Change configuration later

Edit the relevant env file, then recreate the affected service:

```bash
# after editing packaging/env/primary.compose.env
docker compose up -d --build primary

# after editing packaging/env/resolverd.compose.env
docker compose up -d resolverd

# after editing packaging/env/podping.compose.env
docker compose up -d podping gossip

# after editing packaging/env/crawler-gossip.compose.env
docker compose up -d gossip

# after editing packaging/env/crawler-import.compose.env
docker compose run --rm import
```

If you changed Rust code or the root Dockerfile and need both primary-side
containers refreshed, rebuild `primary`; `resolverd` will pick up the same image:

```bash
docker compose up -d --build primary resolverd
```

The persistent `/data` volume keeps `stophammer.db` and `signing.key` across
these recreations.

### Manual docker run alternative

Start the main node:

```bash
docker run -d \
  --name stophammer-primary \
  -p 8008:8008 \
  -v stophammer-data:/data \
  -e CRAWL_TOKEN=change-me \
  -e SYNC_TOKEN=change-me-too \
  stophammer-indexer
```

Start the resolver worker as a second container against the same `/data`
volume:

```bash
docker run -d \
  --name stophammer-resolverd \
  --depends-on stophammer-primary \
  -v stophammer-data:/data \
  -e RESOLVER_INTERVAL_SECS=30 \
  -e RESOLVER_BATCH_SIZE=25 \
  --entrypoint stophammer-resolverd \
  stophammer-indexer
```

Useful checks:

```bash
docker logs -f stophammer-primary
docker logs -f stophammer-resolverd
curl http://127.0.0.1:8008/health
curl http://127.0.0.1:8008/node/info
```

### Run the reference compose stack

The root [docker-compose.yml](docker-compose.yml) is the reference packaged
stack for:

- `primary`
- `resolverd`
- `podping`
- `gossip`

Edit the sample env files first:

- `packaging/env/primary.compose.env`
- `packaging/env/resolverd.compose.env`
- `packaging/env/podping.compose.env`
- `packaging/env/crawler-gossip.compose.env`
- `packaging/env/crawler-import.compose.env`

Then start the stack:

```bash
docker compose up -d --build
```

On first boot, the one-shot `podping-init` and `crawler-init` services fix
volume ownership for the long-running non-root containers.

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

## Maintenance and crawlers

Maintenance/review workflows live in the wiki and operator docs:

- [docs/wiki/Maintenance-and-Review.md](docs/wiki/Maintenance-and-Review.md)
- [docs/operations.md](docs/operations.md)

Crawler runtime, import/gossip modes, and crawler-side analysis tools live in
the separate crawler docs:

- [stophammer-crawler README](https://github.com/inthemorning/stophammer-crawler)

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
