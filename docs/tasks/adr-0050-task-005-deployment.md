# ADR 0050 Task 005: Deployment And Operations

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) §1 and §6.
Plan: [phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md),
decisions 7 and 10.
Needs: task 003 merged in `stophammer-crawler`.

This task changes the `stophammer` repository only. It holds the compose file
and the operations guide.

## Goal

The deployed crawler containers use `/data/feed_cache.db`, the operations
guide describes the file, and `AGENTS.md` records the state.

## Files To Inspect

- `docker-compose.yml`: the `gossip`, `import`, `import-wavlake` and
  `stophammer-crawler` services, and how they pass `--skip-db`
- `packaging/env/crawler-feed.compose.env.example`,
  `crawler-gossip.compose.env.example`, `crawler-import.compose.env.example`,
  `crawler-import-wavlake.compose.env.example`
- `docs/operations.md`: "What to Back Up", "Disk Sizing Guidance"
- `AGENTS.md`: "Where The Work Stands", and "Crawler Subcommands"
- `docs/plans/adr-0050-feed-revalidation-phase-plan.md`, decision 10

## Files Likely To Change

- `docker-compose.yml`
- the four `.compose.env.example` files
- `docs/operations.md`
- `AGENTS.md`

## Do Not Touch

- the `.compose.env` files without `.example`. They hold secrets and are not
  tracked
- `src/`, `tests/`, `migrations/`
- `deploy.sh`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- `gossip`, `import` and `import-wavlake` pass
  `--feed-cache "$${FEED_CACHE_DB:-/data/feed_cache.db}"` in their command,
  in the same style as `--skip-db`.
- The `stophammer-crawler` tools service runs the binary directly, so it needs
  only `FEED_CACHE_DB=/data/feed_cache.db` in `crawler-feed.compose.env.example`.
  Add the same line to the other three example files.
- `docs/operations.md`:
  - "What to Back Up": one row for `FEED_CACHE_DB`. Loss means that the next
    pass fetches each body again. No restore is needed.
  - "Disk Sizing Guidance": about 30 to 50 MB for 10,000 feeds, with the
    2026-04-03 snapshot as the source of that estimate.
  - One short section on the cache. It says what a `304` does in a normal
    crawl and in a `--force` pass. It names `--no-revalidate` for a host with
    a wrong `ETag`, and the `fetch:` line at the end of a pass. Then it gives
    the measurement procedure of plan decision 10: two passes after the
    deploy. The line of the second pass answers whether a `304` counts
    against the `429` limit.
- `AGENTS.md`:
  - "Crawler Subcommands": the `refresh` example gains nothing, and one sentence
    after the block says that the four fetching modes keep a fetch cache.
  - "Where The Work Stands": ADR 0050 tasks 001 to 005 are complete and not
    deployed, and the two passes remain.
- Write in Simplified Technical English. Run the shared checker on the changed
  prose.

## Implementation Steps

1. Change `docker-compose.yml` and the four example files.
2. Add the operations text.
3. Update `AGENTS.md`.
4. Run the checks below.

## Acceptance Criteria

Mechanical:

- `docker compose config` runs without an error on a machine with the four
  `.compose.env` files present, or the report says that it could not run.
- `grep -c "feed-cache" docker-compose.yml` gives 3.
- `grep -l FEED_CACHE_DB packaging/env/*.compose.env.example | wc -l` gives 4.
- `cargo build` and `cargo test` in `stophammer` are green. This task changes
  no code, so the run proves only that nothing else moved.
- On the changed prose, the shared STE checker reports no rule other than 1.6.

Visual, kept apart. If the check cannot run, report the gate as open:

- On the VPS, after `./deploy.sh crawler`, `docker compose config` shows the
  `--feed-cache` argument for the three services and `FEED_CACHE_DB` for
  `stophammer-crawler`.

## Test Commands

```bash
docker compose config > /dev/null
grep -c "feed-cache" docker-compose.yml
grep -l FEED_CACHE_DB packaging/env/*.compose.env.example | wc -l
cargo build
cargo test
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/operations.md AGENTS.md
```

## Expected Final Report

1. files changed
2. checks run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
6. the state of the visual gate

## Escalation Triggers

Stop and report when:

- a compose service for the crawler exists that this task does not name
- the flag names in the merged crawler differ from `--feed-cache` and
  `FEED_CACHE_DB`

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0050-task-005-deployment.md`, the section "Constraints"
- `docker-compose.yml`, the crawler services
- `packaging/env/*.compose.env.example`
- `docs/operations.md`, "What to Back Up" and "Disk Sizing Guidance"
- `AGENTS.md`
- `/home/citizen/.agents/skills/asd-ste100/SKILL.md`

Goal:
- The deployed crawler uses `/data/feed_cache.db`, `docs/operations.md`
  describes the cache and the measurement procedure, and `AGENTS.md` records
  the state.

Constraints:
- `gossip`, `import`, `import-wavlake`: add
  `--feed-cache "$${FEED_CACHE_DB:-/data/feed_cache.db}"` beside `--skip-db`.
- `FEED_CACHE_DB=/data/feed_cache.db` in the four `.compose.env.example` files.
- `docs/operations.md`: the backup row, the disk estimate, and one section on
  the cache and the two-pass measurement.
- `AGENTS.md`: one sentence under "Crawler Subcommands", and the state under
  "Where The Work Stands".
- Simplified Technical English, checked with the shared checker.

Do not touch:
- the `.compose.env` files without `.example`
- `src/`, `tests/`, `migrations/`, `deploy.sh`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- `docker compose config` runs, or the report says why not.
- `feed-cache` appears 3 times in `docker-compose.yml`, and `FEED_CACHE_DB` in
  4 example files.
- `cargo build` and `cargo test` are green.
- The checker reports no rule other than 1.6 on the changed prose.
- Report the VPS visual check as open when it cannot run.

Test commands:
- `docker compose config > /dev/null`
- `grep -c "feed-cache" docker-compose.yml`
- `grep -l FEED_CACHE_DB packaging/env/*.compose.env.example | wc -l`
- `cargo build`
- `cargo test`
- `python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/operations.md AGENTS.md`

At the end, report:
1. files changed
2. checks run
3. behavior changed
4. deviations from task
5. unresolved concerns
