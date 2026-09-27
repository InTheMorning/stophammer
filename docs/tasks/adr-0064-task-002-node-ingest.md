# ADR 0064 Task 002: The Node Stores And Limits Live Rows

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) sections 1, 3
and 4. Plan: [ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`.

## Goal

The node keeps `pending`, `live` and `ended` rows with the relay link. It makes
no track from a live item. It applies the limits of section 4, and it emits
`LiveEventsReplaced` only when the set of rows changes.

## Files To Inspect

- `src/ingest.rs`: `IngestLiveItemData`
- `src/model.rs`: `LiveEvent`
- `src/event.rs`: `LiveEventsReplacedPayload`
- `src/api.rs`: the ingest near `is_musicl`, the `live_events` list, the loop
  that promotes an `ended` item to a track, the `pub_dates` chain,
  `non_web_url_warnings`, `build_live_sse_frames_for_feed`
- `src/db.rs`: `MIGRATIONS`, `get_live_events_for_feed`,
  `replace_live_events_for_feed`, `live_events_changed`,
  `build_live_events_event`
- `src/apply.rs`: the arm for `LiveEventsReplaced`
- `src/schema.sql`: `live_events`
- `docs/adr/0046-migration-versions-are-array-positions.md`
- The [security review](../reviews/adr-0064-live-items-security-review.md)

## Files Likely To Change

- `migrations/0044_live_item_relay_link.sql`, new: `live_value_uri` and
  `live_value_protocol` on `live_events`
- `src/schema.sql`, `src/db.rs`, `src/ingest.rs`, `src/model.rs`,
  `src/event.rs`, `src/apply.rs`, `src/api.rs`
- `docs/schema-reference.md`
- `tests/adr0064_live_ingest_tests.rs`, new

## Steps

1. Add the two fields to the ingest DTO and to `LiveEvent`, with
   `#[serde(default)]`. An old crawler and an old event still deserialize.
2. Keep `ended` items as rows. Remove the promotion of an `ended` item to a
   track, and remove live items from the `pub_dates` chain.
3. Apply section 4 in this order:
   1. Keep the first item of each `live_item_guid`.
   2. Drop a `live` item with no relay link that has no `start`, no `end`, or
      an `end` more than 6 hours after its `start`. Give a warning.
   3. Keep the first 10 `pending` and `live` items in RSS order. Keep the 10
      `ended` items with the newest `start`. Give a warning for the others.
4. Add `content_link` and a `uri` that is a URL to `non_web_url_warnings`.
5. Sort the stored rows and the new rows by `live_item_guid` before
   `live_events_changed`. Compare the relay link too.
6. Change `live_event_ended` to fire when a row changes to `ended`. It no
   longer needs a track.

## Acceptance

Mechanical, each an integration test:

- An `ended` item makes an `ended` row and no track.
- The ban of step 3.2 drops each of its three cases, and keeps a `pending`
  item with the same times.
- A feed with 11 `live` items stores 10, and a feed with 11 `ended` items keeps
  the 10 newest, with a warning.
- A second ingest of the same items in a different order, or with a duplicate
  GUID, emits no `LiveEventsReplaced`.
- A `javascript:` `content_link` gives a warning.
- A community node applies a payload with and with no relay link.
- `cargo test --test migration_tests` passes.
- The gate is green.
