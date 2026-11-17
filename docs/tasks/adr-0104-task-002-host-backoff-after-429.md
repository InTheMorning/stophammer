# ADR 0104 Task 002: A Host That Answers 429 Gets A Longer Delay

Owner: [ADR 0104](../adr/0104-a-moved-feed-is-found-without-a-podping.md) §6.

Repository: `stophammer-crawler`, in the folder `stophammer-crawler/` of the
`stophammer` checkout. It has its own Git repository. Task 001 changed
`src/modes/podcastindex.rs`. Do not change that file.

## Goal

- `HostThrottle` keeps a backoff for each host. A fetch with the HTTP status
  `429` increases it, and fetches with no `429` decrease it.
- `Retry-After` of a feed host is read as seconds or as an HTTP date.
- `gossip` and the batch pipeline use the same backoff.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Host throttle | extend | `stophammer-crawler/src/modes/batch.rs` | `HostThrottle` already holds one slot for each host, with `next_allowed_at` |
| Retry-After parser | extend | `stophammer-crawler/src/crawl.rs` | `parse_retry_after_secs` reads the seconds form only |
| Fetch status | reuse | `stophammer-crawler/src/crawl.rs` | `CrawlReport::fetch_http_status` gives the status of the feed fetch |
| Date parser | reuse | `stophammer-crawler/Cargo.toml` | `chrono` gives `DateTime::parse_from_rfc2822` |

## Files To Inspect

- `stophammer-crawler/src/modes/batch.rs`: `HostThrottle`, `HostSlot`,
  `HostLease`, `crawl_feed_with_retries` and the tests
- `stophammer-crawler/src/modes/gossip.rs`: the two calls of
  `host_throttle.release` at about lines 825 and 888
- `stophammer-crawler/src/crawl.rs`: `CrawlOutcome::retry_delay`,
  `parse_retry_after_secs`, the `FetchError` variant and the place that
  fills `retry_after_secs` for a fetch at about line 1379
- `docs/adr/0104-a-moved-feed-is-found-without-a-podping.md` §6

## Files Likely To Change

- `stophammer-crawler/src/modes/batch.rs`
- `stophammer-crawler/src/modes/gossip.rs`
- `stophammer-crawler/src/crawl.rs`

## Do Not Touch

- `src/modes/podcastindex.rs`.
- The `429` rules for the node: `node_rate_limit_wait`, `wait_after_429` in
  `refresh.rs` and `is_retryable_ingest_status`.
- `CRAWL_ATTEMPTS`, the default of `HOST_DELAY_MS` and the concurrency.
- Each file outside `stophammer-crawler/`.

## Constraints

- Add named constants with a doc comment that names `stophammer` ADR 0104
  §6: `MAX_HOST_BACKOFF` = 60 s, `MAX_RETRY_AFTER` = 600 s and
  `BACKOFF_DECAY_FETCHES` = 20.
- `HostSlot` gets a backoff `Duration` and a count of fetches with no `429`.
- Change `HostThrottle::release` to take the lease and the `CrawlReport` of
  the fetch. Remove the `delay` argument. The rule of `release`:
  - When `report.fetch_http_status` is `Some(429)`: backoff =
    `max(2 * backoff, 2 * host_delay)`, at most `MAX_HOST_BACKOFF`. Set the
    count to 0. Write one line to standard error:
    `crawl: host <host> answered 429, delay now <ms> ms`.
  - Else: add 1 to the count. When the count gets to
    `BACKOFF_DECAY_FETCHES`, halve the backoff and set the count to 0. A
    backoff of `host_delay` or less becomes zero.
  - `next_allowed_at` = now + the longest of `host_delay`, the backoff and
    the `Retry-After` of a fetch `429`. `Retry-After` is at most
    `MAX_RETRY_AFTER`.
- Only the `Retry-After` of the feed fetch counts. The `Retry-After` of an
  `IngestError` comes from the node, and it must not change the delay of the
  feed host.
- A host with a zero `host_delay` (the tests use it) also gets a backoff after
  a `429`. Use 1 s as the first backoff when `2 * host_delay` is zero.
- `crawl_feed_with_retries` keeps its retries. It calls the new `release`.
- `gossip` calls the new `release` with its report. If a call site has no
  report, make the function return the report or the status to the caller.
  Do not change what `gossip` does with the report.
- `parse_retry_after_secs` reads an integer, or an HTTP date with
  `chrono::DateTime::parse_from_rfc2822`. For a date, the result is the
  seconds from now to the date, and 0 for a date in the past.
- Each new or changed comment names `stophammer` ADR 0104 §6.
- Read the `rust-dev` skill rules that `stophammer-crawler/AGENTS.md` names.
- Write each comment in Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a unit test. Use `tokio::time::pause` or read the slot
values directly. Do not make a test wait in real time.

1. A `429` report gives a backoff of two times the host delay. A second `429`
   doubles it. The backoff stops at 60 s.
2. A `429` on host A does not change the backoff of host B.
3. After a `429` with `Retry-After: 30` and a host delay of 1.5 s, the next
   allowed time is 30 s or more after the release.
4. A `Retry-After` of 3600 s gives a wait of 600 s.
5. 20 releases with no `429` halve the backoff. 19 releases do not change it.
6. A backoff that falls to the host delay or less becomes zero.
7. An `IngestError` with a `retry_after_secs` of 120 and a feed status of
   `200` does not give a wait longer than the host delay.
8. `parse_retry_after_secs` gives 5 for `"5"`, a value near 60 for an HTTP
   date 60 s after now, 0 for a date in the past, and `None` for text.
9. The existing tests of `batch.rs` and `gossip.rs` pass.

## Checks

Run each command in `stophammer-crawler/`:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

Report the result of each command. Report each file that you changed.

## Review Corrections

- The review of 2026-11-17 found that a retried fetch lost its wait.
  `release` now also takes `retry_wait`. A retried `FetchError` gives
  `retry_delay(attempt)`, and each other report gives `Duration::ZERO`. The
  test `a_retry_wait_of_a_failed_fetch_holds_the_host` proves it.
- The review also found two limits: the `gossip` fetch after a podping, and
  the host after a redirect. ADR 0104 §6 records them. This task does not
  change them.
