# ADR 0096: The Operator Sets The Return Cooldown

## Status
Accepted on 2026-11-12 by the operator. It amends ADR 0083 section 5.

## Date
2026-11-12

## Context
ADR 0083 section 5 gives a growing cooldown to a return of a feed record to a URL
that the record left. The wait starts at 1 hour, doubles to a limit of 7 days, and the
count starts again after 30 days with no return.

On 2026-11-12 the operator reported that frequent feed moves meet this cooldown. The
operator asked for a much shorter cooldown for the current work. The operator also asked
that a server variable set the values, so that a change needs no code change and no
release.

The cooldown is not the guard against a loop. The tag rules of ADR 0083 section 2 stop a
loop. The cooldown only limits a publisher who controls the two URLs and moves a record back
and forth with correct tags or redirects.

## Decision

### 1. Three server variables set the cooldown

The primary reads these variables one time, at start. Each value is a number of seconds
with no fraction, zero or more.

| Variable | Meaning | Default |
|---|---|---|
| `MOVE_RETURN_WAIT_SECS` | The wait after the first return. Each subsequent return doubles it | `3600` |
| `MOVE_RETURN_WAIT_LIMIT_SECS` | The limit of a wait | `604800` |
| `MOVE_RETURN_COUNT_RESET_SECS` | The time with no return after which the count starts again | `2592000` |

The defaults are the ADR 0083 values. The first return stays free. A value of `0` for
`MOVE_RETURN_WAIT_SECS` removes each wait.

A value that is not a number with no fraction, zero or more, keeps its default, and the node writes
a warning to its log. At start the node writes the values that it uses to its log.

### 2. The other rules of ADR 0083 stay

A return must continue to have a tag at each end and a tag that is not the arrival tag. Section 6 of
ADR 0083, the permanent redirect to a left URL, uses the values of this ADR.

## Consequences

- The operator changes the cooldown with a change of `.env` and a restart. No release is
  necessary.
- A short cooldown lets a publisher who controls the two URLs move a record back and forth
  more frequently. Each return signs one `FeedUpserted` event and can change the payment
  routes of the record. The tag rules of ADR 0083 continue to stop a loop.
- A community node does not read the values, because only the primary decides a move.
- The ADR 0083 statement "a wait can keep a record at a URL with incorrect content for
  up to 7 days" applies to the configured limit.

## Guards

Each criterion is mechanical. This ADR has no visual criterion.

| Criterion | Test |
|---|---|
| Operator values replace the defaults, and a first wait of zero gives no wait | `return_cooldown_uses_the_operator_values` in `src/db.rs` |
| With no variable, the node uses the ADR 0083 values | The same test, and the ADR 0083 guards in `tests/adr0083_move_refusal_tests.rs` |

The parse of the variables in `src/main.rs` has no test. A manual check replaces it: after
a restart, the start log gives the values that the node uses.
