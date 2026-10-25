# Summary Rows Phase Plan

Owner: [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md)
§6 and §7, accepted on 2026-10-25. This plan binds nothing. The ADR owns each
rule. musicindex.org requests 17 and 18 are in
[the musicindex.org requests](musicindex-open-requests.md).

## Goal

- A row of `GET /v1/copies` gives the image of its feed.
- A row of `GET /v1/live-items` gives the title and the image of its feed.
- A remote value time split gives the summary of its feed and its track.

## Non-Goals

- No change to `GET /v1/feeds/{guid}/copies`, to a local split, or to the
  fields that a row gives now.
- No stored summary, no migration, no event and no index.
- No change to `stophammer-crawler` or `stophammer-parser`.

## Assumptions

- `live_events.feed_guid` references `feeds`. Thus a live row has a feed, but
  the code still gives null when the read finds no feed.
- A split gives no URL. The GUID step of `resolve_listed_feed` is a read of
  `feeds` by primary key, so `named_feed_summary` with the GUID gives the same
  feed.

## Affected Modules

- `src/query.rs`: `CopyRecordResponse`, `handle_list_copy_records`,
  `LiveItemListResponse`, `live_item_list_response`, `VtsResponse`,
  `vts_response_from_split`, `named_feed_summary` and
  `feed_remote_item_response`.
- `tests/fixtures/v1-schema-fields.json`, `docs/API.md` and new tests.

## Sequence

| Task | Section | Depends on |
|---|---|---|
| [003](../tasks/adr-0059-task-003-list-row-feed-image.md) | §6 | Nothing |
| [004](../tasks/adr-0059-task-004-split-summary.md) | §7 | Nothing |

The two tasks change different structures. They can run in each sequence.
Run them one at a time, because each one edits `src/query.rs`.

## Schema And API Implications

- Each new field is an added `v1` field (ADR 0044 §4). No field changes its
  name, its type or its meaning.
- Each new property goes into `tests/fixtures/v1-schema-fields.json`.
- `src/openapi.rs` points at each schema with `$ref`, so the generated schema
  gives the new fields with no change there.

## Risk Areas

- A copy row must give the image of the indexed feed, not of a copy.
- A split row with `remote_item_guid` `""` must give null track fields.
- `vts_response_from_split` is a pure function now. Task 004 gives it a
  connection. Its other behavior must not change.

## Test Strategy

Each acceptance criterion is a mechanical test through the HTTP router. No
criterion is visual. The musicindex.org operator can examine the Copies, Live
and track pages after the deploy, but that check is not a gate of this plan.

## Rollback

Each task adds fields only. A rollback is a deploy of the release before it,
with no restore.
