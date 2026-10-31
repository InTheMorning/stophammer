# ADR 0082 Task 004: Release Candidate 0.18.0

Owner: [ADR 0065](../adr/0065-a-release-promotes-its-candidate.md) and
[ADR 0066](../adr/0066-a-version-number-tells-what-to-upgrade.md), for
[ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md). The phase plan is
[the funding links plan](../plans/funding-links-phase-plan.md).

Repositories: `stophammer`, `stophammer-crawler`, `stophammer-parser`. The
agent sets the version and writes the handoff. The operator commits, pushes
and tags. This task starts after the commits of tasks 001 to 003.

## Goal

The three repositories give the version 0.18.0. The handoff and the release
notes exist in `.idea/releases/0.18.0/`. The operator tags `v0.18.0-rc.1`
after a green CI run.

## Context

ADR 0066 §4 item 2: a new field raises MINOR. Release 0.18.0 adds `label` to
each `source_links` row and to the payload of `SourceEntityLinksReplaced`, and
the migration 0058. Nothing is removed or renamed. The three repositories get
one version (ADR 0066 §3). The crawler changes only its version and the version
of its parser dependency in `Cargo.lock`.

Release 0.17.0 of 2026-10-31 is deployed. Its handoff
`.idea/releases/0.17.0/deploy-0.17.0.md` and its notes
`release-0.17.0-notes.md` give the form. Git ignores `.idea/`.

## Files To Inspect

- `Cargo.toml` and `Cargo.lock` of each repository
- `AGENTS.md`, "Where The Work Stands"
- `.idea/releases/0.17.0/deploy-0.17.0.md` and `release-0.17.0-notes.md`
- `deploy.sh`, the `--release` and `--backup` options
- `docs/operations.md`, "Restore"

## Files Likely To Change

- `Cargo.toml` and `Cargo.lock` in the three repositories
- `AGENTS.md`, the first paragraph of "Where The Work Stands"
- New: `.idea/releases/0.18.0/deploy-0.18.0.md`,
  `.idea/releases/0.18.0/release-0.18.0-notes.md`

## Constraints

- **The version.** Each `Cargo.toml` gives `0.18.0`. `cargo build` in each
  crate updates its `Cargo.lock`. The lock of the crawler gives the parser at
  0.18.0.
- **`AGENTS.md`.** The first paragraph says that release 0.18.0 is open and
  that it holds ADR 0082.
- **The notes.** One line for each paragraph, no hard wrap. The notes give the
  MINOR reason, the new field, the migration, the source `podcast_funding`,
  and this rule for a community operator: upgrade before the refill pass,
  because a node of 0.17.0 applies the payload and drops the label.
- **The handoff.** The blocks follow the form of 0.17.0: push, CI, tag, deploy
  with `--backup`, the checks, the refill pass of task 005, promote, notes,
  deploy the release, record. The deploy has a migration, so `--backup` is
  required and the rollback section names the restore. No VPS detail goes in a
  file of the repository.
- **Git.** The agent runs no git write command. The handoff gives the push
  and tag commands.

## Do Not Touch

- `docs/`, other than a read
- Each source file

## Implementation Steps

1. Set the version in the three `Cargo.toml` files, and run `cargo build` in
   each crate.
2. Change the first paragraph of `AGENTS.md`.
3. Write the notes and the handoff.
4. Run the gate in each crate, and the STE checker on the two new files.

## Acceptance Criteria

Mechanical:

- `grep '^version' Cargo.toml` in each repository gives `0.18.0`.
- The gate passes in each crate.

Manual, for the operator, after the tag:

- The CI run of `stophammer` main is green.
- The run `Release v0.18.0-rc.1` is green, and the release page gives its
  assets.
- `deploy.sh --release 0.18.0-rc.1 --backup` finds the two GHCR images.

## Test Commands

```bash
for r in stophammer-parser stophammer-crawler .; do grep -H '^version' ~/build/stophammer/$r/Cargo.toml; done
cd ~/build/stophammer/stophammer-parser && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ~/build/stophammer/stophammer-crawler && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ~/build/stophammer && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics .idea/releases/0.18.0/deploy-0.18.0.md .idea/releases/0.18.0/release-0.18.0-notes.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A commit of tasks 001 to 003 is missing in a repository.
- The gate fails in a crate.

## Expected Final Report

1. files changed and created
2. the result of the gate in each crate
3. the commit commands for the three repositories
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/tasks/adr-0082-task-004-release-candidate.md` (this task)
- `~/build/stophammer/docs/adr/0066-a-version-number-tells-what-to-upgrade.md`
- `~/build/stophammer/docs/adr/0082-a-funding-link-is-a-source-link.md`, "Consequences"
- `~/build/stophammer/.idea/releases/0.17.0/deploy-0.17.0.md` and `release-0.17.0-notes.md`, for the form

Goal:
- The three repositories give 0.18.0, `AGENTS.md` says the release is open, and the handoff and the notes exist.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Put no VPS host, user or directory in a file of a repository. The handoff in `.idea/` can hold them.

Do not touch:
- `docs/`, each source file
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed and created
2. the result of the gate in each crate
3. deviations from this task
4. unresolved concerns
