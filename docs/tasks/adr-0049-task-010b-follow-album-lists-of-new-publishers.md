# ADR 0049 Task 010b: Follow The Album List Of A Publisher Found Through An Album

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §2.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 10.
Needs: tasks 010 and 012 merged.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0049.

## Why This Task Exists

ADR 0049 §2 has two steps and one stop rule:

1. A music feed names a publisher `feedUrl`: fetch the publisher feed.
2. A publisher feed lists a `feedUrl` with `medium="music"`: fetch that feed.
3. Do not follow a link from a feed that was fetched only because of step 2.

Packets 010 and 012 stopped one level too early. They collect no link from a
feed of the second level. So a publisher feed that the crawler finds through
step 1 is fetched, but its album list is not read.

The corrective pass of 2026-09-24 showed the result. The DETOX artist feed was
indexed, and 24 of its 30 listed albums stayed `unresolved`. Only the 6 albums
whose listed URL equals the stored URL resolved. The statistics route gave
8,200 listed links and 1,780 unresolved.

## Goal

A publisher feed that the crawler reaches through step 1 has its album list
followed through step 2. A feed that the crawler reaches through step 2 gives
no link.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/follow.rs`: `follow_urls`
- `src/modes/batch.rs`: `run_waves`, `run_wave`, `report_follow_urls`,
  `wave2_follow_urls`, and the tests
- `src/modes/gossip.rs`: `spawn_follow_fetches`, `run_follow_fetch`,
  `record_follow_outcome`, `should_launch_follow_url`, and the tests

## Files Likely To Change

- `src/modes/batch.rs`
- `src/modes/gossip.rs`
- `src/follow.rs`, only when a small helper is necessary

## Do Not Touch

- `src/crawl.rs`, `src/modes/refresh.rs`, `import.rs`, `ndjson.rs`
- the rule of `follow_urls` for which items a feed gives
- `HostThrottle` and the two semaphores of task 012
- `stophammer`, `stophammer-parser`

## Constraints

The rule, as a function of the reason that a feed was fetched:

| Reason for the fetch | Links it gives |
|---|---|
| The input list, or a notification | All links of `follow_urls` (step 1 and step 2) |
| Step 1: an album named it as its publisher | The step-2 links of `follow_urls`: its `medium="music"` items |
| Step 2: a publisher listed it | None |

Batch path:

- Wave 1 is the input list. It gives all links, as today.
- Wave 2 is the links of wave 1. A wave-2 report gives links only when its
  parsed feed is a publisher feed (`raw_medium` equal to `publisher`, ASCII case
  ignored). An album that wave 1 listed is a music feed, so it gives none.
- Wave 3 is the links of wave 2, less each URL of wave 1 and wave 2, with no
  duplicates, in host interleave order. It gives no link.
- Each wave uses the same client, config, throttle, concurrency and
  failed-feeds output. Print the size of wave 3 before its first fetch, in the
  form of the wave-2 line. Print nothing when it is empty.
- The memory rule of task 010 stays. A task keeps only its links, and it drops
  its report at once.

Gossip path:

- A notification crawl gives all links, as today. These are level-1 fetches.
- A level-1 fetch whose report is `Accepted` or `NoChange`, and whose parsed
  feed is a publisher feed, gives its `medium="music"` links. These are level-2
  fetches.
- A level-2 fetch gives no link.
- A level-2 fetch uses the same follow semaphore, host throttle, dedup and
  skip checks as a level-1 fetch.
- Count level-2 fetches in the existing `follow_fetches_launched` counter.

Stop rule:

- Give the level of a fetch as an explicit value, for example an enum
  `FollowLevel { Input, Publisher, Listed }` or a depth number. Do not infer it
  from a URL.
- A fetch at the last level has no access to a function that starts a fetch,
  as in task 012.

## Implementation Steps

1. Add the level value, and the rule that gives the links for each level.
2. Add wave 3 to the batch path.
3. Add level 2 to the gossip path.
4. Correct the tests that assert the old stop at level 1. Report each one.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Batch, with a stub step:
  - An album in wave 1 names publisher P. P in wave 2 lists albums A1 and A2.
    Wave 3 holds A1 and A2.
  - A1 in wave 3 names publisher Q. No wave 4 runs, and Q is not fetched.
  - A music feed in wave 2 gives no link.
  - A URL of wave 1 or wave 2 is not in wave 3.
- Gossip, with the seams of task 012:
  - A level-1 fetch of a publisher feed gives its `medium="music"` links.
  - A level-1 fetch of a music feed gives no link.
  - A level-2 fetch gives no link.
- The unit tests of `follow_urls` pass with no edit.

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
6. each existing test that changed, with the old and the new expectation

## Escalation Triggers

Stop and report when:

- the level cannot pass through the gossip path without a change to the
  notification crawl
- `run_urls` needs a signature change

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0049-task-010b-follow-album-lists-of-new-publishers.md` in
  the `stophammer` repository, in full
- `src/follow.rs`, `src/modes/batch.rs`, and the follow functions of
  `src/modes/gossip.rs`

Goal:
- A publisher feed that the crawler reaches through an album has its album list
  followed. A feed reached through a publisher's list gives no link.

Constraints:
- Use the table in the task file. The input list and a notification give all
  links. A publisher reached through step 1 gives its `medium="music"` links.
  A feed reached through step 2 gives none.
- Batch: add wave 3. A wave-2 report gives links only when its feed is a
  publisher feed. Wave 3 gives none, and it excludes the URLs of waves 1 and 2.
- Gossip: add level 2 with the same semaphore, throttle and checks. A level-2
  fetch gives no link.
- The level is an explicit value, never inferred from a URL.
- Keep the memory rule of task 010.

Do not touch:
- `src/crawl.rs`, `refresh.rs`, `import.rs`, `ndjson.rs`
- the item rule of `follow_urls`, `HostThrottle`, the two semaphores
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Batch stub tests: album → P → A1 and A2 in wave 3; no wave 4; a music feed
  in wave 2 gives no link; wave 3 excludes waves 1 and 2.
- Gossip tests: a level-1 publisher gives its music links; a level-1 music feed
  gives none; a level-2 fetch gives none.
- The `follow_urls` tests pass unedited.

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
