# ADR 0064 Review Checklist

Date: 2026-09-27. This checklist states no rule. The planner applies it to the
diff of each task of the
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

## Each Task

- [ ] The diff changes only the files of the task, or the report names each
      other file and its reason.
- [ ] The diff does not touch a path under "Do Not Touch".
- [ ] Each acceptance criterion has a test, and the test fails without the
      change.
- [ ] The gate is green in each changed crate: build, test, clippy, fmt.
- [ ] No `#[allow]` without a reason. An exception uses `#[expect(…, reason)]`
      at the narrowest scope.
- [ ] No opportunistic change: no rename, move or cleanup outside the task.
- [ ] New prose is in Simplified Technical English.
- [ ] No git write command ran.

## The Invariants Of ADR 0064

- [ ] A read gives each stored field as the feed gave it. Only
      `confirming_relay` and the views are derived.
- [ ] No track, payment route or value time split comes from a live item.
- [ ] Each rule of §4 and §6 is in `src/live.rs`. The ingest and the reads
      call it, and do not copy it.
- [ ] An unchanged set of live items emits no `LiveEventsReplaced`.
- [ ] The index does not call a relay.
- [ ] The list order does not depend on a value that the feed controls.

## Task-Specific Points

| Task | Check |
|---|---|
| 001 | Only a direct `liveValue` child of the live item counts. An identifier-only `uri` stays as it is |
| 002 | The new fields have `#[serde(default)]`. An old payload applies. Migration 0044 only adds columns |
| 002b | A normal item still becomes a track. The ban does not apply to `pending` or `ended`. The cap order is: deduplicate, ban, the two caps |
| 003 | The host list is not in `AppState`. `is_confirming_relay` needs `https` and a listed host. The margins are exactly 3600 seconds |
| 003b | Raw filters only with `view=all`. The handler reads the clock one time. A feed that is not public gives no row |
| 004 | The crawler has no code change other than a fixture |

## The Result Of A Review

For each task the planner records:

1. pass or fail;
2. the required fixes;
3. the optional improvements;
4. whether the operator can commit the task;
5. whether the next task packet needs a change.
