# ADR 0034 Task 004: The Deploys Of Release A And Release B

Owner: [ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§11. Plan: [the phase plan](../plans/adr-0034-artist-credit-removal-phase-plan.md).

The operator runs each step. An agent prepares the commands and reads the
results. This task changes no code.

## Goal

Release A reaches each known node before release B reaches the primary. The
migration of release B runs with a backup and a measured time.

## Release A: 0.2.0

1. The release notes of 0.2.0 say: "A node accepts a feed or track event with
   no `artist_credit_id`. Upgrade each community node before 0.3.0."
2. Deploy 0.2.0 as usual (ADR 0065).
3. Check: `GET /node/info` of the primary gives the 0.2.0 revision.

## Before Release B

4. List each known community node. For each one, `GET /node/info` gives a
   revision of 0.2.0 or later. A node that gives an older revision, or no
   answer, blocks the deploy of release B. Record the list in the phase plan.
5. Run migration 0047 on a copy of the newest production backup, with the
   0.3.0 candidate image, and with no network. Record the time of the
   migration and the counts of step 7 in the phase plan.

## Release B: 0.3.0

6. The release notes of 0.3.0 say: "The node keeps no artist credit. Each
   community node must run 0.2.0 or later before the primary runs 0.3.0. The
   event types `artist_upserted` and `artist_credit_created` stop."
7. Before the deploy, record these counts on the primary:
   - `SELECT COUNT(*) FROM feeds` and `SELECT COUNT(*) FROM tracks`
   - `SELECT event_type, COUNT(*) FROM events WHERE event_type LIKE 'artist%' GROUP BY 1`
8. Take a backup of the primary database, as the operations guide gives.
9. Deploy 0.3.0.
10. Check, each a mechanical check:
    - The counts of `feeds` and `tracks` are the same as in step 7.
    - `PRAGMA foreign_key_check` gives no row.
    - No table named `artists`, `artist_credit`, `artist_credit_name`,
      `artist_aliases`, `artist_type`, `rel_type` or `external_ids` exists.
    - After the next crawl, the count of `artist%` events does not grow.
    - `GET /v1/feeds/{guid}` of one album gives the same `release_artist` as
      before the deploy.
    - Each community node applies the new events: its sync cursor follows
      the primary within one hour.

## Visual Check

- The operator opens `/api` of the node (`api.musicindex.org/api`) and one
  album page on musicindex.org, and sees the artist names as before.

## Rollback

Release B drops tables. Restore the backup of step 8 and the previous image.
Events that the primary signed after the deploy are then lost from the
primary, so the rollback is possible only in the first hours.
