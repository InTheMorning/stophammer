# Review Checklist: Compression, Value Time Splits And The Feed GUID

This checklist is advisory. ADR 0072, ADR 0073 and ADR 0074 own each rule. The
reviewer examines the diff of each task with it.
[The phase plan](../plans/compression-splits-guid-phase-plan.md) gives the
sequence.

## Each Task

- [ ] The ADR of the task is Accepted.
- [ ] The diff changes only the files of the task, or the report gives the
      reason for each other file.
- [ ] No cleanup or refactor outside the task.
- [ ] Each acceptance criterion of the task has a test, and the test fails
      without the change.
- [ ] The pre-commit gate is green in each crate that the task changes.
- [ ] Each new item names its ADR and section.
- [ ] No test sends a request to an external host.
- [ ] No git write command ran.

## ADR 0072 Task 001

- [ ] `reqwest` has `gzip` and `brotli`, and no other new feature.
- [ ] `Accept` is set in `build_hop_request`, not with `default_headers`.
- [ ] The snapshot client and the SSE client call `.no_gzip()` and
      `.no_brotli()`.
- [ ] The hash code did not change, and a gzip body and a plain body give the
      same hash in a test.
- [ ] The report gives the result of the two research tests before the change.

## ADR 0072 Task 002

- [ ] The cache stores and reads the `ETag` unchanged.
- [ ] Each row of the table in the task has a test.
- [ ] A weak tag keeps `W/` on both tags of the list.

## ADR 0072 Task 003

- [ ] `wasted_200` is `true` only for a conditional request, a `200` and an
      unchanged hash.
- [ ] The ingest decision and the cache write did not change.
- [ ] The report line keeps its present buckets.

## ADR 0074 Task 001

- [ ] The change only adds to the parser API.
- [ ] `raw_medium()` gives the same text as `IngestFeedData::raw_medium`.
- [ ] The `NoGuid` text is `missing podcast:guid`.

## ADR 0074 Task 002

- [ ] No crawler function has a `fallback_guid` argument.
- [ ] No path posts `feed_data: null` after a parse. The gone report of ADR
      0067 did not change.
- [ ] `is_unparsed_channel_without_medium` is the only new skip rule, and both
      stores call it.
- [ ] Each row of the table of ADR 0074 §3b has a test in each store.
- [ ] The skip of a known success, the medium-gate rejection, the 7-day rule
      and the `413` rule did not change.

## ADR 0074 Task 003

- [ ] No fallback GUID code stays in the parser, its CLI or its docs.
- [ ] The crawler builds and passes its tests against the parser.

## ADR 0073 Task 001

- [ ] The migration copies each old row as a remote row.
- [ ] The store holds stated values, with no default and no limit.
- [ ] Each delete of a split deletes its recipient rows first.
- [ ] An old ingest and an old event apply, each in a test.
- [ ] No `v1` response field changed.
- [ ] The release notes need the line "community nodes upgrade first". The
      reviewer adds it to the release handoff.

## ADR 0073 Task 002

- [ ] Each present field of `value_time_splits` keeps its name and type.
- [ ] `split` applies the default 100 and the limit 0 to 100.
- [ ] The contract declares `local_value_time_splits` and each new field.
- [ ] `docs/API.md` gives an example of each form.

## ADR 0073 Task 003

- [ ] The JSON of the parser type agrees with the node type of task 001.
- [ ] The track payment routes did not change.
- [ ] The rules apply in the order of the task.

## Before The Release

- [ ] The release notes say that community nodes upgrade first.
- [ ] The release handoff gives the check of the Satellite Skirmish feed and
      the check of the wasted `200` count.
- [ ] ADR 0050 has the amendment notes of ADR 0072 §5 and §6.
- [ ] ADR 0011 moves to `docs/adr/archive/` when ADR 0072, ADR 0073 and
      ADR 0074 are Accepted, with the index row.
