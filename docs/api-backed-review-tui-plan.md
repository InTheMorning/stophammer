# Plan: API-backed Review TUIs

## Context

The `review_artist_identity_tui` (1,904 lines) and `review_wallet_identity_tui` (3,101 lines)
currently open the SQLite database directly. This requires SSH or local access to the node.
The goal is to let the TUIs run against a remote node via HTTP API + admin token, so operators
can review and resolve identity items from any machine.

This requires two phases: extending the API to cover every operation the TUIs perform, then
refactoring the TUIs to select between DB and API backends at startup.

---

## Phase 1 — New API Endpoints

### 1a. Source evidence for a feed (read-only)

The artist identity TUI builds a "feed evidence row" per artist feed, loading five tables
that have no API equivalent today. Add a single endpoint that returns all of them:

**`GET /admin/sources/feeds/{guid}/evidence`**

Response shape:
```json
{
  "source_feed_release_map": {
    "release_id": "...",
    "match_type": "...",
    "confidence": 95
  },
  "platform_claims": [ SourcePlatformClaim... ],
  "entity_links":    [ SourceEntityLink... ],
  "entity_ids":      [ SourceEntityIdClaim... ],
  "remote_items":    [ FeedRemoteItemRaw... ]
}
```

Handler calls:
- Raw SQL: `SELECT release_id, match_type, confidence FROM source_feed_release_map WHERE feed_guid = ?1`
- `db::get_source_platform_claims_for_feed(conn, guid)`
- `db::get_source_entity_links_for_entity(conn, "feed", guid)`
- `db::get_source_entity_ids_for_entity(conn, "feed", guid)`
- `db::get_feed_remote_items_for_feed(conn, guid)`

Files: `src/api.rs` (new handler + route + response struct)

### 1b. Single artist-identity review by ID (read-only)

The TUI loads the review record when you select a pending item. No GET-by-ID endpoint exists.

**`GET /admin/artist-identity/reviews/{id}`**

Response: `{ "review": ArtistIdentityReviewItem }`

Handler calls: `db::get_artist_identity_review(conn, id)`

Files: `src/api.rs`

### 1c. Wallet admin operations (write)

The wallet TUI performs five direct DB mutations that have no API equivalent:

| Endpoint | Body | DB function |
|---|---|---|
| `POST /admin/wallets/{id}/force-class` | `{"class": "..."}` | `db::set_wallet_force_class` |
| `POST /admin/wallets/{id}/force-confidence` | `{"confidence": "..."}` | `db::set_wallet_force_confidence` |
| `POST /admin/wallets/{id}/revert-classification` | _(none)_ | `db::revert_wallet_operator_classification` |
| `POST /admin/wallets/apply-merges` | _(none)_ | `db::backfill_wallet_pass5` |
| `POST /admin/wallets/undo-last-batch` | _(none)_ | `db::undo_last_wallet_merge_batch` |

All require `X-Admin-Token`. The last two are global operations (not wallet-scoped).

Files: `src/api.rs` (5 new handlers + routes + request/response structs)

### 1d. Coverage check — what the TUI needs vs what exists

After 1a–1c, every TUI DB call maps to an API endpoint:

| TUI operation | API source |
|---|---|
| List/stale/recent pending reviews | Existing `/admin/{type}/reviews/pending/*` |
| Summary, confidence, scores, age | Existing `…/pending/summary` + `/admin/reviews/pending/age-summary` |
| Dashboard + hotspots | Existing `/admin/reviews/dashboard` + `/admin/reviews/feeds/hotspots` |
| Resolve review (merge / do_not_merge) | Existing `POST …/reviews/{id}/resolve` |
| Get single review by ID | **New 1b** |
| Feed evidence (platform claims, entity IDs/links, remote items, release map) | **New 1a** |
| Artist identity plan for feed | Existing `GET /v1/diagnostics/feeds/{guid}` (returns `artist_identity_plan`) |
| Artist info, feeds, releases, external IDs | Existing `GET /v1/diagnostics/artists/{id}` |
| Feed URL for guid | Existing `GET /v1/feeds/{guid}` (has `feed_url` in response) |
| Wallet detail + claim feeds + alias peers | Existing `GET /v1/diagnostics/wallets/{id}` |
| Wallet force class/confidence/revert | **New 1c** |
| Apply merges / undo batch | **New 1c** |
| `pair_already_reviewed` check | Client-side: fetch wallet diagnostics → check reviews |

---

## Phase 2 — TUI Backend Abstraction

### 2a. Define the `ReviewBackend` trait

Create `src/review_backend.rs` with a trait covering every data operation the TUIs need.
Group methods by domain rather than by DB function:

```rust
pub trait ReviewBackend {
    // --- Artist identity reviews ---
    fn list_pending_artist_reviews(&self, limit: usize, confidence: Option<&str>, min_score: Option<u16>) -> Result<Vec<ArtistIdentityPendingReview>>;
    fn list_stale_artist_reviews(&self, min_age_secs: i64, limit: usize) -> Result<Vec<ArtistIdentityPendingReview>>;
    fn list_recent_artist_reviews(&self, max_age_secs: i64, limit: usize) -> Result<Vec<ArtistIdentityPendingReview>>;
    fn get_artist_review(&self, id: i64) -> Result<Option<ArtistIdentityReviewItem>>;
    fn resolve_artist_review(&self, id: i64, action: &str, target: Option<&str>, note: Option<&str>) -> Result<ArtistIdentityReviewActionOutcome>;
    fn explain_artist_identity_for_feed(&self, feed_guid: &str) -> Result<ArtistIdentityFeedPlan>;

    // --- Wallet identity reviews ---
    fn list_pending_wallet_reviews(&self, limit: usize) -> Result<Vec<WalletReviewSummary>>;
    fn list_stale_wallet_reviews(&self, min_age_secs: i64, limit: usize) -> Result<Vec<WalletReviewSummary>>;
    fn list_recent_wallet_reviews(&self, max_age_secs: i64, limit: usize) -> Result<Vec<WalletReviewSummary>>;
    fn resolve_wallet_review(&self, id: i64, action: &str, target_id: Option<&str>, value: Option<&str>) -> Result<WalletIdentityReviewActionOutcome>;
    fn get_wallet_alias_peers(&self, alias: &str) -> Result<Vec<WalletAliasPeer>>;
    fn get_wallet_detail(&self, id: &str) -> Result<Option<WalletDetail>>;
    fn get_wallet_claim_feeds(&self, id: &str) -> Result<Vec<WalletClaimFeed>>;
    fn set_wallet_force_class(&self, id: &str, class: &str) -> Result<()>;
    fn set_wallet_force_confidence(&self, id: &str, confidence: &str) -> Result<()>;
    fn revert_wallet_classification(&self, id: &str) -> Result<()>;
    fn apply_wallet_merges(&self) -> Result<WalletRefreshStats>;
    fn undo_last_wallet_batch(&self) -> Result<Option<WalletUndoStats>>;

    // --- Summaries / dashboard ---
    fn artist_review_summary(&self) -> Result<(Vec<ArtistIdentityPendingReviewSummary>, Vec<PendingReviewConfidenceSummary>, Vec<PendingReviewScoreSummary>)>;
    fn wallet_review_summary(&self) -> Result<(Vec<WalletPendingReviewSummary>, Vec<PendingReviewConfidenceSummary>, Vec<PendingReviewScoreSummary>)>;
    fn review_age_summary(&self) -> Result<(PendingReviewAgeSummary, PendingReviewAgeSummary)>;
    fn feed_hotspots(&self, limit: usize) -> Result<Vec<PendingReviewFeedHotspot>>;

    // --- Evidence lookups ---
    fn feed_url(&self, feed_guid: &str) -> Result<String>;
    fn artist_info(&self, artist_id: &str) -> Result<Option<(String, String, i64)>>; // id, name, created_at
    fn feeds_for_artist(&self, artist_id: &str) -> Result<Vec<(String, String, String)>>; // guid, title, url
    fn feed_count_for_artist(&self, artist_id: &str) -> Result<i64>;
    fn release_count_for_artist(&self, artist_id: &str) -> Result<i64>;
    fn external_ids_for_artist(&self, artist_id: &str) -> Result<Vec<(String, String)>>; // scheme, value
    fn feed_evidence(&self, feed_guid: &str) -> Result<FeedEvidence>; // new composite struct
}
```

Files: new `src/review_backend.rs`, add `pub mod review_backend;` to `src/lib.rs`

### 2b. `DbBackend` — wraps the current `Connection`

Implement the trait by calling the existing `db::` functions directly. This is a mechanical
translation of what the TUIs do today. The TUIs' local helper functions
(`feed_url_for_guid`, `feed_evidence_row`, etc.) move into this impl.

Files: `src/review_backend.rs` (impl block)

### 2c. `ApiBackend` — wraps `reqwest::blocking::Client`

Implement the trait using blocking HTTP calls to the node API. Uses `reqwest::blocking`
(already available — add the `blocking` feature to the existing `reqwest` dep).

```rust
pub struct ApiBackend {
    client: reqwest::blocking::Client,
    base_url: String,
    admin_token: String,
}
```

Each method maps to one or a few HTTP calls. Key mappings:

| Trait method | HTTP call |
|---|---|
| `list_pending_artist_reviews` | `GET /admin/artist-identity/reviews/pending` |
| `resolve_artist_review` | `POST /admin/artist-identity/reviews/{id}/resolve` |
| `explain_artist_identity_for_feed` | `GET /v1/diagnostics/feeds/{guid}` → extract `artist_identity_plan` |
| `artist_info` | `GET /v1/diagnostics/artists/{id}` → extract `artist` fields |
| `feeds_for_artist` | `GET /v1/diagnostics/artists/{id}` → extract `feeds` |
| `feed_evidence` | `GET /admin/sources/feeds/{guid}/evidence` (new 1a) |
| `artist_review_summary` | `GET /admin/artist-identity/reviews/pending/summary` |
| `review_age_summary` | `GET /admin/reviews/pending/age-summary` |
| `set_wallet_force_class` | `POST /admin/wallets/{id}/force-class` (new 1c) |
| `apply_wallet_merges` | `POST /admin/wallets/apply-merges` (new 1c) |

Files: `src/review_backend.rs` (impl block)

### 2d. Refactor TUI binaries to use the trait

Change `App` struct in both TUI binaries:
- Replace `conn: Connection` with `backend: Box<dyn ReviewBackend>`
- Replace all direct `db::` calls and raw SQL with `self.backend.method()` calls
- This is mechanical — the method names and signatures are designed to match the current call sites

Files: `src/bin/review_artist_identity_tui.rs`, `src/bin/review_wallet_identity_tui.rs`

### 2e. CLI argument handling

Both TUI binaries accept a `--db-path` argument today. Add:

```
--node <URL>          Node base URL (or NODE env var)
--admin-token <TOKEN> Admin token (or ADMIN_TOKEN env var)
```

If `--node` is provided (or `NODE` is set), use `ApiBackend`. Otherwise, use `DbBackend`
with the existing `--db-path` logic. The two modes are mutually exclusive — error if both
`--node` and `--db-path` are given.

Files: `src/bin/review_artist_identity_tui.rs`, `src/bin/review_wallet_identity_tui.rs`

---

## File Change Summary

| File | Change |
|---|---|
| `Cargo.toml` | Add `blocking` feature to reqwest |
| `src/lib.rs` | Add `pub mod review_backend;` |
| `src/review_backend.rs` | **New** — trait + `DbBackend` + `ApiBackend` |
| `src/api.rs` | 7 new endpoint handlers + routes + structs |
| `src/bin/review_artist_identity_tui.rs` | Replace `conn` with `backend`, add CLI args |
| `src/bin/review_wallet_identity_tui.rs` | Replace `conn` with `backend`, add CLI args |
| `docs/API.md` | Document new endpoints |
| `docs/operations.md` | Document `--node` / `NODE` env var for TUIs |

---

## Verification

1. **New endpoints**: `cargo test` — add integration tests for each new endpoint
2. **DB mode preserved**: Run TUIs without `--node` — behaviour must be identical to today
3. **API mode**: Start a node with `ADMIN_TOKEN=test`, then:
   ```bash
   review_artist_identity_tui --node http://localhost:8080 --admin-token test
   review_wallet_identity_tui --node http://localhost:8080 --admin-token test
   ```
   List reviews, inspect detail, merge/block, verify dashboard updates.
4. **CI**: `cargo clippy -- -D warnings && cargo fmt -- --check`
