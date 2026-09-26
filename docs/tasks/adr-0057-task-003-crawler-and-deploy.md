# ADR 0057 Task 003: The Crawler And The Deploy

Owner: [ADR 0057](../adr/0057-a-feed-can-block-this-index.md) §3 and §5.
Plan: [ADR 0057 phase plan](../plans/adr-0057-podcast-block-phase-plan.md).

Repository: `stophammer-crawler`, then the operator deploy.

## Goal

The crawler sends the block list of the parser, treats `source_blocked` as a
final answer, and the node and the crawler are deployed.

## Files To Inspect

- `stophammer-crawler/src/crawl.rs`: the submission, `UNCACHED_NODE_REASONS`,
  and the handling of a rejection
- `stophammer-crawler/src/modes/batch.rs`: the follow of a rejected feed
- `stophammer-crawler/AGENTS.md`

## Constraints

- The crawler submits the parser type unchanged, so the field reaches the
  node with no code change. Add one test: a body with an unbounded `yes`
  gives a submission JSON with the `blocks` key.
- `source_blocked` is not in `UNCACHED_NODE_REASONS`. The crawler keeps it as
  the node's answer. When the publisher removes the tag, the body changes,
  the fetch gives `200`, and the crawler submits again. Add one test that
  shows that a `source_blocked` answer is kept as the node's answer.
- The crawler follows no link of a feed with the answer `source_blocked`.
  Add one test.
- Record the rule in `stophammer-crawler/AGENTS.md`, with ADR 0057 as its
  owner. Write in ASD-STE100 Simplified Technical English, and run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics stophammer-crawler/AGENTS.md`.

## The Deploy

The operator runs these steps after the commits of tasks 001 to 003:

1. `./deploy.sh indexer`
2. `./deploy.sh crawler`
3. `curl -s https://api.musicindex.org/openapi.json | grep -c source_blocked`
   gives 1 or more.
4. The operator check of the phase plan, with a test feed on a host that the
   operator controls.

## Acceptance Criteria

Mechanical:

- The three tests above pass.
- The crawler gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

Visual, and open until the operator does it:

- Step 4 of the deploy.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only the crawler part of this task. The operator does the deploy.

Read:
- `docs/tasks/adr-0057-task-003-crawler-and-deploy.md`, all of it
- `docs/adr/0057-a-feed-can-block-this-index.md`
- `stophammer-crawler/AGENTS.md`

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit. Do not deploy.
- Write each test with real assertions. Paste the real output of each gate
  command.

At the end, report:
1. files changed
2. the real output of each gate command
3. behavior changed
4. deviations from task
5. unresolved concerns
