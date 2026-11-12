# ADR 0097: A Feed Read Gives The End Of Its Return Wait

## Status
Accepted on 2026-11-12 by the operator. It amends ADR 0096 section 1, the defaults.
It answers three points of a Studio request through the orchestrator.

## Date
2026-11-12

## Context
On 2026-11-12, Studio moved test album `7ea30e6c-c7ba-5b81-a7be-6465c1591b7a` at
3:50 PM. That move was the first return, and it had no wait. The move back at 4:05 PM
was the second return in 30 days. ADR 0083 section 5 gave it a wait of 1 hour.

The node read the feed at 4:06:17 PM and kept the earlier URL. At 4:51 PM, after the
wait, the record continued to have the earlier URL, because no new podping came. Studio records
the test in its commit `b697538`, file
`docs/tasks/studio-v1-task-091-own-address-tag.md`.

Studio asked three points:

1. A shorter wait for a user who tests moves.
2. A new read of the feed when a wait ends.
3. A field in `GET /v1/feeds/{guid}` that gives the end of the wait, so that Studio
   can tell the user the accurate time.

ADR 0096 lets server variables set the wait. Its defaults are the ADR 0083 values.

## Decision

### 1. The defaults are shorter

The operator selected these defaults for the three variables of ADR 0096:

| Variable | Default |
|---|---|
| `MOVE_RETURN_WAIT_SECS` | `60` |
| `MOVE_RETURN_WAIT_LIMIT_SECS` | `3600` |
| `MOVE_RETURN_COUNT_RESET_SECS` | `86400` |

The first return stays free. The second return waits 1 minute, and each subsequent
return doubles the wait, to a limit of 1 hour. After 1 day with no return, the count
starts again. The operator can continue to set each variable, as ADR 0096 gives.

### 2. The node does not read a feed again when its wait ends

A podping makes the crawler read the feed again. Studio sends a podping for the feed after the time of
section 3. A publisher with no Studio sends a podping, or the next pass reads the feed.

### 3. `GET /v1/feeds/{guid}` gives `return_wait_until`

The field is an integer in Unix seconds, or `null`. It has the same form as each other
time field of the API.

- It gives the end of the active return wait of the record. Before this time, a return to a
  URL that the record left is refused. A move to a URL that is not a left URL never waits.
- It is `null` when no wait is active. The record then has no return, its count started
  again, or the wait ended.
- Only the single-feed read gives the field. A list route gives no `return_wait_until` key.
- Only the primary stores the move state (ADR 0083 section 4). A community node always gives
  `null`. A client reads the field from the primary.

The node computes the value from the stored count, the time of the last return and the values
that it uses. A server variable that changes at a restart changes the value.

## Consequences

- With the defaults, a publisher who controls the two URLs can make a return approximately
  each hour. Each return signs one `FeedUpserted` event and can change the payment routes of
  the record. The tag rules of ADR 0083 section 2 continue to stop a loop.
- The ADR 0083 statement "a wait can keep a record at a URL with incorrect content for up to
  7 days" applies to the configured limit, 1 hour by default.
- The ADR 0083 statement "nothing crawls a URL again when its wait ends" stays true. Studio
  uses `return_wait_until` to send its podping at the correct time.
- A client that reads a community node sees `null` during a wait.

## Alternatives Considered

### Keep the ADR 0083 defaults and set the variables in `.env`
The effect on the primary is the same. The operator selected shorter defaults.

### The node reads the feed again when a wait ends
The primary would keep a queue of the ends of waits and ask the crawler to read the URL. That
needs a new crawl path between the primary and the crawler. A podping from the publisher
gives the same read, so the operator selected no mechanism.

### An RFC 3339 string
It is easier for a person to read, but each other time field of the API is in Unix seconds.

## Guards

Each criterion is mechanical. This ADR has no visual criterion.

| Criterion | Test |
|---|---|
| With no variable, the node uses the defaults of section 1 | `return_cooldown_uses_the_operator_values` in `src/db.rs` |
| The single-feed read gives the end of an active wait, and `null` after the wait | `a_second_return_waits_and_the_feed_read_gives_the_end` in `tests/adr0083_move_refusal_tests.rs` |

Section 2 adds no mechanism, so it has no test.
