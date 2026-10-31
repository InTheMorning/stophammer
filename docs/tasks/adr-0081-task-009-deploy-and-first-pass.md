# ADR 0081 Task 009: The Deploy And The First Production Pass

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§2, §6 and §8, with [ADR 0065](../adr/0065-a-release-promotes-its-candidate.md)
for the deploy. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: none. The operator does this task with the handoff of task 008.
The agent reads the public API after each deploy and compares the numbers.
This task starts after task 008.

## Goal

The primary and the gossip crawler run release 0.17.0. The `podcastindex`
service has its env file with the real key. A `--dry-run` pass gives the
numbers of §8, and they agree with the Context of the ADR. Then a pass with the
pipeline runs, and a schedule starts one pass each day.

## Context

The release path of `deploy.sh` pulls the two images, stops `gossip` and then
`primary`, pins the version in `.env`, and starts the two services. It does
not copy `docker-compose.yml`. The Compose project directory on the VPS is a
git checkout, and the `podcastindex` service is new in it. The service reads
`./packaging/env/crawler-podcastindex.compose.env`, which Git ignores.

The expected numbers come from 2026-10-31. The music list had 8,601 rows. The
comparison with the backup of 2026-10-25 gave 198 candidates. Of these, 25
were already in production, and the operator crawled 31 into production. The
142 that stay failed in the test crawl: 51 `v4v_payment` rejections, 42 feeds
with no GUID, 3 parse errors and 46 fetch errors. New music feeds in
PodcastIndex add some candidates each day.

## Files To Inspect

- `docs/operations.md`, "Deploy a release with Compose" and the four steps of
  the daily pass
- `packaging/env/crawler-podcastindex.compose.env.example`
- `docker-compose.yml`, the service `podcastindex`
- ADR 0081, "Context" and §8

## Files Likely To Change

- `.idea/releases/0.17.0/deploy-0.17.0.md`, outside Git, when a step changes
- On the VPS, outside Git: `.env`, and
  `packaging/env/crawler-podcastindex.compose.env`

## Constraints

- **The credentials.** The operator puts the key and the secret in the env
  file with an editor, not on a command line. The file has mode 600. No
  command of the handoff holds a credential (ADR 0081 §2, PodcastIndex terms
  §4.2).
- **The first pass is a dry run.** `PODCASTINDEX_DRY_RUN=1` is in the env file
  for the first run. The pass then sends no URL to the pipeline.
- **The comparison.** The dry run gives one report line on standard error.
  The pass with the pipeline runs only when each number is in its band:
  - `rows` between 8,000 and 12,000,
  - `candidates` between 100 and 300,
  - `waiting` 0 and `sent` 0,
  - `import_gap` `none`, because no previous pass exists.

  A number outside its band is a cause to examine before the next step, also
  when the pass did not stop under §6.
- **The schedule.** The daily run starts at an hour that differs from the
  `import` run, because both write `import_state.db` (the phase plan, "Two
  writers"). The operator chooses cron on the host or a host timer. The
  handoff gives the exact line.
- **The promotion.** After the checks on the candidate, the operator tags
  `v0.17.0` on the three repositories, sets the notes on the release page and
  deploys `0.17.0` (ADR 0065 §2). The handoff of task 008 gives the blocks.
- **The probe output.** After the first pass, the operator deletes
  `.idea/podcastindex/run1/` and `.idea/podcastindex/run2/` on this machine.
  They hold API content (PodcastIndex terms §5.5).

## Do Not Touch

- The database of the primary and the crawler state, other than through the
  crawler
- `docs/`, `packaging/`, `docker-compose.yml` in Git

## Implementation Steps

Here:

1. `./deploy.sh --release 0.17.0-rc.1 --backup`. The script ends with the
   `git_revision` of `v0.17.0-rc.1`.

On the VPS:

2. In the Compose project directory, `git pull --ff-only`. The result holds
   the service `podcastindex` in `docker-compose.yml`.
3. Copy `packaging/env/crawler-podcastindex.compose.env.example` to
   `packaging/env/crawler-podcastindex.compose.env`, and set its mode to 600.
   With an editor, set `CRAWL_TOKEN`, `PODCASTINDEX_API_KEY` and
   `PODCASTINDEX_API_SECRET`, and add `PODCASTINDEX_DRY_RUN=1`.
4. Run `docker compose --profile tools run --rm podcastindex`. Keep the report
   line and each `429` line.
5. Compare the report line with the bands of "Constraints".
6. Remove `PODCASTINDEX_DRY_RUN` from the env file, and run the command of
   step 4 again. Keep the report line.
7. Add the daily schedule, with the hour of the handoff.

Here:

8. Tell the agent the two report lines. The agent compares them with the
   Context of the ADR, and reads `GET /node/info` and `GET /openapi.json`.
9. Promote and deploy `0.17.0` with the blocks of the handoff.
10. Delete `.idea/podcastindex/run1/` and `.idea/podcastindex/run2/`.

## Acceptance Criteria

Manual, because the checks need the real API and the real node:

- `GET /node/info` gives the commit of `v0.17.0-rc.1`, and after the
  promotion the commit of `v0.17.0`. `GET /openapi.json` gives `info.version`
  `0.17.0`.
- The dry run gives one report line, and each number is in its band.
- The pass with the pipeline gives `sent` equal to `candidates` minus
  `waiting`, and the sum of the outcomes equals `sent`.
- The next day, the schedule ran a pass. The report line of that pass gives
  `waiting` near the number of failed candidates of the first pass, and
  `import_gap` gives a count.
- `.idea/podcastindex/run1/` and `.idea/podcastindex/run2/` do not exist.

## Test Commands

```bash
curl -s https://api.musicindex.org/node/info
curl -s https://api.musicindex.org/openapi.json | python3 -c 'import json,sys; print(json.load(sys.stdin)["info"]["version"])'
```

## Escalation Triggers

Stop and report, with no further step, when:

- The dry run stops under §6, or a number is outside its band. The agent
  examines the cause with the operator before the pass with the pipeline.
- The pass with the pipeline gives `accepted` above 50. The band of the
  Context is near 0, because the 142 candidates failed on 2026-10-31.
- `deploy.sh` stops before the restart, or `GET /node/info` gives another
  commit.

## Expected Final Report

The operator gives the agent:

1. the two report lines, with their dates
2. each `429` line of the two runs
3. the result of `deploy.sh` for the candidate and for the release
4. the schedule line and its hour
5. unresolved concerns
