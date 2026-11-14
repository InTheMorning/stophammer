# ADR 0100: The Search

## Status
Accepted

- Accepted: 2026-11-14. The operator accepted the consolidation of each
  changed decision (musicindex-research `df7b823`). This ADR makes no new
  decision. It restates each live rule of the ADRs that it supersedes, with
  each change from a subsequent ADR.
- Supersedes: [ADR 0078](archive/0078-a-search-index-row-holds-only-searchable-text.md),
  A Search Index Row Holds Only Searchable Text.
- Supersedes: [ADR 0080](archive/0080-a-search-takes-phrases-and-field-filters.md),
  A Search Takes Phrases And Field Filters.
- Supersedes: [ADR 0086](archive/0086-a-search-matches-the-start-of-the-last-word.md),
  A Search Matches The Start Of The Last Word.
- Supersedes: [ADR 0087](archive/0087-a-search-orders-its-hits-by-field.md),
  A Search Orders Its Hits By Field.
- Supersedes: [ADR 0094](archive/0094-a-feed-hit-states-its-medium-kind-and-date.md),
  A Feed Hit States Its Medium, Kind And Date.
- Supersedes: ADR 0071 §5 and ADR 0085 §4 to §6. ADR 0099 supersedes the other
  sections of these two ADRs.
- Restates: the change of ADR 0078 to ADR 0022, in items 1 and 2.
- Restates: the changes of ADR 0071 §5 and ADR 0094 to ADR 0038 Decision B,
  in items 7 to 9.
- Revised: 2026-11-14, by the operator decision of that day (musicindex-research
  `reports/2026-11-14-adr-overuse-global-decision.md`). Item 9: `medium` is not
  a positive term, and a repeated `medium` gives `invalid_filter`.
- Uses: [ADR 0022](0022-fts5-search-architecture.md) for `search_entities`, the
  rowid, the quality weight and the cursor.
  [ADR 0099](0099-the-lists-and-their-rows.md) for `sort`, `reverse`, the order
  key and the sort fields of a search row.
  [ADR 0076](0076-a-named-feed-entry-states-its-confirmation.md) §3 for the link
  facts of a `publisher` row. ADR 0046 for a repair at open. ADR 0044 for the
  contract.

## Date
2026-11-14

## Context

- Five ADRs from 2026-10-18 to 2026-11-10 made the search rules. ADR 0071 §5
  and ADR 0085 §4 to §6 also gave search rules. Each subsequent ADR changed
  some items of an earlier ADR. On 2026-11-14 the shared ADR link guard found
  ADR 0080 with more than three changes.
- The global decision of 2026-11-14 (musicindex-research `df7b823`) sets one
  current ADR for an area after three changes. This ADR is that ADR for the
  search. ADR 0099 is that ADR for the lists, and it also holds the sort of a
  search.
- The superseded ADRs keep their context, their evidence, their alternatives,
  their operator answers and their checks in `docs/adr/archive/`. The item map
  at the end of this ADR gives the new item of each old section. A citation in
  code or in a test, for example "ADR 0080 §4", resolves through that map.

## Decision

### The Index

1. `search_index` has three columns: `name`, `title` and `description`. It
   keeps `tokenize='unicode61'`. It is not contentless: FTS5 keeps a copy of
   the text of each row.

   ```sql
   CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
       name,
       title,
       description,
       tokenize='unicode61'
   );
   ```

   The table has no `entity_type`, `entity_id` or `tags` column.
   `search_entities` stays the map from a rowid to the entity (ADR 0022). The
   index holds no medium text. The `type` parameter and `medium` (items 7 to 9)
   select the row types.
2. A delete of an index row is a `DELETE` by rowid. An update deletes the row
   and then inserts the new text at the same rowid. No write sends the old text
   to FTS5. FTS5 reads the old text from its copy, so the delete removes each
   old word and keeps the bm25 statistics correct. The guard of ADR 0022 on
   `search_entities` stays, and the companion row changes in the same
   transaction as the index row.
3. `populate_search_index` and `delete_from_search_index` stay the only writers
   of the two tables (ADR 0022). `delete_from_search_index` takes the entity
   type and the entity ID only.
4. Each retire path uses the feed delete `delete_feed_sql`. It deletes the
   index rows of the feed, of each row type, and the index rows of the tracks
   of the feed. It does this in the same transaction as the other deletes of
   the feed. No retire path deletes index rows of its own. This applies to each
   path that retires a feed:
   - the admin retire, `DELETE /v1/feeds/{guid}`,
   - the retire of ADR 0057 for `podcast:block`,
   - the retire of ADR 0067 for a gone source,
   - the retire of an old GUID by a GUID change of ADR 0052,
   - the apply of `FeedRetired` on a community node.

   An error of a delete fails the retire, as an error of each other delete in
   the transaction does. No path discards the result of a delete.
5. A publisher feed keeps one quality row, of the type `feed`. The search reads
   the quality of a `publisher` hit from that row. `entity_quality` gets no row
   of the type `publisher`.
6. On the first open of a release with item 1, the node replaces the table and
   fills it one time from the source tables, in one transaction. The repair
   runs before each other repair in `open_db` that writes the index. It runs
   when one of these conditions is true:
   - The definition of `search_index` in `sqlite_master` has `content=''`.
   - `search_entities` has no row, and the node holds a feed.

   A second open after a complete repair does nothing. The repair follows ADR
   0046, so it also runs when the migration runner skips the migration. The
   fill reads only the feeds that the node holds. A community node runs the
   same repair. No event payload changes.

### The Row Types

7. The index holds each feed and each track as an entity of one row type:

   | Record | Row type |
   |---|---|
   | A feed with the medium `music` | `feed` |
   | A feed with the medium `publisher` | `publisher` |
   | A feed with the medium `musicL` | `playlist` |
   | A track | `track` |

   - A feed is at most one of the row types `feed`, `publisher` and `playlist`.
   - A change of the medium deletes the row of the type that the feed left.
   - A feed with no medium, or with an unknown medium, stays out of the index.
   - On the first start of a release with a new row type, the node adds each
     feed of that type that the index does not hold. It reads only the stored
     rows, and no crawl is necessary.
8. `GET /v1/search` takes `type=publisher`. It gives the publisher feeds whose
   text matches, in the row shape of a feed result, with the `entity_type`
   `publisher`. A search with `type=feed`, with `type=track` or with no `type`
   does not read the type `publisher`.
9. `GET /v1/search` takes `medium`. Its value is a comma list of `music` and
   `musicL`. With no `medium`, the value is `music`.
   - The search reads the row type `playlist` only when `medium` names
     `musicL`.
   - `medium=music,musicL` gives the music feeds and the playlists.
   - With `type=feed`, `medium=musicL` gives only the playlists.
   - `medium` applies to the feed rows of `type=feed` and of a search with no
     `type`. With no `type`, the track rows do not change, so `medium=musicL`
     gives the playlists and the tracks.
   - With `type=track` or `type=publisher`, `medium` gives `400` with
     `unsupported_filter`.
   - Each other value, `publisher` included, gives `400` with
     `invalid_filter`.
   - `medium` is not a positive term of item 21. It always has a value, so as a
     positive term it would let a search with no word list the full index. A
     request with `medium` and no other positive term gives
     `missing_positive_term`.
   - A repeated `medium` gives `400` with `invalid_filter`, as a repeated
     positive filter does.
   - The filters of items 15 to 21 apply to a playlist and to a music feed in
     the same way. A playlist has `episode_count` 0, so `min_tracks=1` removes
     it.

   A playlist hit gives the `entity_type` `feed`, and a client reads it at
   `GET /v1/feeds/{guid}`.

### The Text Of `q`

10. The node reads `q` from left to right. It first removes each control
    character, for example NUL.
    - Text between two double quotes is one phrase. The node gives it to FTS5
      as one phrase, so it matches its words in sequence.
    - A double quote with no second double quote after it is part of its word.
    - Each other text divides into words at white space. Each word is one FTS5
      phrase, and the node doubles each double quote in the word.
    - A phrase with no word, for example `""`, adds nothing.
    - The words and the phrases join with the implicit AND of FTS5.

    No character of `q` is an FTS5 operator, a column filter or a prefix. A `*`
    in `q` stays part of its word.
11. If the last item of `q` is a word, FTS5 reads that word as the start of a
    word. Each other word and phrase stays whole.

    | `q` | Matches |
    |---|---|
    | `jsmo` | A row with a word that starts with `jsmo` |
    | `night dr` | A row with the word `night`, and a word that starts with `dr` |
    | `"night dr"` | A row with the words `night dr` in sequence, as whole words |
    | `jsmo ""` | The same rows as `jsmo`, because `""` adds nothing |
    | `"jsmo` | The same rows as `jsmo`, because the double quote is part of the word |

    - Text in double quotes never matches as a prefix. A client sends `"love"`
      when it wants only the whole word.
    - White space after the last word does not change the match.
    - The rule has no minimum length. So each new letter of a live search can
      only remove rows.
12. Each `q` that has a word or a phrase gives `200`, possibly with no rows. A
    `400` message of `GET /v1/search` gives no SQLite error text. The log keeps
    that text. The contract describes `q` as words that each hit contains.

### The Rank

13. With `sort=relevance` (ADR 0099), the node runs two matches: the exact
    query, and the prefix query of item 11 for the other rows. It puts each hit
    in the first group that matches. The rule is the same for each type.

    | Group | The hit |
    |---|---|
    | 1 | All words of `q` are in the title, or all words of `q` are in the artist name |
    | 2 | All words of `q` are in the title and the artist name together |
    | 3 | Each other hit. This includes a match in the description |

    - In each group, a hit with the whole words comes before a hit by prefix.
    - In one such set, the BM25 score and the quality weight of ADR 0022 give
      the sequence.
    - An artist name has the group of a title.
    - For one phrase, the node reads one column filter, because the second
      group cannot have a hit.
    - A request with no `type` runs one statement for feeds and tracks.
    - The node checks the type of a row before it calculates BM25.
    - The hits are the outer loop of the join.
    - The node keeps the prepared rank statement on each connection.
14. `effective_rank` is the score with the quality weight, plus the offset of
    the group and of the prefix set. The cursor holds `effective_rank` and the
    `rowid`, and it keeps the form of ADR 0022. `rank` is the BM25 score with no
    offset, so it is 0 or less.
    - A hit with the whole word has the `rank` of the exact query.
    - A hit by prefix only has the `rank` of the prefix query.
    - `rank` has the same value on each sort.
    - The sequence of the rows is the order. `rank` does not give the order,
      and a client must not sort by it.

### The Filters

15. `GET /v1/search` takes these filter parameters. Each filter applies
    together with `q` and with each other filter. No filter gives an OR between
    two fields.

    | Parameter | Value | `type=feed` | `type=track` | `type=publisher` |
    |---|---|---|---|---|
    | `artist` | text | `release_artist` | `track_artist` | the publisher title |
    | `title` | text | the feed title | the track title or the title of the feed of the track | the publisher title |
    | `release` | text | the feed title | the title of the feed of the track | unsupported |
    | `kind` | `album`, `ep`, `single`, `unknown`, with commas | `release_kind` | the `release_kind` of the feed of the track | unsupported |
    | `tracks`, `min_tracks`, `max_tracks` | a count | `episode_count` | the `episode_count` of the feed of the track | unsupported |
    | `after`, `before` | a date, `YYYY-MM-DD` | `newest_item_at` | `pub_date` | unsupported |
    | `min_duration`, `max_duration` | seconds | unsupported | `duration_secs` | unsupported |
    | `host` | a host name | the host of `feed_url` (item 20) | the host of the feed of the track | the host of `feed_url` |

16. The value rules:
    - **Text.** The value of `artist`, `title` and `release` has words and
      phrases, with the rules of item 10. Each word and each phrase must be in
      the named field. A filter matches whole words and takes no prefix.
      `artist=jsmo` does not match `jSmovie`.
    - **Track title.** Each word and each phrase of `title` on a track can match
      the track title or the title of its feed. `title=love song` matches a
      track "Love" on the album "Song". A phrase in double quotes must match in
      one title. `release` reads only the title of the feed. No filter reads
      only the title of the track.
    - **Release kind.** `kind` matches the stated `release_kind` only.
      `kind=unknown` matches `unknown` and a feed with no release kind. The
      commas give an OR of the listed kinds.
    - **Counts.** `tracks` matches one count. `min_tracks` and `max_tracks`
      include their limit. A count is an integer from 0.
    - **Dates.** A date is a day in UTC. `after` matches a value on that day or
      later. `before` matches a value on that day or earlier. A row with no date
      does not match a date filter. An `after` later than a `before` gives `200`
      with no rows.
    - **Durations.** A duration is an integer number of seconds from 0. The two
      limits are included. A track with no `duration_secs` does not match a
      duration filter.
17. A search with no `type` merges feeds and tracks. It applies the `type=feed`
    rule to a feed row and the `type=track` rule to a track row. It accepts a
    filter only when `type=feed` and `type=track` each support it.
    `min_duration` and `max_duration` give the error of item 21. Item 9 gives
    the rule of `medium`.
18. The node reads each filter from the stored rows. A filter selects rows. It
    does not change the rank of a row.
19. `not_q`, `not_artist`, `not_title`, `not_release` and `not_host` each remove
    the rows that match their value. A `not_` parameter applies to the same
    field as its positive parameter, so `not_title=love` removes a track "Love"
    and each track on an album "Love". `not_q` applies to `name`, `title` and
    `description`, as `q` does.
    - Each `not_` parameter can repeat. Each value is one exclusion.
    - A value with two words removes only a row that has the two words.
    - An exclusion matches whole words and takes no prefix.
    - A request has at most 10 exclusions.
    - A positive filter does not repeat.
20. `feeds.feed_host` holds the host of `feed_url`, as the `url` crate parses
    it, in lower case. A URL that does not parse gives no host.
    - Each statement that writes `feed_url` also writes `feed_host`, in the same
      statement.
    - A repair of ADR 0046 in `open_db` fills each row with no host from its
      `feed_url`.
    - A community node calculates the host from its own `feed_url`. No event
      payload changes.
    - A `host` value matches its host and each subdomain. `wavlake.com` matches
      `wavlake.com` and `feeds.wavlake.com`. It does not match
      `notwavlake.com`.
    - The node reads the `host` value with the host rules of the `url` crate. A
      value with a scheme, a path, a port or user data gives `invalid_filter`.
    - A feed with no host does not match `host`, and `not_host` does not remove
      it.

    The column has no index. A host filter reads each feed.
21. A `400` of `GET /v1/search` for a filter has this body:

    ```json
    {"error": "min_duration is not supported for type=feed", "code": "unsupported_filter", "filter": "min_duration"}
    ```

    | Code | Cause |
    |---|---|
    | `unsupported_filter` | The type of the request cannot apply the filter, or the node does not know the parameter |
    | `invalid_filter` | The value is not correct, a positive filter or `medium` repeats, or the request has more than 10 exclusions |
    | `missing_positive_term` | The request has no word, no phrase and no filter of item 15 |

    - `filter` gives the name of the parameter. A body with the code
      `missing_positive_term` has no `filter`.
    - A parameter that the node does not know gives `unsupported_filter`. Thus
      a client that sends a newer filter to an older node gets an error, and no
      rows that the filter did not reduce.
    - ADR 0099 gives the error codes of `sort` and `reverse`, with the same
      body.

### A Search With No `q`

22. A request with one or more filters and no word or phrase in `q` runs on
    `GET /v1/search`. It reads the same rows that the index holds, so items 7
    to 9 apply.
    - Its default sort is `date` (ADR 0099): `newest_item_at` for a feed and a
      publisher, and `pub_date` for a track. A request with no `type` merges
      feeds and tracks by that date.
    - A row with no date comes after each row with a date. Rows with the same
      date, or with no date, come in rowid order.
    - With the default sort, its cursor holds the date and the rowid. A cursor of a search with `q`
      gives `400` on a search with no `q`, and a cursor of a search with no `q`
      gives `400` on a search with `q`.
    - `rank` is 0 on each of its rows.

    A cursor is valid only with the parameters of the request that gave it.

### The Hit Fields

23. Each hit with the `entity_type` `feed` or `publisher` gives `medium`: the
    stored `raw_medium` of the feed. A track hit does not give the field.
24. Each hit with the `entity_type` `feed` or `publisher` gives `release_kind`
    and `newest_item_at`. Each value is the value that `GET /v1/feeds/recent`
    gives for the same feed. Each can be null, and the key is then present with
    a null value. `pub_date` stays empty on a feed hit. A playlist hit gives
    `newest_item_at` null, because a playlist holds no tracks.
25. The node infers no release kind. The kind rule stays with the apps
    (musicindex ADR 0020 R-WORD-1). For example, for `kind:single` a client
    sends one request with `kind=single`, and one with `kind=unknown&tracks=1`.
    Musicindex owns this mapping.

### The Contract

26. `/openapi.json` declares each parameter of items 9, 15 and 19 on
    `GET /v1/search`. Each description names the types that apply it.
    `/openapi.json` declares the error body of item 21 as a schema.
27. `GET /v1/node/capabilities` gives these names:

    | Name | The addition |
    |---|---|
    | `publisher` in `entity_types` | Item 8 |
    | `search_filters` | A map from each type to the names of its filters. The handler and the capabilities read one table |
    | `search_medium` | Items 9 and 23 |
    | `search_feed_summary` | Item 24 |

    A client sends `medium` only when the node lists `search_medium`. A node
    with no such flag gives `400` for the unknown parameter.
28. `tests/fixtures/v1-schema-fields.json` holds each property and each enum
    value of this ADR (ADR 0044).

## Consequences

- One current ADR holds the rules of the search. A reader does not merge five
  ADRs and three sections of others to find one rule.
- The code and the tests keep their citations of the superseded ADRs. The item
  map gives the item of each citation.

## Invariants

- No character of `q` or of a filter value is an FTS5 operator.
- A delete of an index row removes each of its old words.
- A retired feed has no index row, and none of its tracks has one.
- A search with no `medium` gives no playlist, and a search with no
  `type=publisher` gives no publisher feed.
- In one group, a hit with the whole word comes before each hit by prefix only.
- A filter never changes `rank` or `effective_rank`.
- A filter and an exclusion match whole words.
- An unknown parameter gives `400`, never rows that it did not reduce.
- The node infers no release kind.

## How This Decision Is Checked

### Mechanical

These tests prove the items. Each file keeps the name of the ADR that added it.

| Items | Tests |
|---|---|
| 1 to 3, 5, 6 | `tests/adr0078_search_index_tests.rs` |
| 4 | `tests/adr0078_retire_index_tests.rs` |
| 7, 8 | `tests/adr0071_publisher_search_tests.rs`, `tests/adr0094_playlist_index_tests.rs` |
| 9 | `tests/adr0094_search_medium_tests.rs` |
| 10, 12 | `tests/adr0078_query_words_tests.rs`, `tests/adr0080_phrase_tests.rs` |
| 11 | `tests/adr0086_prefix_match_tests.rs` |
| 13, 14 | `tests/adr0086_prefix_match_tests.rs`, `tests/adr0087_match_group_tests.rs` |
| 15 to 19, 21 | `tests/adr0080_text_filter_tests.rs`, `tests/adr0080_value_filter_tests.rs`, `tests/adr0085_name_filter_tests.rs` |
| 20 | `tests/adr0080_host_filter_tests.rs` |
| 22 | `tests/adr0080_no_q_search_tests.rs` |
| 23, 24 | `tests/adr0094_feed_hit_summary_tests.rs` |
| 26 to 28 | The guards of ADR 0044, and the capability tests of the files above |

### Manual

Item 25 is a rule for the apps. Musicindex checks it.

### Visual

None in this repository. The clients own their pickers and their live searches.

## Item Map

| Old | New |
|---|---|
| ADR 0071 §5 | Items 7 and 8 |
| ADR 0078 §1 | Item 1 |
| ADR 0078 §2 | Items 2 and 3 |
| ADR 0078 §3 | Item 4 |
| ADR 0078 §4 | Item 5 |
| ADR 0078 §5 | Items 10 and 12 |
| ADR 0078 §6 | Item 6 |
| ADR 0080 §1 | Item 10 |
| ADR 0080 §2 | Items 15 to 18 and 25 |
| ADR 0080 §3 | Items 19 and 21 |
| ADR 0080 §4 | Item 21 |
| ADR 0080 §5 | Item 20 |
| ADR 0080 §6 | Item 22 |
| ADR 0080 §7 | Items 26 to 28 |
| ADR 0085 §4 | Items 15 to 17 |
| ADR 0085 §5 | Item 19 |
| ADR 0085 §6 | Item 25 |
| ADR 0086 §1 | Item 11 |
| ADR 0086 §2 | Items 13 and 14 |
| ADR 0086 §3 | Items 16 and 19 |
| ADR 0086 §4, ADR 0087 §4 | Item 1. The index has no `prefix` option |
| ADR 0087 §1 | Item 13 |
| ADR 0087 §2 | Item 14 |
| ADR 0087 §3 | Item 13 |
| ADR 0094 §1 | Item 9 |
| ADR 0094 §1a | Item 7 |
| ADR 0094 §2 | Item 23 |
| ADR 0094 §3 | Item 24 |
| ADR 0094 §4 | Item 27 |
