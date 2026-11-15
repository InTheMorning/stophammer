# ADR 0098 Task 002: The Store Keeps Each Route Type And The Block Columns

Owner: [ADR 0098](../adr/0098-a-value-block-keeps-its-type-and-method.md) §2.

Repository: `stophammer`. This task has no dependency on task 001. Task 003
starts after this task.

## Goal

The node `RouteType` holds each text that a feed gives, and the store writes
and reads that text. A migration removes the `CHECK` on `route_type` and adds
the three block columns. This task does not fill the new columns.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Node route type | extend | `src/model.rs` | `RouteType` gets one value for each other text |
| Migration list | extend | `src/db.rs` | `MIGRATIONS` gets one entry |
| Table rebuild pattern | reuse | `migrations/0053_value_time_split_forms.sql` | It rebuilds a table and keeps the indexes and the triggers |
| Canonical schema | extend | `src/schema.sql` | It must agree with the migrated tables |

## Files To Inspect

- `src/model.rs`: `RouteType`
- `src/db.rs`: `MIGRATIONS`, `run_migrations`, `FOREIGN_KEYS_OFF_MARKER`,
  `route_type_from_db`, and each place that writes `route_type` with
  `serde_json::to_string` and `trim_matches('"')`
- `src/query.rs`: the two places that write `route_type` with the same pattern
- `migrations/0032_feed_scoped_track_identity.sql`,
  `migrations/0043_list_feed_items.sql`,
  `migrations/0053_value_time_split_forms.sql`,
  `migrations/0047_drop_artist_credit.sql`
- `src/schema.sql`
- `tests/migration_tests.rs`: `ALLOWED_DROP_TABLE_LINES`, the tests of
  migration 0053
- `tests/sg_schema_tests.rs`, `tests/sprint2b_issue_tests.rs`,
  `tests/adr0023_migration_hash_tests.rs`, `tests/adr0046_schema_sql_tests.rs`,
  `tests/fixtures/migration_sha256.txt`

## Files Likely To Change

- `src/model.rs`
- `src/db.rs`
- `src/query.rs`
- `src/schema.sql`
- New: `migrations/0065_value_block_fields.sql`
- `tests/fixtures/migration_sha256.txt`
- `tests/migration_tests.rs`
- `tests/sg_schema_tests.rs`
- `tests/sprint2b_issue_tests.rs`
- `docs/schema-reference.md`
- New: `tests/adr0098_route_type_store_tests.rs`

## Do Not Touch

- Each committed migration file.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the index and `AGENTS.md`.
- The API responses. Task 004 changes them.

## Constraints

- `RouteType` keeps `Node`, `Wallet`, `Keysend` and `Lnaddress` and gets
  `Other(String)`. It writes a plain JSON string and reads a plain JSON string.
  A known name reads as its known value. Each other string reads as `Other`.
- Give `RouteType` one function that returns its text as `&str`, and one
  function that makes a value from a `&str`. Each write to the database and
  each API response uses the first function. Remove the pattern with
  `serde_json::to_string` and `trim_matches('"')` for a route type.
- `route_type_from_db` uses the second function. It does not change an unknown
  text into `Node`, and it logs no error for one.
- The migration `0065_value_block_fields.sql` does these changes:
  - It removes `CHECK(route_type IN ('node','wallet','keysend','lnaddress'))`
    from `payment_routes`, `feed_payment_routes`,
    `value_time_split_recipients` and `feed_list_value_raw`.
  - It adds `value_type TEXT`, `value_method TEXT` and `block_index INTEGER`,
    each with no default and with `NULL` permitted, to those four tables and
    to `value_time_splits`.
  - Each existing row stays, with `NULL` in the three columns.
- SQLite cannot remove a `CHECK` in place, so the four tables are built again.
  Follow the pattern of migration 0053. Keep each column, primary key,
  default, `STRICT` setting, foreign key and other `CHECK`. Create each index
  again with its name: `idx_routes_track_0032`, `idx_routes_feed_0032`,
  `idx_routes_feed_track_0032`, `idx_feed_routes_guid`,
  `idx_feed_list_value_guid` and `idx_vts_recipients_split`. The triggers
  `trg_feeds_cleanup_before_delete` and `trg_tracks_cleanup_before_delete`
  must exist with the same body after the migration.
- `value_time_splits` needs no rebuild. Use `ALTER TABLE ... ADD COLUMN`.
- Append the migration to `MIGRATIONS`. Add its hash to
  `tests/fixtures/migration_sha256.txt`. Add its exact `DROP TABLE` lines to
  `ALLOWED_DROP_TABLE_LINES`.
- `src/schema.sql` gets the same five table changes.
- Delete the two `test_route_type_check_constraint_*` tests in
  `tests/sg_schema_tests.rs`. They assert the removed rule.
- Read the `rust-dev` skill rules that `AGENTS.md` names. Write each comment in
  Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a test.

1. A database at migration 0064 with one row in each of the four route tables
   and one row in `value_time_splits` migrates. Each row stays with the same
   values, and the three new columns are `NULL`.
2. After the migration, an `INSERT` with `route_type` `address` succeeds in
   each of the four route tables.
3. After the migration, the six indexes and the two triggers exist. A delete
   of a track removes its routes, its splits and its split recipients.
4. `RouteType::Other("address")` writes the JSON string `"address"` and reads
   back as the same value. `"keysend"` reads as `Keysend`.
5. A stored row with `route_type` `address` reads back from the store as
   `Other("address")`.
6. `tests/adr0046_schema_sql_tests.rs` and
   `tests/adr0023_migration_hash_tests.rs` pass.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files, the result of each check and each point where the code
did not let you follow this packet.
