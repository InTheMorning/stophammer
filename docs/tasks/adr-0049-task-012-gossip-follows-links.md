# ADR 0049 Task 012: The Gossip Path Follows Publisher Links

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §2.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 10.
Needs: task 010.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0049.

## Goal

When the `gossip` mode crawls a feed because of a notification, and the node
accepts it, the mode also crawls the feeds that it names through a publisher
link. The walk stops at one level, and each follow fetch waits for the host
throttle.

## Why The Throttle Is New Here

The `gossip` mode has no host throttle today. It limits only the number of
fetches at one time, with a `Semaphore`. A podping for one Wavlake artist feed
can list 131 albums on one host. ADR 0049 §2 requires host pacing for each
follow fetch.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/modes/gossip.rs`: `process_notification_urls` (near line 398), the
  spawned crawl task, the skip checks (`skip_db`, `should_skip_feed`), `run`
- `src/modes/batch.rs`: `HostThrottle` (near line 74), and how
  `crawl_feed_with_retries` waits for it
- `src/follow.rs` from task 010
- `src/main.rs`: `Mode::Gossip` and how `Mode::Feed` declares `host_delay_ms`

## Files Likely To Change

- `src/modes/gossip.rs`
- `src/modes/batch.rs`: the visibility of `HostThrottle` only
- `src/main.rs`: the `host_delay_ms` flag of the gossip mode

## Do Not Touch

- the behavior of `HostThrottle`
- the notification crawl. It keeps its present behavior and has no throttle
- `src/follow.rs`
- `src/crawl.rs`, `import.rs`, `ndjson.rs`, `refresh.rs`
- the `stophammer` and `stophammer-parser` repositories

## Constraints

- Make `HostThrottle` visible to the crate with `pub(crate)`. Make no other
  change to it.
- Add `--host-delay-ms` to the gossip mode, with `env = "HOST_DELAY_MS"` and
  default `1500`, as the `feed` mode does.
- `run` makes one `HostThrottle` and shares it with each follow fetch.
- After a notification crawl whose outcome is `Accepted` or `NoChange` and that
  has a parsed feed, call `follow::follow_urls`.
- For each follow URL, apply the same dedup and skip checks that a notification
  URL gets. Then spawn a follow fetch.
- A follow fetch takes a permit of its own follow `Semaphore`, not of the
  notification `Semaphore`. Then it waits for the host throttle, and
  calls `crawl_feed_report`. It records its outcome in `skip_db` and in the
  progress store as a notification crawl does.
- A follow fetch does not call `follow::follow_urls`.
- Count follow fetches in a new counter of `GossipCounters`, and print it with
  the other counters.

## Implementation Steps

1. Make `HostThrottle` crate-visible.
2. Add the flag and pass it into `run`.
3. Make the shared throttle in `run`.
4. Collect the follow URLs after a notification crawl, and spawn the follow
   fetches with the constraints above.
5. Add the counter.
6. Add the tests below.
7. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A test proves that a follow fetch does not collect follow URLs.
- A test proves that a follow URL that the dedup or skip check refuses is not
  fetched.
- A test proves that the gossip mode parses `--host-delay-ms` with the default
  1500.
- The existing gossip tests pass with no edit.

## Test Commands

```bash
cd stophammer-crawler
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- the spawned task cannot give the parsed feed back without a change to the
  notification crawl
- the dedup store refuses a follow URL for a reason that is not a repeat
- an existing gossip test fails

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `stophammer-crawler/AGENTS.md`
- `src/modes/gossip.rs`: `process_notification_urls`, the spawned crawl task,
  `run`, `GossipCounters`
- `src/modes/batch.rs`: `HostThrottle`
- `src/follow.rs`
- `src/main.rs`: `Mode::Gossip`, and `host_delay_ms` in `Mode::Feed`

Goal:
- After an accepted notification crawl, the gossip mode also crawls the feeds
  that the feed names through a publisher link. Each follow fetch waits for a
  host throttle. The walk stops at one level.

Constraints:
- `HostThrottle` becomes `pub(crate)`, with no other change.
- Add `--host-delay-ms`, `env = "HOST_DELAY_MS"`, default 1500, to the gossip
  mode. `run` makes one shared `HostThrottle`.
- Follow from `Accepted` or `NoChange` reports that have a parsed feed.
- Each follow URL gets the same dedup and skip checks as a notification URL.
- A follow fetch takes a permit of its own follow `Semaphore`, waits for the
  throttle, calls
  `crawl_feed_report`, records its outcome in `skip_db` and the progress store,
  and never follows.
- Count follow fetches in a new `GossipCounters` field and print it.
- The notification crawl keeps its present behavior and has no throttle.

Do not touch:
- the behavior of `HostThrottle`, `src/follow.rs`
- `src/crawl.rs`, `import.rs`, `ndjson.rs`, `refresh.rs`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - A follow fetch does not follow.
  - A refused follow URL is not fetched.
  - `--host-delay-ms` defaults to 1500.
- The existing gossip tests pass unedited.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
