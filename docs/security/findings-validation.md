# Security Findings Validation Report

Validated 2026-03-13 against source at HEAD.

---

## Finding 1 (High): artist merge silently fails to transfer aliases

**Status: CONFIRMED**

The SQL in `merge_artists_sql` (src/db.rs lines 433-441 and 454-462) contains a correlated subquery bug. Both the SELECT and the UPDATE use:

```sql
SELECT 1 FROM artist_aliases
WHERE alias_lower = artist_aliases.alias_lower
  AND artist_id = ?2
```

Because the inner `FROM artist_aliases` shadows the outer table of the same name, the expression `alias_lower = artist_aliases.alias_lower` is a self-comparison that is always true. The `NOT EXISTS` subquery therefore reduces to "does any row exist in `artist_aliases` with `artist_id = ?2`". If the target artist has even one alias row, `NOT EXISTS` evaluates to false for every source row, and zero aliases are transferred or reported.

The test at tests/apply_tests.rs line 306 confirms the second part of the finding: it hand-simulates the merge with literal SQL (`UPDATE artist_aliases SET artist_id = 'artist-tgt' WHERE artist_id = 'artist-src'`) rather than calling `merge_artists_sql`, so it does not exercise the buggy subquery and passes despite the production code being broken.

**Severity: High.** Any merge where the target artist already has an alias will silently drop all source aliases.

---

## Finding 2 (High): mutation paths break signed event log -- state mutated before event written, outside one transaction

**Status: PARTIALLY_CONFIRMED**

The claim that state is mutated "outside one transaction" is partially accurate. The code operates on a shared `&Connection` (not `&mut Connection`) for PATCH handlers, which means it cannot open a rusqlite transaction (requires `&mut`). However, the entire sequence (UPDATE + insert_event) executes within a single mutex lock scope in `spawn_blocking`, which provides serialisation against other writers.

Specific evidence:

- **`handle_patch_feed`** (src/api.rs line 2269): executes a bare `conn.execute("UPDATE feeds ...")` followed by `db::insert_event(...)` (line 2324). Both are separate SQLite statements with no explicit transaction. If the process crashes between the UPDATE and the INSERT, the materialized state changes but no event is recorded.

- **`handle_patch_track`** (src/api.rs line 2430): same pattern -- bare `conn.execute("UPDATE tracks ...")` followed by `db::insert_event(...)` (line 2483).

- **`handle_admin_merge_artists`** (src/api.rs line 1497): calls `db::merge_artists(&mut conn, ...)` which runs inside its own transaction (src/db.rs line 418), then calls `db::insert_event(...)` (line 1521) outside that transaction. The merge commits before the event is written.

By contrast, `delete_feed_with_event` and `delete_track_with_event` (src/db.rs lines 1167, 1247) correctly wrap both the cascade-delete and the event INSERT in a single transaction.

The finding's description that "state is mutated before event written, outside one transaction" is accurate for all three paths. However, calling this a break of the "signed event log" overstates the blast radius: the single-writer mutex prevents concurrent inconsistency; the gap is crash-safety (process dies between the two statements), not concurrent corruption.

**Severity: Medium-high.** The crash window is real but narrow. The mutex prevents logical races. The fix is straightforward: wrap each PATCH handler's UPDATE + insert_event in a transaction, and move the merge event insert inside `merge_artists`'s existing transaction.

---

## Finding 3 (High): community node onboarding requires full admin credentials

**Status: CONFIRMED**

Evidence:

- `register_with_primary` (src/community.rs line 311-318) reads `ADMIN_TOKEN` from the environment and sends it as `X-Admin-Token` to the primary's `/sync/register` endpoint.

- `handle_sync_register` (src/api.rs line 1236) calls `check_admin_token(&headers, &state.admin_token)`, which is the same guard used by admin endpoints like `handle_admin_merge_artists` (line 1486), `handle_admin_add_alias` (line 1567), and all other `/admin/*` routes.

A community node operator must know the primary's `ADMIN_TOKEN`, which also grants them full admin access to merge artists, manage aliases, and any other admin-gated operation. There is no separate registration token or reduced-privilege credential for sync registration.

**Severity: High.** Any community node operator can perform arbitrary admin operations on the primary.

---

## Finding 4 (Medium-high): single global DB bottleneck defeats WAL

**Status: CONFIRMED**

Evidence:

- src/main.rs line 29: `let db = std::sync::Arc::new(std::sync::Mutex::new(conn));`
- src/db.rs line 17: `pub type Db = Arc<Mutex<Connection>>;`
- src/api.rs lines 398-414: `spawn_db` acquires `db.lock()` inside `spawn_blocking` for every read handler.
- src/api.rs lines 427+: `spawn_db_mut` does the same for write handlers.

The entire server shares a single `Arc<Mutex<rusqlite::Connection>>`. Every handler (read or write) acquires the mutex, which serialises all database access. SQLite WAL mode allows concurrent readers with one writer, but since the Rust mutex gates even read access through the single connection, concurrent reads are impossible.

**Severity: Medium-high.** This is an architectural scalability limitation. Under load, all requests queue behind a single mutex. The fix would require a connection pool or at minimum separate read/write connections.

---

## Finding 5 (Medium-high): POST /sync/reconcile not robust for large drift

**Status: CONFIRMED**

Evidence (src/api.rs lines 1033-1057):

```rust
let our_refs = db::get_event_refs_since(conn, req.since_seq)?;  // unbounded
// ...
let all_events = db::get_events_since(conn, req.since_seq, 10_000)?;  // capped at 10,000
let send_to_node: Vec<crate::event::Event> = all_events
    .into_iter()
    .filter(|e| missing_ids.contains(&e.event_id))
    .collect();
```

`get_event_refs_since` (src/db.rs line 1547) has no LIMIT -- it loads all event refs since `since_seq` into a `Vec`, then collects them into a `HashSet`. If drift is large (e.g., a new community node joining with `since_seq = 0`), this loads the entire event ref table into memory.

`get_events_since` is capped at 10,000, so if there are more than 10,000 missing events, only a subset are returned. There is no continuation token or pagination mechanism in the response -- the caller has no way to request the next batch.

**Severity: Medium-high.** For small clusters with bounded drift this works fine. For a new node joining a mature primary with hundreds of thousands of events, it will either OOM on the refs or silently truncate the result at 10,000 events with no continuation path.

---

## Finding 6 (Medium): proof-of-possession weaker than documented

**Status: CONFIRMED**

The ADR (docs/adr/0018-proof-of-possession-mutations.md, lines 186-201) explicitly requires proof at two locations:

> To assert, the owner must embed the full token binding string in **two places**:
> 1. **The RSS feed** -- in a `<podcast:txt>` tag at channel level.
> 2. **The audio file at the enclosure URL** -- as an ID3 `TXXX` frame...

The code at src/api.rs line 2156 only checks RSS:

```rust
let rss_verified = proof::verify_podcast_txt(&state.push_client, &feed_url, &token_binding)
    .await
```

There is no call to any audio verification function. The `proof.rs` module (src/proof.rs) does not contain a `verify_audio` function at all. After RSS verification, the code proceeds directly to resolving the challenge and issuing the token (lines 2178-2202).

Furthermore, once a bearer token is issued, `PATCH /v1/feeds/{guid}` allows changing `feed_url` (line 2270) and `PATCH /v1/tracks/{guid}` allows changing `enclosure_url` (line 2431). The ADR (lines 203-205) states that for relocations the token must appear "at both the old and the new location", but the PATCH handlers perform the URL update without any additional verification.

**Severity: Medium.** RSS-only verification is weaker than the documented two-factor proof. An RSS mirror operator (who can modify RSS but not audio) could pass the challenge. The relocation gap (no re-verification at new URL) is a separate concern.

---

## Finding 7 (Medium): verifier-chain configuration fails open on unknown names

**Status: CONFIRMED**

Evidence (src/verify.rs lines 246-249):

```rust
unknown => {
    tracing::warn!(verifier = %unknown, "unknown verifier in VERIFIER_CHAIN -- skipping");
    continue;
}
```

An unknown verifier name in the `VERIFIER_CHAIN` environment variable is logged and skipped. The chain is built without that verifier. If the only instance of `crawl_token` is misspelled (e.g., `crawl_tokn`), the resulting chain will have no authentication verifier, and all crawl submissions will bypass the shared secret check.

The default chain is hardcoded correctly at line 191-192: `"crawl_token,content_hash,medium_music,feed_guid,v4v_payment,enclosure_type"`. The risk is only when an operator customises `VERIFIER_CHAIN` and introduces a typo.

**Severity: Medium.** A typo in an env var silently disables security-critical verification steps. The mitigation would be to fail startup on unknown verifier names rather than skipping them.

---

## Finding 8 (Medium): test suite overstates coverage

**Status: CONFIRMED**

Evidence:

1. **Merge test** (tests/apply_tests.rs lines 306-327): The test comment says "Simulate merge: repoint credits, transfer aliases, record redirect, delete source. This mirrors db::merge_artists logic." It then executes hand-written SQL:
   ```rust
   conn.execute("UPDATE artist_aliases SET artist_id = 'artist-tgt' WHERE artist_id = 'artist-src'", [])
   ```
   This simple UPDATE unconditionally moves all aliases. It does not call `merge_artists_sql` or `merge_artists`, so it never exercises the buggy `NOT EXISTS` subquery. The test passes, but the production code silently fails (as confirmed in Finding 1).

2. **PATCH atomicity tests** (tests/auth_atomicity_tests.rs lines 319-353 and 360-389): `patch_feed_bearer_atomic` and `patch_track_bearer_atomic` both issue a valid PATCH request and verify the DB is updated and returns 204. They test only the happy path (successful update). Neither test simulates a failure mid-operation (e.g., event insertion failing after the UPDATE succeeds) to verify rollback behaviour. Given that the PATCH handlers do not use transactions (as confirmed in Finding 2), a rollback test would actually expose the gap.

**Severity: Medium.** The test suite creates confidence that is not warranted by what the tests actually exercise. The merge test masks a real bug; the atomicity tests only verify success, not failure recovery.

---

## Summary

| # | Finding | Status | Actual Severity |
|---|---------|--------|-----------------|
| 1 | Merge alias transfer SQL bug | CONFIRMED | High |
| 2 | PATCH/merge non-transactional mutation+event | PARTIALLY_CONFIRMED | Medium-high |
| 3 | Community node gets full admin credentials | CONFIRMED | High |
| 4 | Single-connection mutex defeats WAL | CONFIRMED | Medium-high |
| 5 | Reconcile unbounded memory + no pagination | CONFIRMED | Medium-high |
| 6 | Proof-of-possession lacks audio verification | CONFIRMED | Medium |
| 7 | Verifier chain fails open on typos | CONFIRMED | Medium |
| 8 | Tests mask bugs and miss failure paths | CONFIRMED | Medium |

Finding 2 was downgraded from "High" to "Medium-high" because the description claimed the operations happen "outside one transaction" which is accurate at the SQLite level, but the mutex provides serialisation that prevents concurrent corruption. The actual risk is crash-safety (process death between UPDATE and event INSERT), not logical races.
