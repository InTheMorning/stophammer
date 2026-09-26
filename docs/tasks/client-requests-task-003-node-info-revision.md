# Client Requests Task 003: The Node Info Revision

Plan: [client requests work plan](../plans/client-requests-work-plan.md),
item 4. The fields are added fields, and they need no ADR.

Repository: `stophammer`.

## Goal

`GET /node/info` gives the git revision and the build time of the binary.

## Files To Inspect

- `docs/plans/client-requests-work-plan.md`, item 4
- `src/api.rs`: `NodeInfoResponse`, `handle_node_info`
- `src/community.rs`: the `NodeInfo` struct that reads `/node/info` from the
  primary
- `src/openapi.rs`: the `/node/info` path and its example
- `deploy.sh`: the indexer build
- `Cargo.toml`: the `build` key, if one exists
- `docs/operations.md`, `docs/API.md`: the `/node/info` route

## Files Likely To Change

- `src/api.rs`, `src/openapi.rs`, `deploy.sh`, `docs/operations.md`,
  `docs/API.md`
- `build.rs`, new
- `tests/client_requests_node_info_tests.rs`, new

## Do Not Touch

- `/v1/node/capabilities`
- `src/query.rs`. Another agent changes it at the same time
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The fields.** Add `git_revision` and `built_at` to `NodeInfoResponse`,
  as `Option<&'static str>` or `Option<String>`. Read each value at compile
  time with `option_env!("STOPHAMMER_GIT_REVISION")` and
  `option_env!("STOPHAMMER_BUILT_AT")`. An empty value gives null. Each key is
  always in the response, also when the value is null.
- The community node reads `/node/info` from the primary. The new fields must
  not break that read. An older primary that does not give the fields must
  also work.
- **`build.rs`.** It emits `cargo:rerun-if-env-changed=STOPHAMMER_GIT_REVISION`
  and `cargo:rerun-if-env-changed=STOPHAMMER_BUILT_AT`, and nothing more. It
  runs no git command, because the builds on the VPS and in Docker have no
  `.git`. Without `build.rs`, Cargo keeps an old value in a binary that it
  does not build again.
- **`deploy.sh`.** Before the indexer `cargo build`, export:
  - `STOPHAMMER_GIT_REVISION` from `git rev-parse --short HEAD`, with the
    suffix `-dirty` when `git status --porcelain` gives output
  - `STOPHAMMER_BUILT_AT` from `date -u +%Y-%m-%dT%H:%M:%SZ`

  These are read-only git commands. Do not run `deploy.sh`. Check the script
  with `bash -n deploy.sh`.
- Add the fields to the `/node/info` example in `src/openapi.rs` and to
  `docs/API.md`. In `docs/operations.md`, state that a build outside
  `deploy.sh` sets no value, and that the two fields are then null. Write
  in ASD-STE100 Simplified Technical English. Run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics`
  on each changed document. Correct each finding in your own lines.

## Implementation Steps

1. Add `build.rs`.
2. Add the fields and fill them in `handle_node_info`.
3. Update `deploy.sh`.
4. Update the example, `docs/API.md` and `docs/operations.md`.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/client_requests_node_info_tests.rs`:

- `GET /node/info` gives the keys `node_pubkey`, `git_revision` and
  `built_at`. The failure message names v4vmm request 2.
- A build with `STOPHAMMER_GIT_REVISION=abc1234` gives that value. Show this
  with `STOPHAMMER_GIT_REVISION=abc1234 cargo test --test client_requests_node_info_tests`
  and report the output. A test can not set a compile-time value, so this
  criterion is a manual check.
- `bash -n deploy.sh` passes.
- The gate is green.

Visual, and open until the next deploy:

- The operator reads `https://api.musicindex.org/node/info`. `git_revision`
  is equal to the deployed commit.

## Test Commands

```bash
cargo build
cargo test --test client_requests_node_info_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
bash -n deploy.sh
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- The community read of `/node/info` rejects a response with unknown fields.
- `deploy.sh` builds the node binary in more than one place.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/client-requests-task-003-node-info-revision.md`, all of it
- `docs/plans/client-requests-work-plan.md`, item 4
- The files in "Files To Inspect"

Goal:
- `GET /node/info` gives `git_revision` and `built_at`.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit. Do not run
  `deploy.sh`.
- Another agent changes `src/query.rs` and the search part of
  `src/openapi.rs` and `docs/API.md` in the same working tree. Change only
  the `/node/info` part of those two files. If a build fails in a file that
  you did not change, wait one minute and run it again.

Do not touch:
- `/v1/node/capabilities`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests and checks of "Acceptance Criteria".
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
