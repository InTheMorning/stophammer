# ADR 0049 Task 014: The Link Rules Of Namespace PR #793

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §6a,
[ADR 0068](../adr/0068-a-publisher-row-gives-its-link-facts.md) §5 and
[ADR 0069](../adr/0069-an-album-confirms-each-credit.md) §1a.
musicindex.org request 11. Release 0.7.0.

Repository: `stophammer`. The parser does not change. The operator commits.

## Goal

The node follows the link rules of podcast-namespace PR #793:

- Each `publisher` row gives `role_agreement`.
- A link whose two sides state different role sets is not a confirmed link,
  and no count uses it.
- The list facts are role tokens, with a new `agreed_roles`, and
  `stated_rel=` matches one token.
- Only an album item with `source` `podcast_publisher` is a publisher link.

## Files To Inspect

- `src/query.rs`: `PublisherResponse`, `build_publisher_row`,
  `music_to_publisher_facts`, `publisher_to_music_facts`, `resolve_role`,
  `normalize_rel`, `LinkFacts`, `link_facts`,
  `confirmed_and_unconfirmed_release_artists`, `co_credited_feeds`,
  `LinkFactFilter::keeps`, `load_publisher`, `load_track_publisher`
- `src/openapi.rs`: the schemas of the publisher row and of `FeedResponse`
- `tests/adr0049_role_tests.rs`, `tests/adr0068_link_facts_tests.rs`,
  `tests/adr0068_link_fact_filter_tests.rs`, `tests/adr0069_credit_tests.rs`,
  `tests/adr0061_confirmed_artists_tests.rs`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0049_pr793_link_rules_tests.rs`, new
- The ADR 0069 tests of a credit: they test a removed rule. Delete each test
  of a bare-item credit, or change it to the form of PR #793. Report each one.
- `docs/API.md`, `docs/publisher-links-guide.md`

## Constraints

- **`role_agreement`** on each `publisher` row: `both`, `one_side`,
  `conflict` or null, as ADR 0049 §6a gives. Compare the role sets after
  `normalize_rel`. `role` and `role_source` do not change.
- **A confirmed link** is a row with `two_way_validated` true and a
  `role_agreement` that is not `conflict`. Add one helper that decides it, and
  use it in each count: `two_way_link_count`, `stated_rels`, `agreed_roles`,
  `confirmed_release_artists`, `co_credited_feeds`. A `conflict` row stays in
  the `publisher` view with each raw value.
- **`stated_rels`** gives the normalized role tokens, not the raw values:
  lowercased, split as `normalize_rel` splits, different values only, sorted.
- **`agreed_roles`** is new on a publisher list row with `include=link_facts`:
  the tokens of the confirmed links with `role_agreement` `both`, sorted.
  Register it in the OpenAPI schema of `FeedResponse`.
- **`stated_rel=`** normalizes its value as one token, and keeps a row when
  that token is in `stated_rels`.
- **A publisher link** on the album side is an item with `medium="publisher"`
  and `source` `podcast_publisher`. `publisher_to_music_facts`,
  `music_to_publisher_facts` and `load_publisher` skip each other album item.
  Never compare `remote_feed_guid` directly in `src/query.rs`. The ADR 0049
  guard fails.
- **`album_names_as`** gives `publisher` or null.

## Deploy Step

Before the deploy, on the VPS, count the album items that stop being links:

```bash
docker run --rm -v stophammer_primary-data:/node alpine:3.20 sh -c 'apk add -q sqlite && sqlite3 -readonly /node/stophammer.db "SELECT CASE WHEN EXISTS (SELECT 1 FROM feed_remote_items_raw p WHERE p.feed_guid = r.feed_guid AND p.source = '"'"'podcast_publisher'"'"') THEN '"'"'credit next to a publisher'"'"' ELSE '"'"'no publisher item: not ingested since 0.6.0'"'"' END, COUNT(*) FROM feed_remote_items_raw r WHERE r.medium = '"'"'publisher'"'"' AND r.source = '"'"'podcast_remote_item'"'"' GROUP BY 1;"'
```

A row of the second kind is a feed that the node has not read since release
0.6.0. Its links would disappear until its next crawl. Replay the fetch cache
for those feeds before the deploy, as for the `source` values of release 0.6.0.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0049_pr793_link_rules_tests.rs`.

- Both sides `rel="artist producer"` and `"producer artist"`: `role_agreement`
  `both`. The link counts.
- Only the publisher side states `rel="artist"`: `role_agreement` `one_side`,
  `role` `artist`. The link counts. `agreed_roles` does not hold `artist`.
- `rel="label"` and `rel="recordLabel"`: `role_agreement` `conflict`,
  `two_way_validated` true. The row is in the `publisher` view. It is not in
  `two_way_link_count`, `stated_rels`, `agreed_roles`,
  `confirmed_release_artists` or `co_credited_feeds`, and its artist is in
  `unconfirmed_release_artists`.
- No side states `rel`: `role_agreement` null.
- A publisher with `rel="artist host author label producer"` on both sides
  of one link: `stated_rels` and `agreed_roles` give the five tokens, and
  `stated_rel=artist` and `stated_rel=label` each find the row.
- `stated_rel=Label` finds a row with `label`.
- An album with `<podcast:publisher>` and a bare `medium="publisher"` item
  next to it, each listed back: one `publisher` row, `album_names_as`
  `publisher`. The bare item gives no row, and the publisher feed of the bare
  item gives `album_names_as` null and `music_names_publisher` false.
- An album with no `<podcast:publisher>` and one bare `medium="publisher"`
  item: one link, `album_names_as` `publisher`.
- The ADR 0044 guards and the ADR 0049 resolver guard pass.

## Test Commands

```bash
cargo build
cargo test --test adr0049_pr793_link_rules_tests
cargo test --no-fail-fast
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run -q --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. each acceptance criterion with the name of its test
3. each test that was deleted or changed, and why
4. tests run and their results
5. deviations from this task
6. unresolved concerns
