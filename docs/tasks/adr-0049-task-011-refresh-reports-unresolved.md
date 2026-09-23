# ADR 0049 Task 011: The Refresh Pass Reports The Unresolved Links

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §8.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).
Needs: task 009 deployed on the node, and task 010.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0049.

## Goal

When the `refresh` pass ends, it reads `GET /v1/publisher-links/stats` from the
node and prints the four counts. A change of the Wavlake format then shows as
an increase in `unresolved`.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/modes/refresh.rs`: `run`, `run_corrective_pass`, the origin
  derivation, the page read and its tests
- the response shape in `docs/tasks/adr-0049-task-009-link-stats-route.md` in
  the `stophammer` repository

## Files Likely To Change

- `src/modes/refresh.rs`

## Do Not Touch

- `src/modes/batch.rs`
- the corpus read and its rule that a failed page stops the pass
- the other modes
- the `stophammer` and `stophammer-parser` repositories

## Constraints

- Read the route after `batch::run_urls` returns, from the origin that the pass
  already derives from `INGEST_URL`.
- Print one line in this form:
  `publisher links: listed=N guid=N feed_url=N unresolved=N`.
- A failed read prints one warning line with the reason. It does not change
  the exit status. The corpus pass is complete at that time.
- Keep the decode of the response in its own function, so a test needs no
  network.
- Use the output style that `refresh.rs` already uses.

## Implementation Steps

1. Add the response types and the decode function, with tests.
2. Read the route after the pass, and print the line or the warning.
3. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A test decodes a sample response and gives the line
  `publisher links: listed=10 guid=4 feed_url=5 unresolved=1`.
- A test proves that a response with a missing field gives an error, not
  zeros.
- The existing refresh tests pass with no edit.

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

- the route shape on the node is different from task 009
- `run_corrective_pass` has no point after the pass where the read fits
  without a change to its callback contract

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `stophammer-crawler/AGENTS.md`
- `src/modes/refresh.rs`, in full

Goal:
- After the `refresh` pass, read `GET /v1/publisher-links/stats` from the node
  and print the four counts.

Constraints:
- The response is `{"data": {"listed_links": N, "resolved_by_guid": N,
  "resolved_by_feed_url": N, "unresolved": N}}`.
- Read after `batch::run_urls` returns, from the origin that the pass derives
  from `INGEST_URL`.
- Print `publisher links: listed=N guid=N feed_url=N unresolved=N`.
- A failed read prints one warning with the reason, and does not change the
  exit status.
- The decode is its own function. A missing field is an error, not a zero.

Do not touch:
- `src/modes/batch.rs`, the corpus read, the other modes
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- A test decodes a sample and gives
  `publisher links: listed=10 guid=4 feed_url=5 unresolved=1`.
- A test proves that a missing field gives an error.
- The existing refresh tests pass unedited.

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
