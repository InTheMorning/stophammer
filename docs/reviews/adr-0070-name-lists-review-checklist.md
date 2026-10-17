# ADR 0070 Review Checklist: Lists In Name Order

This checklist is advisory.
[ADR 0070](../adr/0070-a-name-list-gives-each-identity-one-row.md) owns each
rule. The reviewer examines the diff of each task with it.

## Each Task

- [ ] The diff changes only the files of the task, or the report gives the
      reason for each other file.
- [ ] No existing route, field or column changes its value.
- [ ] No cleanup or refactor outside the task.
- [ ] Each acceptance criterion of the task has a test, and the test fails
      without the change.
- [ ] The pre-commit gate is green: `cargo build`, `cargo test`,
      `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.
- [ ] Each new module item has a doc comment, and each new rule names ADR
      0070 and its section.
- [ ] `docs/API.md` gives the new route.
- [ ] No git write command ran.

## Task 001

- [ ] One Rust function gives the sort key. No SQL `lower()` makes a key.
- [ ] Migration 0051 is the next entry of `MIGRATIONS`, and `src/schema.sql`
      has the column and the index.
- [ ] The feed upsert writes `title_sort` each time that it writes `title`.
- [ ] The fill at open writes each null `title_sort`, in one transaction, and
      does nothing on a second open.
- [ ] `title_lower` is unchanged.
- [ ] No signed event carries `title_sort`. The write derives it from
      `title`, also when a community node applies an old event.
- [ ] The query reads the index `(title_sort, feed_guid)`. Examine
      `EXPLAIN QUERY PLAN`.
- [ ] A request with `cursor` ignores `from`.
- [ ] The report gives the time of the fill and the two page times.

## Task 002

- [ ] The name level reads the link facts of the publisher view, with no
      second rule.
- [ ] A `conflict` link and a link with no `rel` keep a release at the
      publisher level.
- [ ] No row merges a `publisher` row and a `name` row.
- [ ] The cursor encodes `(sort_name, kind, key)`, and `name` comes before
      `publisher`.
- [ ] `image_url` passes `web_url_or_none`.
- [ ] The list is in memory (ADR 0070 §4a). No derived table and no
      migration exist.
- [ ] The copy is built again only when `max(seq)` changed and the interval
      has passed. Only one build runs at a time.
- [ ] The report lists each write of the task and its event row.
- [ ] A lock is not held across an `await`, and a build runs in
      `spawn_blocking`.
