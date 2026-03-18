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
| `ADMIN_TOKEN` | `""` (empty) | No | Token for admin endpoints (`X-Admin-Token` header). If empty, all admin endpoints return 403. Also used as the legacy fallback credential for sync registration/reconcile when `SYNC_TOKEN` is not configured. |
| `SYNC_TOKEN` | unset | No | Dedicated token for `POST /sync/register` and `POST /sync/reconcile` (`X-Sync-Token` header). When set, it replaces `ADMIN_TOKEN` for those sync endpoints. |
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
| `VERIFIER_CHAIN` | `crawl_token,content_hash,medium_music,feed_guid,v4v_payment,enclosure_type` | Comma-separated ordered list of verifiers to run on ingest. Primary only. See the [Verifier Guide](verifier-guide.md). |

---

## Startup Modes

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

**Required env vars:** `CRAWL_TOKEN`

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
curl -s http://primary:8008/sync/events?after_seq=999999&limit=1 | jq .next_seq

# Community
curl -s http://community:8009/sync/events?after_seq=999999&limit=1 | jq .next_seq
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
curl -s http://primary:8008/sync/peers | jq
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

### "Neither SYNC_TOKEN nor ADMIN_TOKEN env var is set" (community node warning)

Community nodes need `SYNC_TOKEN` to authenticate `POST /sync/register` and
`POST /sync/reconcile` with the primary. If `SYNC_TOKEN` is unset on the
primary, `ADMIN_TOKEN` still works as a deprecated fallback.

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
