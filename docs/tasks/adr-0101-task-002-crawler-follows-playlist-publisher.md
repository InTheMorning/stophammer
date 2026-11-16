# ADR 0101 Task 002: The Crawler Follows The Link Between A Publisher And A Playlist

Owner: [ADR 0101](../adr/0101-a-publisher-feed-can-list-playlists.md) §2, §3
and §4.

Repository: `stophammer-crawler`, in the folder `stophammer-crawler/` of the
`stophammer` checkout. It has its own Git repository. This task does not need
task 001.

## Goal

- A publisher feed gives the URL of each channel item with the medium `music`
  or `musicL`.
- A `musicL` feed also gives the URL of its publisher reference, and no other
  `medium="publisher"` item.
- The skip store does not skip a URL for an earlier `[medium_music]` rejection
  when the stored medium of that URL is `publisher`.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Follow rules | extend | `stophammer-crawler/src/follow.rs` | `all_follow_urls` holds the item mediums that each feed medium follows |
| Follow levels | reuse | `stophammer-crawler/src/follow.rs` | `follow_urls_at_level` already stops a `musicL` feed at the publisher level |
| Publisher reference flag | reuse | `stophammer-parser/src/types.rs` | `IngestRemoteFeedRef::publisher_reference` marks the reference item |
| Skip store | extend | `stophammer-crawler/src/feed_skip.rs` | `should_skip` holds the medium-gate rule |

## Files To Inspect

- `stophammer-crawler/src/follow.rs`: `follow_urls_at_level`, `follow_urls`,
  `all_follow_urls`, the doc comments and the tests
- `stophammer-parser/src/types.rs`: `IngestRemoteFeedRef`
- `stophammer-parser/src/engine.rs`: `extract_feed_remote_items`, for the
  meaning of `publisher_reference`
- `stophammer-crawler/src/feed_skip.rs`: `should_skip`, `is_index_medium`
  and the tests
- `stophammer-crawler/src/modes/batch.rs` and
  `stophammer-crawler/src/modes/gossip.rs`: the callers of
  `follow_urls_at_level`. Read them only. They do not change.

## Files Likely To Change

- `stophammer-crawler/src/follow.rs`
- `stophammer-crawler/src/feed_skip.rs`

## Do Not Touch

- `MAX_FOLLOW_URLS_PER_FEED`, `MAX_FOLLOW_URLS_PER_LIST_FEED` and the pass
  limit.
- `FollowLevel` and the level of each fetch in `modes/`.
- `stophammer-parser/` and each file outside `stophammer-crawler/`.

## Constraints

- In `all_follow_urls`:
  - A `publisher` feed accepts an item with the medium `music` or `musicL`,
    with the ASCII case ignored. An item with no medium stays skipped.
  - A `musicL` feed keeps its rule for `music` items and items with no medium.
    It also accepts the one item with `publisher_reference` set to `true`.
  - A `musicL` feed accepts no other item with `medium="publisher"`.
  - The order of the URLs is the order of the items. Duplicates are removed as
    before.
- The report line of `follow.rs` for a list item with no `feedUrl` counts only
  `music` items and items with no medium. A publisher reference with no
  `feedUrl` adds no count.
- In `should_skip`: a `[medium_music]` rejection skips the URL only when the
  stored `raw_medium` is not `publisher`, with the ASCII case ignored. The
  rule for each other stored medium does not change.
- Update the module comment and the doc comments of `follow.rs` and the comment
  of the rule in `feed_skip.rs`. Each changed comment names `stophammer`
  ADR 0101 and its section.
- Replace the test `a_musicl_feed_with_a_publisher_item_gives_nothing_for_it`.
  Do not keep a test that asserts the old rule.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment in Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a unit test in the file that it names.

1. `follow.rs`: a publisher feed with one `music` item and one `musicL` item
   gives the two URLs in item order.
2. `follow.rs`: a publisher feed with an item with `medium="MusicL"` gives its
   URL.
3. `follow.rs`: a `musicL` feed with a publisher reference and a second
   `medium="publisher"` item gives only the URL of the reference. This test
   replaces `a_musicl_feed_with_a_publisher_item_gives_nothing_for_it`.
4. `follow.rs`: a `musicL` feed at `FollowLevel::Publisher` gives nothing, also
   with a publisher reference.
5. `follow.rs`: a publisher feed at `FollowLevel::Publisher` gives its `musicL`
   URLs.
6. `feed_skip.rs`: a `Rejected` report with the reason
   `[medium_music] publisher feed has no remoteItem with medium='music'` and the
   medium `publisher` does not skip.
7. `feed_skip.rs`: `skips_medium_gate_rejection` and
   `skips_prior_skipped_known_irrelevant_medium_gate_rows` pass with no change.

## Checks

Run each check in `stophammer-crawler/`:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files and the result of each check. Name each point where the
code did not let you follow this packet.
