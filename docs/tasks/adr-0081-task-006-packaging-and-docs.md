# ADR 0081 Task 006: The Operator Runs The Pass Each Day

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§1, §2 and §8. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repositories: `stophammer-crawler` and `stophammer`. One commit in each. The
operator commits. This task starts after task 004. It can run at the same
time as task 005.

## Goal

The operator can run the pass each day with Compose or with systemd, with the
credentials in an environment file outside the repository. The documents name
the sixth mode, its flags and its report line.

## Files To Inspect

- `stophammer/docker-compose.yml`: the `import` service (line 233) and the
  `stophammer-crawler` tool service (line 135)
- `stophammer/packaging/env/crawler-import.compose.env.example`
- `stophammer/packaging/systemd/stophammer-import.service` and
  `stophammer-import.timer`
- `stophammer/packaging/arch/PKGBUILD` (line 122) and
  `stophammer-crawler.install` (line 11)
- `stophammer/docs/operations.md`: "Shipped example one-shot units" (near
  line 280) and the `import` paragraphs that follow
- `stophammer/AGENTS.md`: "Crawler Subcommands" and the line "There are five
  modes" (line 217)
- `stophammer-crawler/README.md`: `### refresh` (line 138) and `### import`
  (line 193), for the form of a mode section
- `stophammer-crawler/AGENTS.md`: "Five modes" (line 85)
- `src/main.rs` of the crawler: the flags of `Mode::Podcastindex`

## Files Likely To Change

In `stophammer`:

- `docker-compose.yml`: a service `podcastindex`
- `packaging/env/crawler-podcastindex.compose.env.example`, new
- `packaging/systemd/stophammer-podcastindex.service` and
  `stophammer-podcastindex.timer`, new
- `packaging/arch/PKGBUILD`, `packaging/arch/stophammer-crawler.install`,
  `packaging/arch/README.md`
- `docs/operations.md`, `AGENTS.md`

In `stophammer-crawler`:

- `README.md`, `AGENTS.md`

## Constraints

- **The Compose service.** Copy the form of `import`, and keep these parts:
  - `profiles: ["tools"]` and `restart: "no"`,
  - the `hardened` anchor and the `crawler-data` volume,
  - the `depends_on` of `crawler-init` and `primary`.

  The `env_file` is `./packaging/env/crawler-podcastindex.compose.env`. The
  command wrapper builds `stophammer-crawler podcastindex --state ...
  --skip-db ... --feed-cache ...` from the same variables as `import`. It
  adds these flags from these variables:
  - `--dry-run` when `PODCASTINDEX_DRY_RUN=1`,
  - `--api-base` when `PODCASTINDEX_API_BASE` is set,
  - `--concurrency` and `--host-delay-ms` when `CONCURRENCY` and
    `HOST_DELAY_MS` are set.
- **The env example.** `PODCASTINDEX_API_KEY=change-me` and
  `PODCASTINDEX_API_SECRET=change-me`, with a comment that names ADR 0081 §2
  and says that the real file stays outside Git. `.gitignore` already ignores
  `*.env`. Confirm this with `git check-ignore -v
  packaging/env/crawler-podcastindex.compose.env`.
- **The systemd units.** Copy `stophammer-import.service` and
  `stophammer-import.timer`. The service reads
  `/etc/stophammer/crawler-podcastindex.env` and runs the same wrapper as
  the Compose service, with the paths under `/var/lib/stophammer-crawler`.
  The timer is `OnCalendar=daily` with `Persistent=true`. Add the two files
  to the PKGBUILD install list, and the env file to the backup list. Add the
  names to `stophammer-crawler.install` and `packaging/arch/README.md`, where
  `import` is named.
- **`docs/operations.md`.** Add the two units to "Shipped example one-shot
  units", and a paragraph after the `import-wavlake` paragraph:
  `docker compose run --rm podcastindex`, the credential file, the first run
  with `PODCASTINDEX_DRY_RUN=1`, and the report line of §8. Say that
  `import` keeps its schedule until the measurement of ADR 0081 §10 is
  complete.
- **`stophammer/AGENTS.md`.** In "Crawler Subcommands", add the
  `podcastindex` example and change "five modes" to "six modes". Add
  `podcastindex` to the list of modes that keep a fetch cache. Change no
  other section.
- **The crawler `README.md`.** Add `### podcastindex` after `### refresh`,
  with the form of that section: what the mode does, the credentials, one
  command, an options table from the flags of `src/main.rs`, the report line
  and the 7-day wait. Name ADR 0081 as the owner. Add
  `PODCASTINDEX_API_KEY`, `PODCASTINDEX_API_SECRET` and
  `PODCASTINDEX_API_BASE` to "Environment variables".
- **The crawler `AGENTS.md`.** Change "Five modes" to "Six modes" and add
  `podcastindex` to the list. Add one paragraph in "Where The Work Stands"
  that names ADR 0081 and the module.
- **No real value.** No file of this task holds a real key or secret.
- **STE.** Write each new paragraph in Simplified Technical English, and run
  the shared checker on each changed document.

## Do Not Touch

- `src/` of the crawler
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `docs/reviews/`
- The `import`, `import-wavlake`, `gossip` and `primary` services
- `migrations/`, `src/` of `stophammer`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the env example and the Compose service. Run
   `docker compose --profile tools config` and confirm that the service
   appears with its command.
2. Write the systemd units and the PKGBUILD changes.
3. Write the paragraphs of `docs/operations.md` and `stophammer/AGENTS.md`.
4. Write the crawler `README.md` section and the `AGENTS.md` lines.
5. Run the STE checker on each changed document.

## Acceptance Criteria

Mechanical:

- `docker compose --profile tools config` in `stophammer` exits 0, and its
  output has a service `podcastindex` whose command holds
  `stophammer-crawler` and `podcastindex`.
- `git check-ignore -v packaging/env/crawler-podcastindex.compose.env` names
  the `*.env` rule.
- `grep -rn "change-me" packaging/env/crawler-podcastindex.compose.env.example`
  gives two lines, and no file of the diff holds a value of 20 or more
  uppercase letters after `PODCASTINDEX_API_KEY=`.
- `cargo test --test adr0045_governance_guards_tests` in `stophammer` passes.
- The STE checker gives no sentence-length or paragraph-length result on the
  new paragraphs.

Visual, for the operator:

- The `### podcastindex` section of the crawler `README.md` has the same
  parts as `### refresh`.
- The `docs/operations.md` paragraph tells a new operator the four steps: the
  credential file, the dry run, the report line, and the schedule.

## Test Commands

```bash
cd ~/build/stophammer
docker compose --profile tools config > /dev/null
git check-ignore -v packaging/env/crawler-podcastindex.compose.env
cargo test --test adr0045_governance_guards_tests
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/operations.md AGENTS.md stophammer-crawler/README.md stophammer-crawler/AGENTS.md
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The flags of `Mode::Podcastindex` in `src/main.rs` differ from the list in
  task 004.
- `docker compose config` fails for a reason outside the new service.
- A step needs a change to a file of "Do Not Touch".

## Expected Final Report

1. files changed, by repository
2. commands run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` and `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/.agents/skills/asd-ste100/SKILL.md` (the prose rules and the checker)
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §1, §2, §8 and §10
- `~/build/stophammer/docs/tasks/adr-0081-task-006-packaging-and-docs.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0081-task-004-pass-and-cli.md` (the flags)
- the files of "Files To Inspect" in this task

Goal:
- The operator can run `stophammer-crawler podcastindex` each day with Compose or systemd, with the credentials in an environment file outside Git. The documents name the sixth mode.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Copy the form of the `import` service and units.
- No file holds a real key or secret.
- Write new prose in Simplified Technical English and run the checker.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/` of each crate, `docs/adr/`, `docs/plans/`, `docs/tasks/`, `docs/reviews/`, the other Compose services, `migrations/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task holds. Report the visual items as open for the operator.

Test commands:
- `docker compose --profile tools config > /dev/null`
- `git check-ignore -v packaging/env/crawler-podcastindex.compose.env`
- `cargo test --test adr0045_governance_guards_tests`
- the STE checker command of this task

At the end, report:
1. files changed, by repository
2. commands run
3. behavior changed
4. deviations from task
5. unresolved concerns
