# Pre-Sprint Audit Report

Audit date: 2026-03-13
Codebase: `/Volumes/T7/hey-v4v/stophammer/`
Auditor scope: all `src/*.rs`, `src/verifiers/*.rs`, `src/schema.sql`, `Cargo.toml`

---

## 1. Crawl token uses non-constant-time comparison

- **File:line**: `src/verifiers/crawl_token.rs:19`
- **Code**:
  ```rust
  if ctx.request.crawl_token == self.expected {
  ```
- **Severity**: HIGH
- **Status**: CONFIRMED
- **Description**: The crawl token verifier uses standard `==` string comparison, which is vulnerable to timing side-channel attacks. An attacker with network proximity could statistically determine the token character-by-character. The admin token check in `api.rs:1458-1460` already uses `subtle::ConstantTimeEq` (SHA-256 hash comparison), but the crawl token was missed.
- **Fix**: Hash both `ctx.request.crawl_token` and `self.expected` with SHA-256, then compare using `subtle::ConstantTimeEq`, matching the pattern used by `check_admin_token()` in `api.rs`.

---

## 2. Cargo.toml uses `edition = "2024"`

- **File:line**: `Cargo.toml:4`
- **Code**:
  ```toml
  edition = "2024"
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: Rust edition 2024 was stabilized in Rust 1.85 (2025-02-20). The project compiles, so the toolchain supports it. However, edition 2024 introduces several breaking changes (lifetime capture rules in opaque types, `unsafe_op_in_unsafe_fn` lint, `gen` keyword reservation, etc.). If the intent was to use 2021, this is a mistake. If intentional, this is fine but worth flagging since `.cast_signed()` (used at `db.rs:159`, `search.rs:60`) is a nightly method on integer types that was only stabilized in 2024 edition. This locks the project to a very recent minimum Rust version.
- **Fix**: Verify this is intentional. If CI targets stable Rust < 1.85, change to `edition = "2021"`. If the team is on 1.85+, no action needed but document the minimum Rust version.

---

## 3. `insert_event_idempotent` seq assignment is not inside a transaction

- **File:line**: `src/db.rs:1499-1518`
- **Code**:
  ```rust
  let sql = "INSERT OR IGNORE INTO events \
      (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8)";

  let changed = conn.execute(sql, ...)?;

  if changed == 0 {
      return Ok(None);
  }

  let seq: i64 = conn.query_row(
      "SELECT seq FROM events WHERE event_id = ?1", ...)?;
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: The `INSERT OR IGNORE` with `(SELECT COALESCE(MAX(seq),0)+1 FROM events)` and the subsequent `SELECT seq` are two separate statements without an explicit transaction. However, this function is only called from `apply_single_event` which already holds the database mutex. SQLite's single-writer mode means the mutex serializes all calls, so the subselect in the INSERT is safe from concurrent seq assignment. The follow-up `SELECT seq` is also safe because no other writer can interleave. **Safe in practice due to the global mutex, but fragile**: if the mutex is ever replaced with a connection pool or WAL concurrent writers, this would be a race condition.
- **Fix**: Wrap the INSERT + SELECT in a `conn.unchecked_transaction()` or use `RETURNING seq` (which is already used in `insert_event` at line 1226 and `ingest_transaction` at line 2075). This function does not use `RETURNING` and instead does a separate `SELECT`, which is an inconsistency. Change to `RETURNING seq` to match the rest of the codebase and make the function self-contained.

---

## 4. Community node pubkey auto-discovery over plain HTTP

- **File:line**: `src/main.rs:109-128`, `src/community.rs:184-209`
- **Code**:
  ```rust
  // main.rs:112-122
  let primary_pubkey_hex = if let Ok(pk) = std::env::var("PRIMARY_PUBKEY") {
      pk
  } else {
      let discovery_client = reqwest::Client::builder()
          .timeout(std::time::Duration::from_secs(5))
          .build()
          .expect("failed to build discovery client");
      // ...
      community::fetch_primary_pubkey(&discovery_client, &primary_url_for_discovery, 10)
          .await
          .expect(...)
  };
  ```
- **Severity**: HIGH
- **Status**: CONFIRMED
- **Description**: When `PRIMARY_PUBKEY` is not set, the community node fetches the primary's public key from `GET {PRIMARY_URL}/node/info`. If `PRIMARY_URL` uses `http://` (as it does in docker-compose development), the pubkey is transmitted in plaintext and susceptible to MITM interception. An attacker who intercepts this could substitute their own pubkey, causing the community node to accept events signed by an attacker-controlled key.
- **Fix**: (a) Log a WARN when the discovery URL is not HTTPS. (b) Consider requiring `PRIMARY_PUBKEY` to be set explicitly in production environments or at minimum requiring HTTPS for auto-discovery. (c) The `main.rs` line 325 already warns about plain HTTP for the node itself; extend similar treatment to the primary URL.

---

## 5. Search index + event insert are NOT in the same transaction (ingest)

- **File:line**: `src/api.rs:879-921`
- **Code**:
  ```rust
  // 11. Run ingest transaction
  let seqs = db::ingest_transaction(
      &mut conn, feed_artist, feed_artist_credit,
      feed.clone(), feed_routes, track_tuples, event_rows,
  )?;

  // 11b. Populate search index + compute quality scores
  {
      crate::search::populate_search_index(...)?;
      crate::quality::compute_feed_quality(...)?;
      // ...
  }

  // 12. Update crawl cache
  db::upsert_feed_crawl_cache(...)?;
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: `ingest_transaction` commits its transaction at `db.rs:2095`, and then the search index population, quality scoring, and crawl cache update happen as separate auto-committed statements. If the process crashes between the transaction commit and these follow-up writes, the data will be in the DB but the search index will be stale. The search index uses FTS5 `content=''` (contentless), so the row would be unfindable via search until the next re-ingest. Quality scores and crawl cache would also be missing.
- **Fix**: Either move the search/quality/cache writes into `ingest_transaction` (they run on the same `&mut Connection` so they can share the transaction), or accept this as a tolerable inconsistency and add a periodic re-indexing job as a safety net.

---

## 6. N+1 query pattern in query.rs: per-item credit loading in loops

- **File:line**: `src/query.rs:456-457`, `src/query.rs:887-888`
- **Code** (handle_get_artist_feeds):
  ```rust
  let mut feeds = Vec::with_capacity(items.len());
  for r in items {
      let credit = load_credit(&conn, r.credit_id)?;
      feeds.push(FeedResponse { ... });
  }
  ```
  Also at `handle_get_recent` (line 887-888), same pattern.
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: `load_credit` executes 2 SQL queries per call (one for the credit row, one for the credit names). For a page of 50 feeds, this is 100 extra queries beyond the initial list query. With SQLite's single-connection model this is sequential and fast in practice (microsecond-level per query), but it scales poorly and is a textbook N+1.
- **Fix**: Batch-load credits: collect all `credit_id` values from the result set, query `artist_credit` and `artist_credit_name` in two queries with `WHERE id IN (...)`, then build a `HashMap<i64, CreditResponse>` for O(1) lookup during response assembly.

---

## 7. Missing indexes in schema.sql

- **File:line**: `src/schema.sql` (entire file)
- **Severity**: MEDIUM (individual items LOW-MEDIUM)
- **Status**: CONFIRMED (all three missing)

### 7a. Missing: `artist_credit(LOWER(display_name))`

- **Line**: After line 62
- **Description**: `db.rs:500` queries `WHERE LOWER(display_name) = ?1`. Without an index on the expression, this is a full table scan of `artist_credit`. Called on every ingest via `get_or_create_artist_credit`.
- **Fix**: Add `CREATE INDEX IF NOT EXISTS idx_ac_display_lower ON artist_credit(LOWER(display_name));` (SQLite supports expression indexes since 3.31.0; bundled version is >= 3.45).

### 7b. Missing: `events(signed_by)`

- **Line**: After line 193
- **Description**: No index on `events.signed_by`. Currently no query filters on this column alone, but community nodes filter events by `signed_by` in application code (`community.rs:339`). If a query-level filter is ever added, it would need this index.
- **Fix**: LOW priority. Add index only if query patterns change.

### 7c. Missing: `proof_challenges(feed_guid, state)`

- **Line**: After line 430
- **Description**: The `handle_proofs_challenge` handler queries `SELECT COUNT(*) FROM proof_challenges WHERE feed_guid = ?1 AND state = 'pending'` (api.rs:1986). The existing index `idx_proof_challenges_feed` covers `feed_guid` but not `state`, so SQLite must scan all challenges for a given feed and filter by state. For a proof-flooding scenario (up to `MAX_PENDING_CHALLENGES_PER_FEED = 20` per feed), the scan is small. But a composite index would be more correct.
- **Fix**: Replace the existing index with `CREATE INDEX IF NOT EXISTS idx_proof_challenges_feed_state ON proof_challenges(feed_guid, state);`

---

## 8. `VerifierChain::run()` is not `#[must_use]`

- **File:line**: `src/verify.rs:123`
- **Code**:
  ```rust
  pub fn run(&self, ctx: &IngestContext) -> Result<Vec<String>, String> {
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: `VerifierChain::run()` returns a `Result<Vec<String>, String>` containing warnings or rejection reasons. If a caller ignores the return value, verification is silently skipped. `Result` itself has `#[must_use]` on the type, so Rust will warn if the return value is completely discarded. However, `VerifierChain::new()` at line 106 does have `#[must_use]`, creating an inconsistency.
- **Fix**: Add `#[must_use]` to `run()` for consistency and belt-and-suspenders safety. Note: Rust's built-in `#[must_use]` on `Result` already provides a warning, so this is truly LOW priority.

---

## 9. FTS5 delete-before-insert always attempted, even on first write

- **File:line**: `src/search.rs:91-99`
- **Code**:
  ```rust
  if let Err(e) = conn.execute(
      "INSERT INTO search_index(search_index, rowid, entity_type, entity_id, name, title, description, tags) \
       VALUES('delete', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      params![rowid, entity_type, entity_id, &*name, &*title, &*description, &*tags],
  ) {
      tracing::debug!(entity_type, entity_id, error = %e, "FTS5 pre-delete note");
  }
  ```
- **Severity**: LOW
- **Status**: CONFIRMED (intentional design, with appropriate error handling)
- **Description**: On first insert, the FTS5 `delete` command will fail because there is no existing row. The error is caught and logged at `debug` level. This is the documented pattern for contentless FTS5 tables -- there is no way to `SELECT` to check existence first. The approach is correct and the performance cost is negligible (one failed SQLite statement per first-time entity).
- **Fix**: No fix needed. The current implementation is the correct pattern. If the debug log is noisy, it could be changed to `trace` level, but this is cosmetic.

---

## 10. ADR 0018 reference and document existence

- **File:line**: `src/proof.rs:5`
- **Code**:
  ```rust
  //! See `docs/adr/0018-proof-of-possession-mutations.md` for the full design.
  ```
- **Severity**: LOW
- **Status**: ALREADY_FIXED
- **Description**: The reference to ADR 0018 exists in the proof.rs module doc. The file `docs/adr/0018-proof-of-possession-mutations.md` also exists on disk (confirmed via grep). No issue here.

---

## 11. `RouteType` enum has only 2 variants but schema allows 4

- **File:line**: `src/model.rs:102-106` vs `src/schema.sql:135`
- **Code** (model.rs):
  ```rust
  pub enum RouteType {
      Node,
      Lnaddress,
  }
  ```
  **Code** (schema.sql):
  ```sql
  route_type TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: The Rust `RouteType` enum defines only `Node` and `Lnaddress`. The SQLite schema allows `wallet` and `keysend` as well. If a feed arrives with `route_type = "wallet"` or `route_type = "keysend"`, the serde deserialization from the database will fail. Currently the ingest pipeline only accepts what the crawler sends, and the crawler presumably only sends `node` or `lnaddress`, but data imported from external sources or future crawlers could use `wallet` or `keysend`.
- **Fix**: Add `Wallet` and `Keysend` variants to the `RouteType` enum to match the schema, or tighten the schema CHECK constraint to only allow the values the application supports.

---

## 12. PATCH /feeds/{guid} and PATCH /tracks/{guid} don't emit events

- **File:line**: `src/api.rs:2220-2257` (handle_patch_feed), `src/api.rs:2267-2316` (handle_patch_track)
- **Code** (handle_patch_feed):
  ```rust
  if let Some(new_url) = &req.feed_url {
      conn.execute(
          "UPDATE feeds SET feed_url = ?1 WHERE feed_guid = ?2",
          params![new_url, guid2],
      )
      .map_err(|e| ApiError::from(db::DbError::from(e)))?;
  }
  Ok(())
  ```
- **Severity**: HIGH
- **Status**: CONFIRMED
- **Description**: Both PATCH handlers mutate the database directly without creating signed events. This means:
  1. Changes are invisible to the sync protocol -- community nodes will never learn about `feed_url` or `enclosure_url` changes.
  2. The mutation history has a gap -- there is no audit trail for these changes.
  3. This violates the event-sourcing design where every state change is recorded as a signed event.
- **Fix**: Create and sign a `FeedUpserted` or `TrackUpserted` event (or introduce new event types like `FeedPatched`/`TrackPatched`) and insert it within the same lock scope. Fan out to push subscribers after the lock is released, matching the pattern used by `handle_retire_feed`.

---

## 13. `handle_patch_feed` doesn't verify feed exists before UPDATE

- **File:line**: `src/api.rs:2239-2245`
- **Code**:
  ```rust
  if let Some(new_url) = &req.feed_url {
      conn.execute(
          "UPDATE feeds SET feed_url = ?1 WHERE feed_guid = ?2",
          params![new_url, guid2],
      )
      .map_err(|e| ApiError::from(db::DbError::from(e)))?;
  }
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: If the feed GUID doesn't exist, the UPDATE silently affects 0 rows and the handler returns `204 NO_CONTENT`. The caller gets no indication that nothing happened. Compare with `handle_retire_feed` which does a proper 404 check.
- **Fix**: Query for the feed first and return 404 if not found, or check `conn.changes()` after the UPDATE.

---

## 14. SSE polling loop uses 100ms busy-sleep instead of async select

- **File:line**: `src/api.rs:1393-1418`
- **Code**:
  ```rust
  loop {
      let mut any_received = false;
      for rx in &mut receivers {
          match rx.try_recv() {
              Ok(frame) => { ... any_received = true; }
              // ...
          }
      }
      if !any_received {
          tokio::time::sleep(std::time::Duration::from_millis(100)).await;
      }
  }
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: The SSE live stream uses polling with `try_recv()` and a 100ms sleep fallback. With 1,000 max SSE connections (the cap), this means up to 10,000 wakeups/second server-wide even when idle. The `tokio::sync::broadcast::Receiver` supports async `.recv()` which would block until a message arrives, eliminating the polling overhead entirely.
- **Fix**: Use `tokio::select!` over all receivers' `.recv()` futures, or use `tokio_stream::wrappers::BroadcastStream` to convert each receiver into a `Stream` and merge them with `futures::stream::select_all`. This eliminates the busy-wait entirely.

---

## 15. `main.rs` uses `.unwrap()` for TLS/HTTP server bind failures

- **File:line**: `src/main.rs:322`, `src/main.rs:327`, `src/main.rs:331`
- **Code**:
  ```rust
  axum_server::bind_rustls(addr, rustls_config)
      .serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>())
      .await
      .unwrap();
  // ...
  let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
  // ...
  axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
      .await
      .unwrap();
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: Server bind/serve failures panic with `.unwrap()` instead of producing a clean error message. Since these are in `main()` and the process must exit anyway, this is acceptable but produces ugly panic backtraces instead of clean error messages.
- **Fix**: Replace `.unwrap()` with `.expect("descriptive message")` to improve error diagnostics on failure, matching the pattern used elsewhere in main.rs.

---

## 16. `community.rs:260` reads `ADMIN_TOKEN` from env with `unwrap_or_default()`

- **File:line**: `src/community.rs:260`
- **Code**:
  ```rust
  let admin_token = std::env::var("ADMIN_TOKEN").unwrap_or_default();
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: In community mode, `register_with_primary` reads `ADMIN_TOKEN` and sends it as `X-Admin-Token` to the primary. If the env var is not set, an empty string is sent. The primary's `handle_sync_register` requires a valid admin token (checked by `check_admin_token` which rejects empty tokens), so the registration will be rejected with 403. This is a silent failure -- the community node logs "non-success" but doesn't make it clear that the token is missing.
- **Fix**: Log a WARNING at startup in community mode if `ADMIN_TOKEN` is not set, explaining that push registration will fail without it.

---

## 17. `apply_single_event` does search/quality writes outside any transaction

- **File:line**: `src/apply.rs:61-68`, `src/apply.rs:76-83`, `src/apply.rs:92-99`
- **Code** (example for ArtistUpserted):
  ```rust
  db::upsert_artist_if_absent(&conn, &p.artist)?;
  let score = crate::quality::compute_artist_quality(&conn, &p.artist.artist_id)?;
  crate::quality::store_quality(&conn, "artist", &p.artist.artist_id, score)?;
  crate::search::populate_search_index(&conn, "artist", &p.artist.artist_id, ...)?;
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: In `apply_single_event` (the community-node event apply path), the entity upsert, quality computation, search index update, and event insertion are all separate auto-committed statements. The function holds the mutex, so there's no concurrency issue, but a crash or error partway through leaves the DB in an inconsistent state (e.g., entity written but event not recorded, or entity + event written but search index not updated).
- **Fix**: Wrap the entire `apply_single_event` body (from the `match` block through `insert_event_idempotent`) in a single transaction.

---

## 18. CORS allows any origin for mutating endpoints

- **File:line**: `src/api.rs:422-423`
- **Code**:
  ```rust
  CorsLayer::new()
      .allow_origin(Any)
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: The CORS layer uses `allow_origin(Any)`, meaning any web page can make cross-origin requests to mutating endpoints (`POST /ingest/feed`, `DELETE /v1/feeds/{guid}`, `PATCH /v1/feeds/{guid}`, etc.). While all mutating endpoints require authentication (crawl token, admin token, or bearer token), the `Any` origin policy expands the attack surface for CSRF-like attacks if tokens are ever exposed to browser contexts.
- **Fix**: For the readonly query endpoints and SSE, `Any` is appropriate. For mutating endpoints, consider either: (a) a configurable `CORS_ALLOW_ORIGIN` env var, or (b) splitting the CORS layer so mutating routes have a restricted origin policy. LOW urgency since all mutations require non-cookie auth tokens.

---

## 19. `proof.rs:verify_podcast_txt` doesn't enforce Content-Length on streaming

- **File:line**: `src/proof.rs:265-286`
- **Code**:
  ```rust
  if let Some(cl) = resp.content_length() {
      if cl > MAX_RSS_BODY_BYTES as u64 {
          return Err(...);
      }
  }
  let bytes = resp.bytes().await.map_err(...)?;
  if bytes.len() > MAX_RSS_BODY_BYTES {
      return Err(...);
  }
  ```
- **Severity**: LOW
- **Status**: CONFIRMED (partially mitigated)
- **Description**: If the `Content-Length` header is absent (chunked transfer encoding), `reqwest` will buffer the entire response into memory before the post-hoc size check at line 281. A malicious feed server could stream a very large response without a Content-Length header, consuming server memory up to OOM before the check kicks in. The existing 10-second timeout at line 261 provides some protection, but a fast connection could deliver hundreds of MBs in 10 seconds.
- **Fix**: Use `resp.bytes()` with a size-limited reader, or `reqwest`'s body streaming API to read in chunks and abort if the accumulated size exceeds the limit. Alternatively, `reqwest::Response::chunk()` can be used in a loop.

---

## 20. `ingest_transaction` duplicates all entity upsert SQL from individual functions

- **File:line**: `src/db.rs:1854-2098`
- **Description**: `ingest_transaction` contains inline copies of the INSERT/UPDATE SQL for artists, feeds, tracks, payment routes, and events. The same SQL exists in `upsert_feed`, `upsert_track`, `replace_payment_routes`, `insert_event`, etc. If a schema column is added or renamed, both locations must be updated.
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: This is a maintainability concern, not a functional bug. The duplication exists because the standalone functions operate on `&Connection` while the transaction path operates on `&Transaction`, and rusqlite's `Transaction` derefs to `Connection` so the standalone functions *could* theoretically be called within the transaction.
- **Fix**: Refactor the standalone upsert functions to accept `&Connection` (which `Transaction` derefs to) and call them from within `ingest_transaction`, or extract the SQL strings as constants.

---

## 21. Search query passes raw user input to FTS5 MATCH

- **File:line**: `src/search.rs:175`
- **Code**:
  ```rust
  let rows = stmt.query_map(params![query, filter, limit, offset], |row| { ... })?;
  ```
  Where `query` comes from `params.q` (user input) at `query.rs:943`:
  ```rust
  crate::search::search(&conn, &q, kind.as_deref(), limit + 1, cursor_offset)
  ```
- **Severity**: MEDIUM
- **Status**: CONFIRMED
- **Description**: The FTS5 `MATCH` clause receives raw user input. FTS5 supports complex query syntax including boolean operators (`AND`, `OR`, `NOT`), prefix queries (`*`), column filters (`name:foo`), and `NEAR` clauses. A malformed FTS5 query (e.g., unclosed quotes or invalid syntax) will cause a SQLite error that surfaces as an HTTP 500 instead of a 400. Additionally, certain pathological FTS5 queries (e.g., very broad prefix matches like `a*`) can be expensive.
- **Fix**: Sanitize or validate the search query before passing to MATCH. At minimum, catch FTS5 parse errors and return 400. Consider wrapping the query in double quotes to force phrase search, or stripping FTS5 operators.

---

## 22. `proof.rs:validate_feed_url` does synchronous DNS resolution

- **File:line**: `src/proof.rs:404-416`
- **Code**:
  ```rust
  use std::net::ToSocketAddrs;
  let socket_addr = format!("{host}:{}", url.port_or_known_default().unwrap_or(443));
  if let Ok(addrs) = socket_addr.to_socket_addrs() {
      for addr in addrs {
          if is_private_ip(addr.ip()) {
              return Err(...);
          }
      }
  }
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: `std::net::ToSocketAddrs` performs synchronous DNS resolution, which blocks the current thread. This function is called in the async handler's flow (api.rs:2136, between Phase 1 and Phase 2 of `handle_proofs_assert`). Since it's not inside a `spawn_blocking`, it blocks the tokio worker thread during DNS resolution. For cached lookups this is fast, but for uncached domains on slow DNS servers it could block for seconds.
- **Fix**: Move the `validate_feed_url` call inside a `tokio::task::spawn_blocking` block, or use an async DNS resolver (e.g., `trust-dns-resolver` / `hickory-resolver`).

---

## 23. SSE connection guard uses `fetch_add`/`fetch_sub` without CAS

- **File:line**: `src/api.rs:96-102`
- **Code**:
  ```rust
  pub fn try_acquire_connection(&self) -> bool {
      let current = self.active_connections.load(Ordering::Relaxed);
      if current >= MAX_SSE_CONNECTIONS {
          return false;
      }
      self.active_connections.fetch_add(1, Ordering::Relaxed);
      true
  }
  ```
- **Severity**: LOW
- **Status**: CONFIRMED
- **Description**: The check-then-increment is not atomic. Two concurrent calls could both read `current = 999` (under the 1000 limit), both increment, and end up at 1001. With `Relaxed` ordering, this TOCTOU gap is wider. The comment in code acknowledges this: "worst case we allow a few extra connections momentarily." This is acceptable for a soft limit.
- **Fix**: Use `fetch_update` with `compare_exchange` if strict enforcement is desired. Given the soft-limit nature of this cap, the current approach is adequate.

---

## Summary by Severity

| Severity | Count | Issues |
|----------|-------|--------|
| CRITICAL | 0 | |
| HIGH     | 3 | #1 (crawl token timing), #4 (plaintext pubkey discovery), #12 (PATCH no events) |
| MEDIUM   | 9 | #2 (edition 2024), #3 (idempotent event no RETURNING), #5 (search outside tx), #6 (N+1), #7 (missing indexes), #11 (RouteType mismatch), #14 (SSE polling), #17 (apply no tx), #18 (CORS any), #21 (FTS5 raw input) |
| LOW      | 7 | #8 (run must_use), #9 (FTS5 delete), #10 (ADR exists), #13 (patch no 404), #15 (unwrap), #16 (community admin_token), #19 (streaming body), #20 (SQL duplication), #22 (sync DNS), #23 (SSE CAS) |
| ALREADY_FIXED | 1 | #10 (ADR 0018 reference) |
