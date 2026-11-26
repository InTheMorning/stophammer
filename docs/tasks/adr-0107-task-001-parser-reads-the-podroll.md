# ADR 0107 Task 001: The Parser Reads The Podroll

Owner: [ADR 0107](../adr/0107-a-feed-keeps-its-podroll.md) §1.

Repository: `stophammer-parser`, with the test feeds of `stophammer-crawler`
in a second commit. Each repository gets its own commit.

## Goal

`IngestFeedData` and the feed values give the field
`podroll: Vec<IngestRemoteFeedRef>`, filled from the `podcast:remoteItem`
children of each channel `podcast:podroll`. `remote_items` does not change.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Remote item type | reuse | `stophammer-parser/src/types.rs` | `IngestRemoteFeedRef` holds each value of a podroll entry |
| Remote item reader | extend | `stophammer-parser/src/engine.rs` | `append_remote_ref` reads one remote item. A podroll reader calls it with its own position counter |
| Feed values to ingest data | extend | `stophammer-parser/src/check.rs` | The values move into `IngestFeedData` as `remote_items` does |
| Crawler test feeds | extend | `stophammer-crawler/src/modes/gossip.rs` | Each literal `IngestFeedData` gets the new field |

## Acceptance Criteria

Each criterion is mechanical.

1. A channel with one remote item and a podroll of three items, one with no
   `feedGuid`, gives one `remote_items` entry and two `podroll` entries. Each
   podroll entry has its GUID, its URL, its medium and the positions 0 and 1.
   Test: `channel_podroll_gives_its_entries` in `tests/podroll.rs`.
2. A `podcast:podroll` in an item gives no podroll entry. Test:
   `item_podroll_gives_no_entry` in `tests/podroll.rs`.
3. `read` gives the same podroll as `parse`. Test:
   `read_gives_the_podroll` in `tests/podroll.rs`.
4. The gates of `stophammer-parser` and `stophammer-crawler` pass.
