# ADR 0078: A Search Index Row Holds Only Searchable Text

## Status
Proposed on 2026-10-23, for the research request on the search index and item 1
of v4vmm request 10. It amends ADR 0022: the columns of `search_index`, and
step 2 of "Insert/update". The rest of ADR 0022 stays.

## Date
2026-10-23

## Context

[The research request of 2026-10-23](../plans/research-search-index-request.md)
reports four defects in the search index.
[The review of 2026-10-23](../reviews/requests-after-release-0.13.2-review.md)
and its addendum confirm each defect in the source at `9ce7e16`. S1, S3 and S4
are also observed on the live API or in an SQLite test.

| # | Defect | Cause |
|---|---|---|
| S1 | After a change of a title, an artist or a description, the words from before the change stay in the index. The bm25 statistics also become incorrect | `populate_search_index` sends the new text in the FTS5 `'delete'` command (`src/search.rs:139-145`). A contentless table needs the text that it indexed, and no table keeps that text |
| S2 | A retired publisher feed stays in the index | The two retire paths delete only the row of the type `feed` (`src/api.rs:4803`, `src/apply.rs:307`). A publisher feed has a row of the type `publisher` (ADR 0071 §5). The delete also sends `""` as `name` |
| S3 | The internal columns match words. `q=track` matches each track, and `q=music` matches each music feed | `entity_type` and `entity_id` are indexed columns, and `tags` holds `raw_medium` |
| S4 | Each publisher hit gives `quality_score` 0 | `src/db.rs:1419` stores the score with the type `feed`. The search joins `entity_quality` on the type of the hit, `publisher` |

The text of `q` also goes to FTS5 as a query expression. `sanitize_fts5_query`
removes some operators. It keeps an apostrophe and a colon. Thus
`q=haleen'` gives `400`, and `artist:foo` gives `400`. The `400` message gives
the SQLite error text (`src/query.rs:5189`).

v4vmm request 10 item 1 asks that
each text of `q` gives `200`. The research request gives this change priority 1.

SQLite 3.43 added the FTS5 option `contentless_delete=1`. With it, a plain
`DELETE` by rowid removes the indexed words of a contentless row. The bundled
SQLite is 3.45.0 (`libsqlite3-sys` 0.28.0). The research reproduced S1 and the
correction in SQLite (`fts5_contentless_delete_test.py` in the research
repository).

The rebuild of the index on 2026-09-26 took 3.7 s
(`docs/reviews/v4vmm-request-5-search-duplicates-verification.md`).

## Decision

### 1. The index holds only searchable text

`search_index` has three columns: `name`, `title` and `description`. It stays
contentless, with `tokenize='unicode61'`, and it adds `contentless_delete=1`:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    name,
    title,
    description,
    content='',
    contentless_delete=1,
    tokenize='unicode61'
);
```

The table has no `entity_type`, `entity_id` or `tags` column. `search_entities`
stays the map from a rowid to the entity, as ADR 0022 states. The index holds no
medium text. The `type` parameter and the medium rules of ADR 0071 §5 select
the entity types.

This corrects S3.

### 2. A delete needs no old text

A delete of an index row is a `DELETE` by rowid. An update deletes the row and
then inserts the new text at the same rowid. No write sends the old text to
FTS5. The guard of ADR 0022 on `search_entities` stays, and the companion row
changes in the same transaction as the index row.

`populate_search_index` and `delete_from_search_index` stay the only writers of
the two tables (ADR 0022). `delete_from_search_index` takes the entity type and
the entity ID only.

This corrects S1.

### 3. A retire deletes each row of the feed

When a feed is retired, each path deletes the index rows of the types `feed`
and `publisher` for that feed. It also deletes the rows of its tracks. An error
of a delete fails the retire. Each other write in its transaction has the same
rule. No path discards the result of a delete.

This corrects S2.

### 4. A publisher hit gives the quality of its feed

A publisher feed keeps one quality row, of the type `feed`. The search reads
the quality of a `publisher` hit from that row. `entity_quality` gets no row of
the type `publisher`.

This corrects S4.

### 5. The text of `q` is words

The node divides `q` at white space. It puts each word in double quotes as one
FTS5 phrase, and it doubles each double quote in the word. The phrases join
with the implicit AND of FTS5. Thus no character of `q` is an FTS5 operator, a
column filter or a prefix.

- Each `q` that has a word gives `200`, possibly with no rows.
- A `q` with no word gives `400`, as a missing `q` does now.
- A `400` message of `GET /v1/search` gives no SQLite error text. The log keeps
  that text.
- The contract describes `q` as words that each hit contains. It no longer says
  "FTS5 syntax".

This decision adds no phrase syntax, no field filter and no prefix match. The
decision on the other items of v4vmm request 10 owns those.

### 6. One rebuild at the first open

On the first open of a release with this decision, the node replaces the
table and fills it one time from the source tables. The repair keys on the
definition of `search_index` in `sqlite_master`. A definition without
`contentless_delete` causes the repair. A second open does nothing. The repair
follows ADR 0046, so it also runs when the migration runner skips the
migration.

A community node runs the same repair. No event payload changes, so the
primary and the community nodes can upgrade in each sequence.

## Alternatives Considered

### A content table

A content table keeps a copy of each text, so a delete can send the old text.
It doubles the size of the index, and ADR 0022 rejected it for that reason.
`contentless_delete=1` gives the same correction with no copy.

### A periodic rebuild

A rebuild from the source tables at a fixed interval removes the old words.
Between two rebuilds the index stays incorrect, and each rebuild writes the
full index.

### Store a quality row of the type `publisher`

This needs a second write at each ingest, and a second delete at each retire.
The score is the score of the feed, so one row is sufficient.

### Keep a query syntax and give a stable error code

Request 10 item 2 offers this method. It keeps the `400` for a name with an
apostrophe. No client asks for FTS5 syntax, and v4vmm ADR 0087 D5 sends fields
as parameters, not as syntax.

## Consequences

- One rebuild runs at the first open after the deploy. The rebuild of
  2026-09-26 took 3.7 s.
- `q=track`, `q=feed` and `q=music` match only the text of the entity. A search
  for a medium word no longer finds each feed of that medium.
- A cursor from before the deploy can give a different page, because the bm25
  values change after the rebuild. A cursor is opaque, and no client keeps one
  across a deploy.
- A query with `title:foo` now searches for the words `title:foo`. It no longer
  filters the `title` column by chance.
- The decision makes the node keep no new value from a feed body, so ADR 0077
  §1 states no refill pass for it. The rebuild of §6 reads the source tables.
- `docs/API.md` and the `q` description in `src/openapi.rs` change in the same
  commit as the code.
- The tests of `tests/issue21_fts5_sanitize_tests.rs` assert the old
  `sanitize_fts5_query`. The tests of §5 replace them.

## Checks

Each criterion is mechanical. A test in `tests/` proves it.

1. §1: the definition of `search_index` in `sqlite_master` has the three
   columns and `contentless_delete=1`.
2. §2: a test changes the title of a feed and of a track. Then a search for a
   word that only the old title had gives no hit. A search for a word of the new title
   gives the entity one time.
3. §2: after the change, the bm25 rank of an unchanged entity equals its rank in
   a new index that holds the same rows.
4. §3: a test retires a publisher feed by the API path and by the apply path.
   After each retire, `search_entities` has no row for that feed. A search for
   its title or artist gives no hit.
5. §3: a retire path that receives an error from a delete gives that error.
6. §4: the `quality_score` of a `publisher` hit equals the score of the `feed`
   quality row of that feed.
7. §1: `q=track`, `q=feed` and `q=music` give no hit for an entity that does
   not have the word in its text.
8. §5: `q` values with an apostrophe, a double quote, a colon, `*`, `-`, `(`,
   `AND`, `NEAR` and `title:foo` each give `200`.
9. §5: a `400` of `GET /v1/search` gives no text from SQLite.
10. §6: a database with the old definition opens with the new definition and
    the same number of `search_entities` rows. A second open runs no repair.

No criterion is visual.
