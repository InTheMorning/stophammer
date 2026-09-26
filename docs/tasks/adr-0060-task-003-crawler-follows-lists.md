# ADR 0060 Task 003: The Crawler Follows A List

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md) sections 5
and 6. Plan: [ADR 0060 phase plan](../plans/adr-0060-list-feeds-phase-plan.md).

Repository: `stophammer-crawler`. The crate is in `stophammer-crawler/`. Read
`stophammer-crawler/AGENTS.md`.

## Goal

A `musicL` feed gives the `feedUrl` of each music entry as a follow URL, at
most 1,000. A feed reached through a list gives no follow URL of its own.

## Files To Inspect

- `stophammer-crawler/src/follow.rs`: `follow_urls`, `all_follow_urls`,
  `follow_urls_at_level`, `MAX_FOLLOW_URLS_PER_FEED`, and its tests,
  including `a_medium_l_feed_gives_nothing`
- `stophammer-crawler/src/modes/batch.rs`: the three waves and their levels
- `stophammer-crawler/src/modes/gossip.rs`: the two levels
- `stophammer-crawler/AGENTS.md`: the text on following

## Files Likely To Change

- `stophammer-crawler/src/follow.rs`
- `stophammer-crawler/AGENTS.md`

## Do Not Touch

- `stophammer`, `stophammer-parser`
- The follow rule of a music feed and of a publisher feed
- The pass limit of 50,000

## Constraints

- **The list rule.** Compare each medium with ASCII case ignored. A feed
  with the `raw_medium` `musicL` gives the `remote_feed_url` of each channel
  remote item with the `medium` `music` or with no `medium`. An item with another
  `medium` gives nothing. The URL checks of `is_followable_url` and the
  removal of duplicates apply, as for the other mediums. The
  `itunes:new-feed-url` rule does not change.
- **The limit.** Add `MAX_FOLLOW_URLS_PER_LIST_FEED: usize = 1_000`. A
  `musicL` feed is cut at that value, and each other feed at
  `MAX_FOLLOW_URLS_PER_FEED`. The log line names the limit that applied.
- **No URL.** Count the music items of a `musicL` feed that give no
  `remote_feed_url`. When the count is more than zero, log it once with the
  fetched URL.
- **One level.** A feed reached through a list is fetched at the next level:
  `FollowLevel::Publisher` in wave 2 of `batch.rs` and in `gossip.rs`. At
  that level, only a feed whose medium is `publisher` gives URLs. The list
  rule gives only music entries, so a feed reached through a list gives no
  follow URL.

  Do not add a level. Confirm this with a test.
- Replace `a_medium_l_feed_gives_nothing` with tests of the new rule.
- Update the follow text of `stophammer-crawler/AGENTS.md`: name ADR 0060 as
  the owner of the list rule. Write in ASD-STE100 Simplified Technical
  English, and run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics stophammer-crawler/AGENTS.md`.

## Acceptance Criteria

Mechanical. Tests in `stophammer-crawler/src/follow.rs`. Each failure message
names ADR 0060 and its guard:

- A `musicL` feed with one music item with a `feedUrl` gives that URL.
- A `musicL` feed with an item with no `medium` gives its URL.
- A `musicL` feed with a `publisher` item gives nothing for it.
- A `musicL` feed with 1,200 different URLs gives 1,000.
- A music feed with 300 publisher links gives 200. The present test of 201
  links stays.
- A `musicL` feed with 30 items for one URL gives one URL.
- `follow_urls_at_level` with `FollowLevel::Publisher` on a music feed that
  names a publisher gives nothing.
- The crawler gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

## Expected Final Report

1. files changed
2. tests run and their real results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0060-task-003-crawler-follows-lists.md`, all of it
- `docs/adr/0060-a-list-feed-keeps-its-items.md`, sections 5 and 6
- `stophammer-crawler/AGENTS.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Run `cargo clippy --all-targets -- -D warnings` exactly, and report its
  real output. Write each test that the criteria name, with real assertions.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
