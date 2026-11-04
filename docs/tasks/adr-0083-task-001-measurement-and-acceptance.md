# ADR 0083 Task 001: The Measurement Decides, And The Operator Accepts

Owner: [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) "Open Question".

Repository: `stophammer`. The research gives the counts. The agent writes the
record and the documents. The operator decides and commits. This task starts
when the research gives the result of
`.idea/research/self-link-measurement-request.md`. Tasks 002 to 005 wait for
this task.

## Goal

A measurement record keeps the six counts. The operator answers the open
question. ADR 0083 has the status Accepted, and ADR 0052 gives the amendment in
the same commit.

## Context

Section 2 of ADR 0083 lets a first move go on when the self link at the new URL
names the stored source URL. "Open Question" gives the two branches:

- No measured body has this form, or the new URL is correct for each such body:
  section 2 stays.
- A measured body of this form shows that the old URL is correct: rule 3 applies
  to each move, and each result in the ADR table with that form changes.

## Files To Inspect

- ADR 0083, each section
- ADR 0052 sections 1 and 2, "Invariants" and "Guards"
- The capture that the research names, with its `SHA256SUMS`
- `docs/reviews/adr-0074-feeds-without-guid-count.md`, for the form of a count
  record
- `AGENTS.md`, "Open Items"

## Files Likely To Change

- New: `docs/reviews/adr-0083-self-link-measurement.md`
- `docs/adr/0083-a-feed-move-refuses-a-loop.md`
- `docs/adr/0052-a-source-moves-its-own-feed.md`
- `docs/adr/README.md`: the rows of ADR 0052 and ADR 0083
- `docs/plans/research-feed-move-confirmation-request.md`: the "Status" section
- `AGENTS.md`: "Open Items" and "Complete And Not Deployed" as true at that time

## Constraints

- **The record.** It gives the six counts for the destinations and the same
  counts for the origins. It gives the date, the capture path and the commit that
  the research examined. It says that 35 pairs give no rate for all feeds. It gives
  no feed URL and states no rule.
- **The second branch.** When it applies, rewrite rule 3 and repeat each scenario
  of the ADR table with `.idea/research/feed-move-smaller-rule-patch.py` changed
  in the same way. Then ask the operator again before acceptance.
- **ADR 0083.** The status gives the date and the operator decision. "Open
  Question" keeps the answer and the counts. Remove each word that says the ADR
  waits.
- **ADR 0052.** Add one "Amendment" section that names ADR 0083 and each amended
  item: section 1, section 2, and the first invariant. Change the first invariant
  to the text of ADR 0083.
- **One change.** Each listed document moves in one commit.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `src/`, `migrations/`, `tests/`
- `stophammer-crawler/`, `stophammer-parser/`

## Implementation Steps

1. Examine the capture hashes and the counts.
2. Write the record.
3. Give the operator the counts and the branch that they support. Wait for the
   decision.
4. Change the ADR files, the index, the request and `AGENTS.md`.
5. Run the checks, and give the commit command.

## Acceptance Criteria

Mechanical:

- `cargo test --test adr0045_governance_guards_tests` passes.
- `grep -n '^Accepted' docs/adr/0083-a-feed-move-refuses-a-loop.md` gives one
  line.
- `docs/adr/README.md` gives the status Accepted in the row of ADR 0083.

Visual, for the operator:

- The answer in "Open Question" is the decision that the operator made.

## Test Commands

```bash
cd ~/build/stophammer
sha256sum -c SHA256SUMS   # in the capture folder that the research names
cargo test --test adr0045_governance_guards_tests
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/reviews/adr-0083-self-link-measurement.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A capture hash fails.
- The six counts do not add up to 35.
- Fewer than 20 destinations gave a usable body.

## Expected Final Report

1. the six counts and the branch that they support
2. files changed and created
3. the result of the governance guard
4. the commit command
5. unresolved concerns
