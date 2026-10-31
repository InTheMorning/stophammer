# ADR 0081 Task 008: Release 0.17.0 Holds The `podcastindex` Mode

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md),
with [ADR 0065](../adr/0065-a-release-promotes-its-candidate.md) for the
candidate and [ADR 0066](../adr/0066-a-version-number-tells-what-to-upgrade.md)
for the version. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repositories: `stophammer`, `stophammer-crawler` and `stophammer-parser`. The
agent changes the files and writes the handoff. The operator commits, pushes
and tags. This task starts after the commit that accepts ADR 0081.

## Goal

The three repositories give the version 0.17.0, and the candidate
`v0.17.0-rc.1` builds on commits that passed CI. The release page of the
candidate holds each asset of ADR 0063 §3, and GHCR holds the image
`stophammer-crawler:0.17.0-rc.1`.

## Context

The `podcastindex` mode adds a subcommand, settings and a Compose service. No
route, field, parameter or event changes. ADR 0066 §4 case 2 raises MINOR, so
the version after 0.16.0 is 0.17.0. The three repositories release together
with one version (ADR 0066 §3), so the node and the parser change only their
version. Release 0.16.0 of 2026-10-25 is the latest release.

## Files To Inspect

- `Cargo.toml` and `Cargo.lock` of each repository
- `AGENTS.md` of `stophammer`, "Where The Work Stands"
- ADR 0065 §4 and ADR 0066 §4
- The handoff of release 0.16.0 in `.idea/releases/0.16.0/`, when it is on the
  machine. It gives the form of the handoff. Git ignores `.idea/`.

## Files Likely To Change

- `Cargo.toml` and `Cargo.lock` of each of the three repositories
- `AGENTS.md` of `stophammer`: the first paragraph of "Where The Work Stands"
- `.idea/releases/0.17.0/deploy-0.17.0.md` and
  `.idea/releases/0.17.0/release-0.17.0-notes.md`, outside Git

## Constraints

- **The version check.** Before the version changes, the agent lists each
  commit since `v0.16.0` in each repository with
  `git log v0.16.0..main --oneline`. It names each change that ADR 0066 §4
  covers, and confirms that no route, field, parameter or event is removed or
  renamed.
- **The version.** Each `Cargo.toml` gives `0.17.0`. `cargo build` in each
  crate refreshes its `Cargo.lock`. The commit holds both files.
- **The commit messages.** `stophammer`:
  `Set version 0.17.0 for the next release candidate (ADR 0066)`. The crawler
  and the parser: the same text with `(stophammer ADR 0066)`.
- **`AGENTS.md`.** The first paragraph says that release 0.17.0 is open, that
  it holds ADR 0081, and that each `Cargo.toml` gives its version.
- **The handoff.** The deploy file gives the blocks in the sequence of ADR
  0065 §4. The push of `main` of the three repositories comes first, then the
  CI run of `stophammer`. The candidate tag on the three repositories follows,
  then the release workflow. Each VPS value is in the handoff and in no file
  of the repository. The agent gives a browser URL in a code block, with the click
  steps.
- **The notes.** The release notes are in Simplified Technical English, with
  one line for each paragraph. The section "For operators" gives the key, the
  env file, the dry run, the schedule and the §10 measurement. The section
  "For clients" says that no route or field changes.
- **Git.** The agent runs no git write command. The operator commits, pushes
  and tags.

## Do Not Touch

- The source of the three crates, except `Cargo.toml` and `Cargo.lock`
- `docs/adr/`, `docs/tasks/`, `packaging/`, `docker-compose.yml`
- `.github/workflows/`

## Implementation Steps

The agent:

1. Lists the commits since `v0.16.0` in each repository, and reports the
   version decision with its reason.
2. Sets the version in each `Cargo.toml`, runs `cargo build` in each crate,
   and runs the gate of each crate.
3. Changes the first paragraph of "Where The Work Stands" in `AGENTS.md`.
4. Writes the handoff and the notes, and gives the three commit commands.

The operator:

5. Commits in each repository and pushes `main` of the three repositories.
6. Waits until the CI run of `main` of `stophammer` is green.
7. Tags `v0.17.0-rc.1` on the three repositories and pushes the tags.
8. Waits until the run `Release v0.17.0-rc.1` is green.

## Acceptance Criteria

Mechanical:

- `grep '^version' Cargo.toml` gives `0.17.0` in each of the three
  repositories.
- The gate is green in each crate: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`.

Manual, because a test cannot give a tag to GitHub:

- The CI run of `main` of `stophammer` on the version commit is green.
- The run `Release v0.17.0-rc.1` is green.
- The release page of `v0.17.0-rc.1` holds the three tarballs, the Arch
  packages and the checksum files, and GHCR holds
  `stophammer-crawler:0.17.0-rc.1` and `stophammer-indexer:0.17.0-rc.1`.

## Test Commands

```bash
for r in . stophammer-crawler stophammer-parser; do
  (cd ~/build/stophammer/$r && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check)
done
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A commit since `v0.16.0` removes or renames a route, a field, a parameter or
  an event. The version then needs a decision under ADR 0066 §2.
- The release workflow stops under ADR 0065 §3 or §4. The correction is a new
  candidate `v0.17.0-rc.2` on the correct commits, never a moved tag.

## Expected Final Report

1. the commits since `v0.16.0` and the version decision
2. files changed
3. the gate result of each crate
4. the path of the handoff and the notes
5. the three commit commands and the tag command
6. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md`, "Where The Work Stands"
- `~/build/stophammer/docs/adr/0065-a-release-promotes-its-candidate.md`, §4
- `~/build/stophammer/docs/adr/0066-a-version-number-tells-what-to-upgrade.md`, §3 and §4
- `~/build/stophammer/docs/tasks/adr-0081-task-008-release-candidate.md` (this task)
- `~/build/stophammer/.idea/releases/0.16.0/deploy-0.16.0.md`, when present

Goal:
- Each of the three repositories gives the version 0.17.0, `AGENTS.md` says that release 0.17.0 is open, and the handoff and the notes exist in `.idea/releases/0.17.0/`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write the handoff and the notes in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Put no VPS host, user or directory in a file of the repository.
- Stop at each escalation trigger of this task.

Do not touch:
- the source of the three crates, except `Cargo.toml` and `Cargo.lock`
- `docs/adr/`, `docs/tasks/`, `packaging/`, `docker-compose.yml`, `.github/workflows/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task that is mechanical passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. the commits since `v0.16.0` and the version decision
2. files changed
3. the gate result of each crate
4. the path of the handoff and the notes
5. the three commit commands and the tag command
6. unresolved concerns
