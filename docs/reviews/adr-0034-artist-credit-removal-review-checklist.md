# ADR 0034 Artist Credit Removal: Review Checklist

Date: 2026-09-27. This list states no rule.
[ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§11 owns each rule. The
[phase plan](../plans/adr-0034-artist-credit-removal-phase-plan.md) gives the
tasks.

## Each Task

- [ ] The diff touches only the files of the task packet.
- [ ] No public read route and no OpenAPI field changes.
- [ ] `release_artist`, `track_artist` and their sort and source fields keep
      their values.
- [ ] The gate of the `stophammer` crate is green.

## Task 001

- [ ] The primary still makes each credit and still signs the two artist
      events.
- [ ] A `Feed` or `Track` with `Some` serializes to the same JSON as before.
- [ ] The local credit uses the same helper and the same placeholder as the
      ingest.

## Task 002

- [ ] The SSE and quality code reads no artist ID and no credit.
- [ ] The score test computes its expected value from the rules, not from
      the new code.
- [ ] No write, no event and no schema changes.

## Task 003

- [ ] The ingest emits no artist event.
- [ ] An old artist event is verified, counted as applied, and changes
      nothing.
- [ ] The migration does not rename the old table first.
- [ ] The migration makes each index and each trigger of the two tables again.
- [ ] `schema.sql` and the migrated schema agree.
- [ ] Each deleted function had no other caller.
- [ ] `docs/API.md` marks the two artist event types as log-only.
