# ADR 0107 Task 002: The Node Keeps The Podroll

Owner: [ADR 0107](../adr/0107-a-feed-keeps-its-podroll.md) §2, §3 and §4.

Repository: `stophammer`. It needs task 001, because the node reads the
parser type `IngestRemoteFeedRef` through its own `IngestFeedData`.

## Goal

The node stores the podroll of each feed in `feed_podroll_items`, sends it in
`FeedRemoteItemsReplaced`, and gives it in `GET /v1/feeds/{guid}` with
`include=podroll`.

## Steps

1. `src/ingest.rs`: `IngestFeedData` gets
   `#[serde(default)] pub podroll: Vec<IngestRemoteFeedRef>`.
2. A migration `migrations/0067_feed_podroll_items.sql` and `src/schema.sql`
   add the table of ADR 0107 §2. Add its line to
   `tests/fixtures/migration_sha256.txt`.
3. `src/model.rs`: a new struct `FeedPodrollItem` with `id`, `feed_guid`,
   `position`, `medium`, `remote_feed_guid` and `remote_feed_url`, with the
   derives of `FeedRemoteItemRaw`.
4. `src/event.rs`: `FeedRemoteItemsReplacedPayload` gets
   `#[serde(default, skip_serializing_if = "Option::is_none")] pub podroll: Option<Vec<FeedPodrollItem>>`.
5. `src/db.rs`:
   - `get_feed_podroll_for_feed` and `replace_feed_podroll`, adjacent to the
     functions of `feed_remote_items_raw`.
   - `ingest_transaction` gets the podroll rows, replaces them, and gives
     the event when the remote items or the podroll changed. The event
     carries `Some` with the full podroll.
   - Each `DELETE FROM feed_remote_items_raw WHERE feed_guid` path also
     deletes the podroll rows.
   - `GUID_CHANGE_COLUMNS` and `GUID_CHANGE_STAYS` get the table.
6. `src/api.rs`: the ingest handler maps `feed_data.podroll` into
   `FeedPodrollItem` rows and gives them to `ingest_transaction`.
7. `src/apply.rs`: the `FeedRemoteItemsReplaced` arm replaces the podroll
   rows when the payload has `Some`.
8. `src/query.rs`: `FEED_INCLUDES` gets `podroll`. `FeedResponse` gets
   `podroll: Option<Vec<FeedPodrollItemResponse>>`, with the fields of ADR
   0107 §4. The summary of the named feed uses `db::resolve_listed_feed` and
   the same helpers that `feed_remote_item_response` uses for
   `remote_feed_title`, `remote_feed_image_url` and
   `remote_release_artist`. Register the schema for OpenAPI.
9. `tests/fixtures/v1-schema-fields.json`: add `podroll` to `FeedResponse`,
   and add `FeedPodrollItemResponse` with its fields.
10. `docs/API.md`: add `podroll` to each list of feed include values, and
    describe the entry fields.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Ingest DTO | extend | `src/ingest.rs` | `IngestFeedData` gets the field |
| Remote items event | extend | `src/event.rs` | `FeedRemoteItemsReplacedPayload` carries the podroll (ADR 0107 §3) |
| Remote items storage | extend | `src/db.rs` | The podroll functions go adjacent to the functions of `feed_remote_items_raw` and follow them |
| Event apply | extend | `src/apply.rs` | The existing arm also replaces the podroll |
| Feed route includes | extend | `src/query.rs` | `FEED_INCLUDES` and `FeedResponse` get `podroll` |
| Named feed summary | reuse | `src/db.rs` | `resolve_listed_feed` finds the named feed, as for a remote item |
| GUID change guard | reuse | `tests/adr0106_guid_change_in_place_tests.rs` | The guard of the migrated schema covers the new table |
| Field removal guard | extend | `tests/fixtures/v1-schema-fields.json` | The recorded fields get the new field |

## Acceptance Criteria

Each criterion is mechanical. Each test goes in
`tests/adr0107_podroll_tests.rs`, except where the line names another file.

1. An ingest of a feed with a podroll of two entries stores two rows, and
   `GET /v1/feeds/{guid}?include=podroll` gives them in position sequence.
   An entry that names a held feed gives its title. An entry that names no
   held feed gives null. Test: `ingest_keeps_the_podroll`.
2. A publisher feed with a podroll entry of `medium="music"` gives no
   `remote_items` entry and no album for that entry in the publisher view.
   Test: `podroll_entry_is_not_a_remote_item`.
3. A second body with the same remote items and a changed podroll gives a
   `FeedRemoteItemsReplaced` event with the new podroll. Test:
   `changed_podroll_gives_the_event`.
4. A node that applies that event has the same podroll rows. An event with no
   `podroll` field leaves the podroll rows of the node. Test:
   `apply_replaces_the_podroll_only_with_the_field`.
5. A retire deletes the podroll rows of the feed. Test:
   `retire_deletes_the_podroll`.
6. A GUID change moves the podroll rows. `change_feed_guid_sql` moves each
   column of `GUID_CHANGE_COLUMNS`, and it is not public. Thus the test is
   the guard `the_guid_change_column_list_covers_the_migrated_schema` in
   `tests/adr0106_guid_change_in_place_tests.rs`, which fails for a
   `feed_guid` column that is in neither list.
7. The same podroll in a new body gives no `FeedRemoteItemsReplaced` event.
   Test: `same_podroll_gives_no_event`.
8. The ADR 0106 guard of the migrated schema, the ADR 0044 field guard and the
   migration hash guard pass.
9. The gate passes: `cargo build`, `cargo test`, `cargo clippy --all-targets
   -- -D warnings` and `cargo fmt --check`.
