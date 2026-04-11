# Operations Manual

This guide covers deploying, configuring, monitoring, and maintaining Stophammer nodes in production.

---

## Environment Variables Reference

### Core

| Variable | Default | Required | Description |
|----------|---------|----------|-------------|
| `NODE_MODE` | `primary` | No | `primary` or `community`. Determines which router and sync behavior the node uses. |
| `DB_PATH` | `stophammer.db` | No | Path to the SQLite database file. Use a persistent volume in Docker. |
| `KEY_PATH` | `signing.key` | No | Path to the ed25519 signing key. Generated on first start if absent. **Back this up.** |
| `BIND` | `0.0.0.0:8008` | No | Socket address to bind. Format: `ip:port`. |
| `CRAWL_TOKEN` | -- | **Yes** (primary) | Shared secret for crawler authentication. Compared in constant time (SHA-256). |
| `ADMIN_TOKEN` | `""` (empty) | No | Token for write-side admin endpoints (`X-Admin-Token` header). It is not accepted on sync endpoints. |
| `SYNC_TOKEN` | unset | No | Dedicated token for sync endpoints (`GET /sync/events`, `GET /sync/peers`, `POST /sync/register`, `POST /sync/reconcile`) via `X-Sync-Token`. If unset, those sync endpoints return 403. |
| `RUST_LOG` | `stophammer=info` | No | Tracing filter directive. Examples: `stophammer=debug`, `stophammer=trace`, `stophammer::api=debug,stophammer=info`. |

### TLS (ACME / Let's Encrypt)

See [ADR-0019](adr/0019-tls-acme-let-s-encrypt.md) for the full design.

| Variable | Default | Required | Description |
|----------|---------|----------|-------------|
| `TLS_DOMAIN` | unset | No | Domain (or IP) to provision a certificate for. If unset, the node starts in plain HTTP with a log warning. |
| `TLS_ACME_EMAIL` | -- | **Yes** when `TLS_DOMAIN` is set | Contact email registered with Let's Encrypt. |
| `TLS_CERT_PATH` | `./tls/cert.pem` | No | Path to store the provisioned certificate chain. |
| `TLS_KEY_PATH` | `./tls/key.pem` | No | Path to store the certificate private key (written with 0o600 permissions). |
| `TLS_ACME_STAGING` | `false` | No | Set to `true` to use the Let's Encrypt staging environment (for testing). |
| `TLS_ACME_ACCOUNT_PATH` | `./tls/acme-account.json` | No | Path to persist the ACME account credentials across restarts. |
| `TLS_ACME_DIRECTORY_URL` | unset | No | Custom ACME directory URL. Overrides `TLS_ACME_STAGING` when set. Useful for Pebble or other non-Let's-Encrypt test endpoints. |

### Community Mode

| Variable | Default | Required | Description |
|----------|---------|----------|-------------|
| `PRIMARY_URL` | -- | **Yes** (community) | Base URL of the primary node (e.g. `https://primary.example.com:8008`). |
| `PRIMARY_PUBKEY` | auto-discovered | No | Hex-encoded ed25519 pubkey of the primary. If omitted, fetched from `GET {PRIMARY_URL}/node/info` at startup with up to 10 retries. |
| `TRACKER_URL` | `https://stophammer-tracker.workers.dev` | No | Cloudflare tracker URL for initial bootstrap registration. |
| `NODE_ADDRESS` | -- | **Yes** (community) | Publicly reachable URL of this node (e.g. `https://my-node.example.com:8008`). The primary pushes events to `{NODE_ADDRESS}/sync/push`. |
| `POLL_INTERVAL_SECS` | `300` | No | Seconds between fallback poll-loop iterations. |
| `PUSH_TIMEOUT_SECS` | `90` | No | Seconds of push silence before the fallback poll fires. |
| `ALLOW_INSECURE_PUBKEY_DISCOVERY` | `false` | No | Set to `true` to allow pubkey auto-discovery over plain HTTP. **Only for local development/Docker.** Production nodes must use HTTPS or set `PRIMARY_PUBKEY` explicitly. |

### Rate Limiting

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT_RPS` | `50` | Requests per second per IP (token bucket refill rate). |
| `RATE_LIMIT_BURST` | `100` | Maximum burst size per IP. |
| `TRUST_PROXY` | `false` | Set to `true` when behind a reverse proxy. Uses `X-Forwarded-For` for client IP extraction. When `false` (default), `X-Forwarded-For` is ignored to prevent IP spoofing. |

### Security

| Variable | Default | Description |
|----------|---------|-------------|
| `CORS_ALLOW_ORIGIN` | `*` (any) | Restrict CORS `Access-Control-Allow-Origin`. Set to a specific origin (e.g. `https://app.example.com`) in production. |

### Tuning

| Variable | Default | Description |
|----------|---------|-------------|
| `PROOF_PRUNE_INTERVAL_SECS` | `300` | How often the background pruner deletes expired proof challenges and tokens (seconds). |
| `VERIFIER_CHAIN` | `crawl_token,content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type` | Comma-separated ordered list of verifiers to run on ingest. Primary only. See the [Verifier Guide](verifier-guide.md). |
| `BLOCKED_FEED_GUIDS` | empty | Optional comma-separated exact GUID blocklist used by the `feed_blocklist` verifier. |
| `BLOCKED_FEED_URLS` | empty | Optional comma-separated exact URL blocklist used by the `feed_blocklist` verifier. |

---

## Startup Modes

## Build and Install

### Build from source

```bash
cargo build --release
./target/release/stophammer
```

### Install the published Linux binary

```bash
sh install.sh
stophammer
```

`install.sh` is now the legacy direct-binary path. The preferred deployment
assets live in:

- [docker-compose.yml](../docker-compose.yml)
- [packaging/README.md](../packaging/README.md)
- [packaging/releases/README.md](../packaging/releases/README.md)
- [packaging/systemd](../packaging/systemd)
- [packaging/env](../packaging/env)

### Container image

```bash
docker build -t stophammer .
```

If the Docker `buildx` plugin is installed, you can also use:

```bash
docker buildx build --load -t stophammer .
```

### Versioned deployment assets

The repository now ships versioned assets for the three operator roles:

- indexer / primary
- community node
- crawler

Asset roots:

- packaging asset index: [packaging/README.md](../packaging/README.md)
- release assembly layout: [packaging/releases/README.md](../packaging/releases/README.md)
- systemd units: [packaging/systemd](../packaging/systemd)
- env examples: [packaging/env](../packaging/env)
- service users: [packaging/sysusers.d](../packaging/sysusers.d)
- state dirs: [packaging/tmpfiles.d](../packaging/tmpfiles.d)
- production compose skeleton: [docker-compose.yml](../docker-compose.yml)

Release tarballs can be assembled with:

```bash
./scripts/assemble-release.sh
```

To produce the tarballs plus a checksum file suitable for a tagged release:

```bash
./scripts/publish-release.sh
```

To unpack and smoke-check the produced bundles before publishing:

```bash
./scripts/verify-release.sh
```

To build the Arch split packages into `dist/arch/`:

```bash
./scripts/build-arch-packages.sh
```

To verify the built Arch packages before publishing:

```bash
./scripts/verify-arch-packages.sh
```

Tagged releases also publish multi-arch OCI images to GHCR for:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

Tagged releases also attach the Arch split packages and an
`SHA256SUMS-arch-<version>.txt` file to the GitHub release.

The compose file intentionally uses runnable sample env files:

- [primary.compose.env.example](../packaging/env/primary.compose.env.example)
- [podping-listener.compose.env.example](../packaging/env/podping-listener.compose.env.example)
- [crawler-gossip.compose.env.example](../packaging/env/crawler-gossip.compose.env.example)
- [crawler-import.compose.env.example](../packaging/env/crawler-import.compose.env.example)

Additional Docker-specific templates are also shipped for custom compose
services that are not part of the default root stack:

- [community.compose.env.example](../packaging/env/community.compose.env.example)
- [crawler-crawl.compose.env.example](../packaging/env/crawler-crawl.compose.env.example)

Copy them into local ignored `*.compose.env` files before using the compose stack.
The default stack now includes an in-network `podping-listener` service based on
`podping.alpha`'s `gossip-listener`, with a shared `podping-listener-data` volume for
the listener's node key, peer cache, `archive.db`, and the SSE stream that
`gossip` consumes at `http://podping-listener:8089/events`.
One-shot `podping-listener-init` and `crawler-init` services fix named-volume ownership
before the long-running non-root containers start.

**One-shot stophammer-crawler service (ad-hoc feed crawl):**

Run a single feed or set of feeds without defining a separate service:

```bash
docker compose run --rm stophammer-crawler feed https://example.com/feed.xml
docker compose run --rm stophammer-crawler feed --force https://example.com/feed.xml

# Replay NDJSON corpus without re-fetching
docker compose run --rm stophammer-crawler ndjson --input /data/stored-feeds.ndjson
```

This uses the tools profile and `stophammer-crawler` service with `entrypoint: ["stophammer-crawler"]`.
Both `CRAWL_TOKEN` and `INGEST_URL` must be set in `crawler-feed.compose.env`.

Container runtime contract:

- `stophammer` image:
  - binaries installed in `/usr/local/bin`
  - working directory `/data`
  - default command `stophammer`
- `stophammer-crawler` image:
  - binary installed in `/usr/local/bin`
  - working directory `/data`
  - default command `stophammer-crawler gossip`
- release automation publishes:
  - `ghcr.io/<owner>/stophammer-indexer`
  - `ghcr.io/<owner>/stophammer-node`
  - `ghcr.io/<owner>/stophammer-crawler`
- the indexer and node images share the same root Dockerfile and differ by
  release target/default role

Both images include `ca-certificates` so HTTPS sync/fetch behavior works without
extra image customization.

Shipped long-running units:

- `stophammer-primary.service`
- `stophammer-community.service`
- `stophammer-gossip.service`

Shipped example one-shot units:

- `stophammer-import.service` + `stophammer-import.timer`
- `stophammer-crawl.service` + `stophammer-crawl.timer`

The Docker Compose stack also includes a one-shot `import` service. Edit
`crawler-import.compose.env`, then run:

```bash
docker compose run --rm import
```

The oneshot import/crawl units are examples only. They are not intended to be
enabled by default in the first packaging milestone.

### Primary Mode (default)

```
NODE_MODE=primary  (or omit — primary is the default)
```

The primary node:
- Accepts `POST /ingest/feed` from crawlers
- Runs the verifier chain on each submission
- Signs accepted events with its ed25519 key
- Fans out new events to all registered community nodes via `POST /sync/push`
- Serves the full API (read, write, admin, sync)
- Spawns a background proof pruner
- Can reject exact blocked feeds early via `feed_blocklist`

**Required env vars:** `CRAWL_TOKEN`

**Crawler deployment boundary:** Crawlers fetch untrusted URLs and should be treated as
an SSRF-exposed tier. Give them outbound access to public feed hosts and
`POST /ingest/feed` on the primary, but avoid broad access to internal services, cloud
metadata endpoints, admin paths, or the primary's signing/admin credentials. The
`CRAWL_TOKEN` only authenticates crawler submissions; it does not harden crawler-side
fetch behavior.

Example blocklist configuration:

```bash
BLOCKED_FEED_GUIDS=27293ad7-c199-5047-8135-a864fb546492,27293ad7-c199-5047-8135-a864fb546491
BLOCKED_FEED_URLS=https://feeds.podcastindex.org/100retro.xml,https://feeds.podcastindex.org/100retro_test.xml
VERIFIER_CHAIN=crawl_token,content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type
```

### Community Mode

```
NODE_MODE=community
```

The community node:
- Does NOT accept ingest requests
- Does NOT run verifiers
- Registers its push URL with the primary on startup
- Receives pushed events from the primary and verifies ed25519 signatures
- Falls back to polling if no push arrives within `PUSH_TIMEOUT_SECS`
- Serves a read-only API (queries, search, sync/events, sync/peers, node/info, SSE events)
- Exposes `POST /sync/push` for the primary to deliver events

**Required env vars:** `PRIMARY_URL`, `NODE_ADDRESS`

### Startup Sequence (Community)

1. Fetch primary's pubkey from `GET {PRIMARY_URL}/node/info` (retries 10x with 2s delay)
2. Register with the Cloudflare tracker (fire-and-forget)
3. Register push URL with the primary via `POST {PRIMARY_URL}/sync/register`
   and sign the registration payload with the community node key. The primary
   requires the submitted URL to end with `/sync/push`, checks that `signed_at`
   is fresh, and verifies same-origin `GET {NODE_ADDRESS}/node/info` returns
   the same `node_pubkey`.
4. Load persisted sync cursor from local DB
5. Enter the push-receive + fallback-poll loop

If `PRIMARY_URL` uses plain HTTP, auto-discovery is rejected unless you either
set `PRIMARY_PUBKEY` explicitly or set `ALLOW_INSECURE_PUBKEY_DISCOVERY=true`
for local development.

---

## TLS Setup

### Tier 1: Domain-based TLS (recommended)

For nodes with a public domain name:

```bash
TLS_DOMAIN=node.example.com \
TLS_ACME_EMAIL=admin@example.com \
CRAWL_TOKEN=secret \
BIND=0.0.0.0:8008 \
./stophammer
```

Requirements:
- Port 80 must be reachable from the public internet (for ACME http-01 challenge)
- DNS A/AAAA record pointing to the server's IP
- The ACME challenge server binds port 80 temporarily during provisioning

On startup, the node:
1. Checks if a valid certificate exists with >30 days remaining
2. If not, provisions via ACME http-01 (temporary port 80 listener)
3. Starts HTTPS on the `BIND` port
4. Spawns a renewal task that checks every 12 hours

### Tier 2: IP-based TLS

For nodes with a public IP but no domain. Let's Encrypt supports IP address certificates (6-day validity). Set `TLS_DOMAIN` to the public IP address:

```bash
TLS_DOMAIN=203.0.113.42 \
TLS_ACME_EMAIL=admin@example.com \
./stophammer
```

Certificates renew every ~5 days instead of every 60.

### Plain HTTP Fallback

When `TLS_DOMAIN` is not set, the node starts in plain HTTP with a warning:

```
WARN: TLS_DOMAIN not set -- node is serving plain HTTP.
      Bearer tokens and crawl tokens are transmitted unencrypted.
      Set TLS_DOMAIN and TLS_ACME_EMAIL for production use.
```

Plain HTTP is acceptable only for:
- Local development (`BIND=127.0.0.1:8008`)
- Docker-internal traffic (crawler -> primary on the same compose network)

### Reverse Proxy Alternative

If you prefer nginx or Caddy in front of the node, leave `TLS_DOMAIN` unset and set `TRUST_PROXY=true` so the node reads `X-Forwarded-For` correctly.

Example primary setup behind nginx:

```bash
TRUST_PROXY=true \
CRAWL_TOKEN=secret \
SYNC_TOKEN=change-me \
BIND=127.0.0.1:8008 \
./stophammer
```

Example nginx TLS termination:

```nginx
server {
    listen 80;
    server_name node.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name node.example.com;

    ssl_certificate /etc/letsencrypt/live/node.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/node.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8008;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Port 443;

        # Useful for server-sent events
        proxy_buffering off;
    }
}
```

Notes:
- Keep `TLS_DOMAIN` unset when nginx terminates TLS.
- Set `TRUST_PROXY=true` so Stophammer uses `X-Forwarded-For` instead of the
  nginx loopback address for rate limiting and request logging.
- Binding Stophammer to `127.0.0.1:8008` keeps the plain-HTTP upstream private
  to the same host.
- The same pattern works for community nodes; proxy to the community node's
  local `BIND` address instead of the primary.

---

## Backup and Restore

### What to Back Up

| File | Purpose | Recovery Impact |
|------|---------|-----------------|
| `DB_PATH` (SQLite .db file) | All feeds, tracks, events, peer state | Lose everything; must resync from primary |
| `KEY_PATH` (signing.key) | ed25519 identity | Lose identity; community nodes reject new events |
| `TLS_CERT_PATH`, `TLS_KEY_PATH` | TLS certificate and key | Re-provisioned automatically on next start |
| `TLS_ACME_ACCOUNT_PATH` | ACME account credentials | New account created on next provisioning |

### SQLite Backup Procedure

Stophammer uses SQLite in WAL (Write-Ahead Logging) mode. Safe backup options:

**Option 1: Copy while paused (simplest)**

```bash
# Stop the node
systemctl stop stophammer

# Copy the database files
cp stophammer.db stophammer.db.bak
cp stophammer.db-wal stophammer.db.bak-wal 2>/dev/null
cp stophammer.db-shm stophammer.db.bak-shm 2>/dev/null

# Restart
systemctl start stophammer
```

**Option 2: SQLite `.backup` command (online)**

```bash
sqlite3 stophammer.db ".backup /backups/stophammer-$(date +%Y%m%d).db"
```

This uses SQLite's built-in online backup API and is safe to run while the node is serving traffic.

**Option 3: File-system snapshot**

If running on ZFS, Btrfs, or LVM, take a filesystem-level snapshot. This is atomic and safe for WAL-mode SQLite.

### Signing Key Backup

The signing key is critical. If lost, a new key is generated on next start and all existing community nodes will reject events signed by the new key.

```bash
cp signing.key signing.key.bak
# Store off-server (encrypted backup, vault, etc.)
```

### Restore

1. Stop the node
2. Replace `DB_PATH` with the backup
3. Replace `KEY_PATH` with the backed-up signing key
4. Start the node
5. Community nodes will catch up automatically via the fallback poll

---

## Maintenance Utilities

Resolver, backfill, and review binaries were retired in Phase 1 of the v4v
music metadata refactor.

```bash
FEED_GUID=feed-guid-here ./tests/load_test.sh
FEED_GUID=feed-guid-here SEARCH_QUERY=artist-name ./tests/load_test.sh
```

Use the current source-first API and the vision/ADR documents instead of the
retired resolver review tooling. The staged plan for later phases lives in:

- [v4v-music-metadata-vision.md](vision/v4v-music-metadata-vision.md)
- [0032-retire-resolver-and-review-runtime.md](adr/0032-retire-resolver-and-review-runtime.md)

---

## Monitoring

### Key Metrics to Watch

**Disk space (events table)**

The `events` table grows monotonically (append-only log). Each event is ~1-5 KB of JSON. At 10,000 events/day, expect ~10-50 MB/day of growth. Monitor the database file size:

```bash
ls -lh stophammer.db
```

**Sync lag (community nodes)**

Check `GET /sync/events?after_seq=0&limit=1` on both the primary and community nodes. Compare `next_seq` values. A large gap indicates the community node is behind.

```bash
# Primary
curl -s -H "X-Sync-Token: $SYNC_TOKEN" \
  http://primary:8008/sync/events?after_seq=999999&limit=1 | jq .next_seq

# Community
curl -s -H "X-Sync-Token: $SYNC_TOKEN" \
  http://community:8009/sync/events?after_seq=999999&limit=1 | jq .next_seq
```

**Push failures**

Monitor logs for `fanout: push returned non-success` and `fanout: evicted peer from push cache`. Peers are evicted after 10 consecutive push failures.

**Health check**

```bash
curl -f http://localhost:8008/health
```

Returns `ok` with status 200. Use as a liveness probe in Docker, Kubernetes, or systemd.

**Peer status**

```bash
curl -s -H "X-Sync-Token: $SYNC_TOKEN" http://primary:8008/sync/peers | jq
```

Check `last_push_at` timestamps. A peer with a stale `last_push_at` is either down or unreachable.

**Proof pruner**

The background pruner logs `proof-pruner: pruned expired proof rows` at debug level. If it logs `proof-pruner: db mutex poisoned`, the node needs a restart.

---

## Disk Sizing Guidance

| Component | Growth Rate | Notes |
|-----------|------------|-------|
| SQLite database | ~1 MB per 1,000 ingested feeds | Includes feeds, tracks, routes, events, search index |
| Events table | ~3 KB per event | Append-only; grows indefinitely |
| WAL file | Up to ~1 MB during writes | Checkpointed automatically by SQLite |
| Signing key | 64 bytes | Static |
| TLS certificates | ~5 KB | Renewed in place |

For a deployment indexing 50,000 music feeds with ~500,000 tracks, expect the database to be 500 MB - 1 GB. The events table will be the largest component over time.

Recommendation: Start with 10 GB of disk. Add monitoring alerts at 80% utilization.

---

## Rolling Restart Procedure

### Single Node

```bash
systemctl restart stophammer
```

The node recovers its sync cursor from the database on startup. No data is lost.

### Primary + Community Network

1. **Restart community nodes first** (in any order). They will reconnect and catch up via fallback poll.
2. **Restart the primary last.** While the primary is down:
   - Community nodes cannot receive pushes
   - After `PUSH_TIMEOUT_SECS` (default 90s), they fall back to polling
   - After the primary restarts, community nodes re-register on their next poll cycle
3. To force immediate re-registration, restart the community nodes after the primary is up.

### Docker Compose

```bash
# Restart one node at a time
docker compose restart community1
docker compose restart community2
docker compose restart primary
```

---

## Common Issues and Fixes

### "CRAWL_TOKEN env var required"

The primary node requires `CRAWL_TOKEN`. Set it:

```bash
CRAWL_TOKEN=your-secret-token ./stophammer
```

### "PRIMARY_URL env var required in community mode"

Community nodes require `PRIMARY_URL` and `NODE_ADDRESS`. Set both:

```bash
NODE_MODE=community \
PRIMARY_URL=http://primary:8008 \
NODE_ADDRESS=http://this-node:8008 \
./stophammer
```

### "FATAL: cannot determine primary node public key"

The community node cannot reach `{PRIMARY_URL}/node/info` after 10 retries. Check:
- Is the primary running?
- Is the URL correct?
- Is there a firewall between the nodes?

Workaround: set `PRIMARY_PUBKEY` explicitly.

### "HTTPS required for pubkey auto-discovery"

Pubkey auto-discovery requires HTTPS to prevent MITM attacks. Either:
- Use an HTTPS `PRIMARY_URL`
- Set `PRIMARY_PUBKEY` explicitly
- For local dev only: `ALLOW_INSECURE_PUBKEY_DISCOVERY=true`

### "admin token not configured on this node"

All admin endpoints return 403 when `ADMIN_TOKEN` is empty. Set `ADMIN_TOKEN` to enable admin operations.

### "SYNC_TOKEN env var is not set" (community node warning)

Community nodes need `SYNC_TOKEN` to authenticate sync reads and writes with
the primary: `GET /sync/events`, `GET /sync/peers`, `POST /sync/register`, and
`POST /sync/reconcile`. `ADMIN_TOKEN` is not accepted on these sync endpoints.

### Push fan-out failures / peer eviction

Peers are evicted from the push cache after 10 consecutive failures. Common causes:
- Community node is down or unreachable
- Firewall blocking the push URL
- Community node's `NODE_ADDRESS` is wrong

Fix: restart the community node. It will re-register on next startup.

### "rate limit exceeded"

The node is returning 429 responses. Increase limits:

```bash
RATE_LIMIT_RPS=100 RATE_LIMIT_BURST=200 ./stophammer
```

Or if behind a proxy, ensure `TRUST_PROXY=true` so rate limiting uses the real client IP, not the proxy IP.

### Database mutex poisoned

A panic occurred in a database-accessing thread. The node must be restarted. This is a bug -- if reproducible, report it.

### TLS provisioning failed

Check that:
- Port 80 is reachable from the internet (ACME http-01 challenge)
- `TLS_DOMAIN` resolves to this server's IP
- You have not hit Let's Encrypt rate limits (50 certs/domain/week)

Use `TLS_ACME_STAGING=true` for testing to avoid rate limits.

### FTS5 search errors

If search queries return 400 with "invalid search query", the query contains FTS5 syntax errors. FTS5 uses its own query language; bare special characters like `*`, `"`, `OR` may cause parse failures. Sanitize user input before passing to `GET /v1/search`.
