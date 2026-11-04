# ADR 0083 Task 005: The Release, The Deploy And The Record

Owner: [ADR 0065](../adr/0065-a-release-promotes-its-candidate.md) and
[ADR 0066](../adr/0066-a-version-number-tells-what-to-upgrade.md), for
[ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md).

Repositories: `stophammer`, `stophammer-crawler`, `stophammer-parser`, and the
VPS. The agent sets the version and writes the handoff. The operator commits,
pushes, tags and deploys. This task starts after the commit of task 004.

## Goal

A release holds ADR 0083 and runs in production. `AGENTS.md` describes the present,
and tasks 001 to 005 are deleted.

## Context

The check of ADR 0066 §4 gives the version. This change adds no route, field,
parameter, setting or event type. It has one migration and a new warning text. So
item 3 of that check applies, and the change raises PATCH.

The tag `v0.18.0` is at commit `a34be18`, so release 0.18.0 does not hold ADR 0083.
The next version after 0.18.0 holds it.

ADR 0083 §4 requires no refill pass. A declaration column gets its value at the
next update or move. The left URLs start empty. No feed needs a new crawl.

## Files To Inspect

- `Cargo.toml` and `Cargo.lock` of each repository
- `AGENTS.md`, "Where The Work Stands"
- The latest handoff and notes in `.idea/releases/`, for the form
- `deploy.sh`, the `--release` and `--backup` options

## Files Likely To Change

- `Cargo.toml` and `Cargo.lock` in the three repositories
- `AGENTS.md`
- `docs/adr/0083-a-feed-move-refuses-a-loop.md`: the "Status" section only
- New: `.idea/releases/<version>/deploy-<version>.md` and
  `release-<version>-notes.md`
- Deleted: `docs/tasks/adr-0083-task-001-*.md` to
  `docs/tasks/adr-0083-task-005-*.md`

## Constraints

- **The version.** The three repositories get one version (ADR 0066 §3).
- **The notes.** One line for each paragraph, no hard wrap. They give the reason for
  the version, the migration, the new warning and this fact: a community node needs
  no action, because no event changes.
- **The handoff.** The deploy has a migration, so `--backup` is required, and the
  rollback section names the restore. No VPS detail goes in a file of the
  repository.
- **The check after the deploy.** The start log shows no skipped migration.
  `GET /node/info` gives the commit of the candidate. A read-only query on a backup
  finds the empty table `feed_left_urls`.
- **`AGENTS.md`.** Remove the ADR 0083 text of "Open Items". The request stays there
  until the tag of a mirror has a decision. "Deployed" names the release.
- **No link to a deleted file.** The search in "Test Commands" gives no result.
- **Git.** The agent runs no git write command. It gives the commands.

## Do Not Touch

- `docs/`, other than the listed files
- Each source file, other than the version lines

## Implementation Steps

The agent:

1. Sets the version, and runs the gate in each crate.
2. Writes the notes and the handoff, and gives the commit commands.

The operator, with the handoff:

3. Pushes, and waits for a green CI run.
4. Tags the candidate, and deploys it with `--backup`.
5. Gives the start log and a backup to the agent.

The agent:

6. Runs the checks of the deploy.

The operator:

7. Promotes the release, sets the notes and deploys it.

The agent:

8. Changes `AGENTS.md` and the ADR status, deletes the five packets, and gives the
   commit command with `git rm`.

## Acceptance Criteria

Mechanical:

- `grep '^version' Cargo.toml` gives one version in each repository.
- The gate passes in each crate.
- `cargo test --test adr0045_governance_guards_tests` passes after step 8.
- `ls docs/tasks/` gives no `adr-0083-*` file after step 8.

Visual, for the operator:

- The CI run and the release run are green.

## Test Commands

```bash
for r in stophammer-parser stophammer-crawler .; do grep -H '^version' ~/build/stophammer/$r/Cargo.toml; done
cd ~/build/stophammer/stophammer-parser && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ~/build/stophammer/stophammer-crawler && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ~/build/stophammer && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ~/build/stophammer && cargo test --test adr0045_governance_guards_tests
rg 'adr-0083-task-00[1-5]' . stophammer-parser stophammer-crawler --glob '!target' --glob '!.idea'
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A commit of tasks 002 to 004 is missing.
- The gate fails in a crate.
- The start log reports a skipped migration.

## Expected Final Report

1. files changed, created and deleted
2. the result of the gate in each crate and of each deploy check
3. the commit commands
4. deviations from this task
5. unresolved concerns
