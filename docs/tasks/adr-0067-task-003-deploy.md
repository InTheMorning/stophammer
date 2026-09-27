# ADR 0067 Task 003: The Deploy And The Checks

Owner: [ADR 0067](../adr/0067-a-gone-source-retires-its-feed.md). Plan:
[the phase plan](../plans/adr-0067-gone-source-phase-plan.md).

The operator runs each step. This task changes no code. Release 0.3.0 carries
task 001 and task 002.

## Steps

1. Before the deploy, record on the primary:
   `SELECT COUNT(*) FROM feeds` and
   `SELECT COUNT(*) FROM events WHERE event_type = 'feed_retired'`.
2. Set `SOURCE_GONE_HOSTS=wavlake.com` in the env file of the primary.
3. Deploy the node and the crawler of release 0.3.0.
4. Run a `refresh` pass. It writes one `source_gone_answers` row for each
   Wavlake feed that answers `404`. It retires nothing yet.
5. Record: `SELECT COUNT(*) FROM source_gone_answers`.
6. Run a second `refresh` pass 24 hours or more after the first ended.
7. Record the counts of step 1 again, and
   `SELECT COUNT(*) FROM source_gone_answers`.

## Mechanical Checks

- Step 5 gives about the count of Wavlake `404` answers in the log of the
  pass of step 4.
- After step 6, the `feed_retired` events grew by about the count of step 5,
  and the count of feeds fell by the same number.
- After step 6, `source_gone_answers` has a row only for a feed whose first
  gone answer came less than 24 hours before its second fetch.
- After step 6, a `refresh` pass sends no report for a retired feed. The
  `refresh` mode reads the feed list of the node, and the list no longer
  holds it.

## Visual Check

- The operator opens one retired Wavlake URL in a browser and sees `Not
  found`.

## Stop Condition

When step 5 gives more than 1,000 rows, stop before step 6. A count that large
suggests a fault of the host, not removed albums. Clear the table with
`DELETE FROM source_gone_answers` and report it.
