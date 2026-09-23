# ADR 0049 Task 013: The Reference Documents

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md).
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).
Needs: tasks 002 to 012 merged.

## Goal

The reference documents describe the code that tasks 001 to 012 made. No
document states a Wavlake rule that ADR 0049 superseded.

## Files To Inspect

- the merged diffs of tasks 001 to 012
- `docs/API.md`, `docs/schema-reference.md`, `docs/operations.md`,
  `docs/user-guide.md`, `docs/wiki/Data-Model-and-API.md`, `README.md`
- `docs/identity-evidence-policy.md`, `docs/importer-review-findings.md`
- `AGENTS.md`: "Where The Work Stands" and "Crawler Subcommands"
- `stophammer-crawler/AGENTS.md`
- `docs/adr/README.md`

These files mention Wavlake today. Examine each mention:
`docs/API.md`, `docs/schema-reference.md`, `docs/operations.md`,
`docs/user-guide.md`, `docs/wiki/Data-Model-and-API.md`, `README.md`,
`docs/identity-evidence-policy.md`, `docs/importer-review-findings.md`.

## Files Likely To Change

- the documents in the list above
- `AGENTS.md`
- `stophammer-crawler/AGENTS.md`, in a separate commit in that repository

## Do Not Touch

- `src/`, `tests/`, `migrations/`
- `docs/vision/`. Those plans are historical
- `docs/adr/` other than `docs/adr/README.md`
- the text of an ADR

## Constraints

- A mention of Wavlake that states a host rule is deleted or corrected. A
  mention that states a measured fact about Wavlake stays.
- `docs/API.md` describes each new field: the `publisher` view fields, `rel` in
  `remote_items`, `release_artist_source`, `publisher_feed_title`,
  `distinct_release_artist_count`, `distinct_release_artists`. It marks each
  derived value as derived, and `rel` as non-standard.
- `docs/schema-reference.md` describes the `rel` columns,
  `feed_url_observations`, `release_artist_source` and the
  `feed_url_observed` event.
- `docs/operations.md` gives:
  - the deploy sequence: each community node before the primary node sends
    `FeedUrlObserved`,
  - the first corrective pass: `refresh --force` after task 010 is deployed,
    and its time of about 2 hours 46 minutes for the Wavlake album fetches at
    the default host delay,
  - how to read the `publisher links:` line at the end of the pass.
- `AGENTS.md` "Where The Work Stands" says that ADR 0049 is complete, and
  removes item 4 from the list of work that remains. "Crawler Subcommands" says
  that `feed`, `refresh` and `gossip` follow publisher links one level.
- `stophammer-crawler/AGENTS.md` says the same about the three modes and names
  ADR 0049.
- `docs/adr/README.md`: if each rule of ADR 0035 is now enforced by a test or
  superseded, report it. Do not archive an ADR in this task.
- Write in Simplified Technical English. Run the shared checker on each changed
  document.

## Implementation Steps

1. Examine each Wavlake mention in the list, and correct or delete each host
   rule.
2. Update `docs/API.md`, `docs/schema-reference.md` and `docs/operations.md`.
3. Update `AGENTS.md`.
4. Update `stophammer-crawler/AGENTS.md` in its own commit.
5. Run the checker and the gate.

## Acceptance Criteria

Mechanical:

- `cargo build` and `cargo test` are green in `stophammer`.
- `grep -n "publisher-links/stats" docs/API.md` finds the route.
- `grep -n "feed_url_observations" docs/schema-reference.md` finds the table.
- `grep -rn "is_wavlake_url\|wavlake_artist_name_from_links" docs README.md
  AGENTS.md`, outside `docs/adr/`, `docs/plans/`, `docs/tasks/`,
  `docs/reviews/` and `docs/vision/`, finds nothing.
- On the changed prose, the shared STE checker reports no rule other than 1.6.

Visual, kept apart. If the check cannot run, report the gate as open:

- Load `/api` on a node that runs the task 009 code. Confirm that
  `/v1/publisher-links/stats` and the new fields of the feed read show their
  examples.

## Test Commands

```bash
cargo build
cargo test
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/API.md docs/schema-reference.md docs/operations.md AGENTS.md
```

## Expected Final Report

1. files changed, in each repository
2. tests run and their results
3. each Wavlake mention and what you did with it
4. deviations from this task
5. unresolved concerns
6. the state of the visual gate

## Escalation Triggers

Stop and report when:

- a document describes a behavior that the merged code does not have
- a Wavlake mention in `docs/identity-evidence-policy.md` states a rule that is
  not an ADR 0035 rule

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/adr/0049-publisher-relationships-are-rss-facts.md`
- `docs/tasks/adr-0049-task-013-reference-documents.md`, the section
  "Constraints"
- `/home/citizen/.agents/skills/asd-ste100/SKILL.md`
- the documents in the section "Files To Inspect"

Goal:
- Make the reference documents describe the ADR 0049 code, and remove each
  Wavlake host rule from them.

Constraints:
- Delete or correct each Wavlake mention that states a host rule. Keep each one
  that states a measured fact.
- `docs/API.md` describes each new field and the new route, marks derived
  values as derived, and marks `rel` as non-standard.
- `docs/schema-reference.md` describes the new columns, the new table and the
  new event.
- `docs/operations.md` gives the deploy sequence, the first `refresh --force`
  pass and its time, and how to read the `publisher links:` line.
- `AGENTS.md`: ADR 0049 is complete, item 4 goes, and "Crawler Subcommands"
  says that `feed`, `refresh` and `gossip` follow publisher links one level.
- `stophammer-crawler/AGENTS.md` says the same, in its own commit.
- Write in Simplified Technical English, and run the shared checker.

Do not touch:
- `src/`, `tests/`, `migrations/`, `docs/vision/`, and each ADR file

Acceptance criteria:
- `cargo build` and `cargo test` are green.
- `docs/API.md` has `publisher-links/stats`. `docs/schema-reference.md` has
  `feed_url_observations`.
- No reference document outside the ADR, plan, task, review and vision folders
  names `is_wavlake_url` or `wavlake_artist_name_from_links`.
- On the changed prose, the checker reports no rule other than 1.6.
- Report the `/api` visual check as open when it cannot run.

Test commands:
- `cargo build`
- `cargo test`
- `python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/API.md docs/schema-reference.md docs/operations.md AGENTS.md`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
