# ADR 0099: The Lists And Their Rows

## Status
Accepted

- Accepted: 2026-11-14. The operator accepted the consolidation of each
  changed decision (musicindex-research `df7b823`). This ADR makes no new
  decision. It restates each live rule of the ADRs that it supersedes, with
  each change from a subsequent ADR.
- Supersedes: [ADR 0068](archive/0068-a-publisher-row-gives-its-link-facts.md),
  A Publisher Row Gives Its Link Facts.
- Supersedes: [ADR 0070](archive/0070-a-name-list-gives-each-identity-one-row.md),
  A Name List Gives Each Identity One Row.
- Supersedes: [ADR 0071](archive/0071-a-list-row-links-what-it-names.md),
  A List Row Links What It Names. ADR 0100 restates its §5.
- Supersedes: [ADR 0084](archive/0084-a-search-and-a-list-take-a-sort.md),
  A Search And A List Take A Sort.
- Supersedes: [ADR 0085](archive/0085-a-name-list-takes-the-role-filters.md),
  A Name List Takes The Role Filters, And A Track Title Filter Reads Its Feed.
  ADR 0100 restates its §4 to §6.
- Supersedes: [ADR 0089](archive/0089-an-artist-list-takes-newest-first.md),
  An Artist List Takes Newest First.
- Supersedes: [ADR 0090](archive/0090-a-name-sorts-by-a-folded-key.md),
  A Name Sorts By A Folded Key.
- Supersedes: [ADR 0091](archive/0091-an-artist-list-takes-the-role-filters.md),
  An Artist List Takes The Role Filters.
- Supersedes: [ADR 0092](archive/0092-an-artist-row-counts-its-releases-by-track-count.md),
  An Artist Row Counts Its Releases By Track Count.
- Restates: the change of ADR 0091 §2 to ADR 0059 §1, in item 49.
- Restates: the change of ADR 0092 to ADR 0076 §3, in item 14.
- Revised: 2026-11-14. Item 18: the map and the count of a row come from the
  same rows, also in the copy in memory of `GET /v1/artist-names`.
- Revised: 2026-11-14. Item 36 and its invariant: a page reads no row of the
  list. It reads the highest `seq` of the event log, to find a stale copy.
- Uses: [ADR 0049](archive/0049-publisher-relationships-are-rss-facts.md) for
  the `publisher` view, the role tokens (§6), the confirmed links (§6a) and the
  artist count (§7). [ADR 0061](0061-a-publisher-read-counts-its-listed-artists.md)
  for `confirmed_release_artists`.
  [ADR 0069](0069-an-album-confirms-each-credit.md) §3a for `album_names_rank`.
  [ADR 0076](0076-a-named-feed-entry-states-its-confirmation.md) for
  `two_way_validated` on a named feed entry, the filter `named_feed_guid` and
  the link facts of a search row.
  [ADR 0100](0100-the-search.md) for the search index, `q`, the filters and the
  rank. ADR 0044 §4 for a `v1` addition.
- Changed by: [ADR 0102](0102-a-publisher-view-shows-its-playlists.md) §3
  and §7, for items 14 and 51. Accepted on 2026-11-17.

## Date
2026-11-14

## Context

- Nine ADRs from 2026-10-01 to 2026-11-08 made the list rules: the release
  lists, the artist list, the link facts, the sort and the name keys. Each
  subsequent ADR changed some items of an earlier ADR. On 2026-11-14 the shared
  ADR link guard found four ADRs of this area with more than three changes.
- The global decision of 2026-11-14 (musicindex-research `df7b823`) sets one
  current ADR for an area after three changes. This ADR is that ADR for the
  lists. ADR 0100 is that ADR for the search.
- ADR 0084 gives the sort of the lists and of the search. This ADR keeps the
  full sort rule, because one rule orders the four routes.
- The superseded ADRs keep their context, their evidence, their alternatives
  and their checks in `docs/adr/archive/`. The item map at the end of this ADR
  gives the new item of each old section. A citation in code or in a test, for
  example "ADR 0084 §9", resolves through that map.

## Decision

### Row Rules

1. A field that gives the title of another feed has a field in the same object
   that gives the GUID of that feed. A field `<prefix>feed_title` has the field
   `<prefix>feed_guid`. The guard of this rule holds the list of each pair with
   other names, for example `remote_feed_title` and `resolved_feed_guid` of a
   copy entry.
2. A field that counts items has a route or a filter that lists the same items.
   The decision that adds the count names that list. A review checks this rule,
   because the contract does not record which route lists a count.
3. The OpenAPI document declares each `include` name that a route acts on. The
   node ignores an unknown name, so a client that sends an old name gets no
   error.
4. Items 1 to 3 apply to each `v1` response. A new route follows them in its
   design.
5. The node gives facts, and the client derives a type. The node does not label
   a publisher feed as an artist or as a label (ADR 0061 §4). It does not
   decide that a name and a publisher feed are the same artist, and it does not
   split a credit such as "feat.". It infers no release kind (ADR 0100).
6. Each route, field, include, filter and parameter of this ADR is a `v1`
   addition (ADR 0044 §4). Each route is in the OpenAPI document with the
   schema of its row type.

### The Name Keys

7. The identity key, `model::sort_key(text)`, is the text after the
   normalization of ADR 0049 §7:
   1. Remove the white space at the start and at the end.
   2. Replace each internal run of white space with one space.
   3. Change each character to Unicode lowercase.

   The identity key keeps a leading article such as "The", the punctuation and
   the accents.
8. The identity key makes these values: the `key` of a `name` row, the value
   of `artist_key`, the release groups of the artist list, and the artist count
   of ADR 0049 §7. The identity key does not depend on the order key.
9. The order key, `model::order_key(text)`, makes the key of each name
   sequence. It starts with the identity key and takes four steps:
   1. **Fold.** The key takes the NFKD decomposition. Then it removes each
      combining mark, so `ë` and `é` become `e`. A table changes the letters
      that have no decomposition: `ø` to `o`, `æ` to `ae`, `œ` to `oe`, `ß` to
      `ss`, `ł` to `l`, `đ` and `ð` to `d`, `þ` to `th` and `ı` to `i`.
   2. **Numbers.** Each run of ASCII digits gets zeros at its start, to 20
      digits. A longer run does not change. So `2` comes before `10`. NFKD
      changes a full-width digit into an ASCII digit first.
   3. **Group.** When the first character is not a letter from `a` to `z`, the
      key starts with `#`. Digits, symbols, emoji and other scripts are in this
      group. The group comes before `a`.
   4. The key keeps the leading article and the punctuation.

   An empty text gives an empty key, so an empty `from` names no position.
10. Two order keys compare by Unicode code point. This is the byte order of
    UTF-8 and the order of the SQLite `BINARY` collation. In the `#` group, the
    folded text gives the sequence.
11. `tests/fixtures/order-key-vectors.json` gives each input with its
    `sort_key` and its `order_key`. The vectors cover accents, the table of item
    9, digit runs, the `#` group, emoji, other scripts and the empty text. A
    test checks `model::sort_key` and `model::order_key` against each vector.
    A client that merges its own rows with the rows of the node makes the same
    key. musicindex-search holds the one client copy of the order key, and it checks
    that copy against this file. A change to the file needs an ADR, because each
    copy then changes.
12. `feeds.title_sort` and `tracks.title_sort` hold the order key of the title.
    - Each write of a feed or a track makes it. A Rust fill in `open_db` sets
      each null value. Migration 0063 sets each stored value to null, so that
      the fill writes the order key into each row that held the identity key.
    - `feeds.title_sort` has an index. `tracks.title_sort` has no index,
      because the search sorts after a join. An index does not change the API,
      so a subsequent change can add one for a slow query.
    - `title_lower` is not a name key, and it does not change. The search index
      and the event log do not hold a name key.
13. The crate `unicode-normalization` 0.1 gives the NFKD decomposition and the
    test for a combining mark. It brings `tinyvec` and `tinyvec_macros`. Their
    licenses are `(MIT OR Apache-2.0) AND Unicode-3.0` and
    `Zlib OR Apache-2.0 OR MIT`, and `deny.toml` allows each of them. `cargo deny`
    in CI checks the three crates.

### The Link Facts

14. The include name `link_facts` gives these fields on each row of a feed with
    the medium `publisher`. Each field counts only the confirmed links (ADR 0049
    §6a):

    | Field | Value |
    |---|---|
    | `two_way_link_count` | The count of the confirmed links |
    | `stated_rels` | The different role tokens of `publisher_rel` over the confirmed links, normalized as ADR 0049 §6 gives, and sorted |
    | `agreed_roles` | The different role tokens of the confirmed links with `role_agreement` `both`, sorted |
    | `confirmed_release_artists` | The value of ADR 0061 §1, over the confirmed links |
    | `release_track_counts` | Item 17 |

    `GET /v1/feeds/recent`, `GET /v1/feeds/by-name` and `GET /v1/artist-names`
    take the include. ADR 0076 §3 gives it on a `publisher` row of
    `GET /v1/search`.
15. With `include=link_facts`, each `publisher` row of `GET /v1/artist-names`
    also gives these fields:

    | Field | Value |
    |---|---|
    | `first_named_count` | The count of the confirmed links of `two_way_link_count` whose `album_names_rank` is 1 (ADR 0069 §3a) |
    | `release_artist`, `release_artist_source` | The values of `GET /v1/feeds/recent` for the same feed. Each can be null |

    `first_named_count` is a fallback. A client uses it only for a feed whose
    `agreed_roles` is empty. The node derives no artist from it.
16. Each value of items 14 and 15 is the value that
    `GET /v1/feeds/{guid}?include=publisher` gives for the same feed at the same
    time. The node computes it with the functions of the `publisher` view. A
    row of another medium gives none of the fields. Without the include, no row
    gives them, and the read costs the same as a read with no include. The
    node derives the facts at read time and does not store them. It computes
    them only for the `publisher` rows of the page, except in the build of the
    artist list (item 37).
17. `release_track_counts` is an object. Its key is a track count, as text. Its
    value is the number of releases with that track count. A track count with
    no release is absent. A row with no release gives `{}`.

    | Row | The releases | The sum of the values |
    |---|---|---|
    | A row with `two_way_link_count` | The confirmed links of `two_way_link_count` | `two_way_link_count` |
    | A `name` row of `GET /v1/artist-names` | The releases of `release_count`, the set of `artist_key` | `release_count` |

    A row with `two_way_link_count` gives the field only with
    `include=link_facts`. A `name` row gives it with no include, as it gives
    `release_count`.
18. The track count of a release is `feeds.episode_count`, the column that the
    filters `tracks`, `min_tracks` and `max_tracks` read (ADR 0100). On a
    confirmed link, the value is the `remote_episode_count` of the row (ADR 0059
    §1). The map of a row with `two_way_link_count` comes from the same rows as
    that count. On `GET /v1/artist-names`, the copy in memory holds the map and
    the count from the same rows. So the two agree, and each is at most 30
    seconds old (item 36).

### The Release Lists

19. `GET /v1/feeds/recent` gives the feeds newest first, by `newest_item_at`.
20. `GET /v1/feeds/by-name` gives the feeds of one medium in the order of
    `sort_name`, then of `feed_guid`. The default medium is `music`. Each row
    gives the fields of a row of `GET /v1/feeds/recent`, and `sort_name`. The
    route takes `limit` and `cursor` as the recent list does.
21. `GET /v1/feeds/by-name` and `GET /v1/artist-names` take `from`. The list
    then starts at the first row whose order key is equal to or after the order
    key of `from`. `from=O` starts at the first key at or after `o`, so
    "Øystein" is in its place among the `O` names. `from=#` starts the list at
    the `#` group. Each other `from` text that does not start with a letter is a
    position in the `#` group. A request with `cursor` ignores `from`.
22. Each row of the two release lists gives `publisher_feed_guid` beside
    `publisher_feed_title` (item 1). It is the GUID of the feed that gives the
    title, and it is null when the title is null.
23. The two release lists take `include=named_feeds`. With it, each row of a
    music feed gives `named_feeds`. It holds one entry for each
    `music_to_publisher` row of the `publisher` view of that feed, in the
    sequence of `album_names_rank`. Each entry gives the values of that row:

    | Field | Value |
    |---|---|
    | `publisher_feed_guid` | The GUID that the item names |
    | `remote_feed_title` | The title of that feed. Null when the node does not hold it |
    | `album_names_rank` | ADR 0069 §3a |
    | `role` | ADR 0049 §6 |
    | `role_agreement` | ADR 0049 §6a |
    | `two_way_validated` | ADR 0076 §1 |

    The list is empty when the album names no publisher feed. A row of another
    medium does not give `named_feeds`.
24. With `medium=publisher`, the two release lists take two filters:

    | Parameter | Rows that it keeps |
    |---|---|
    | `stated_rel=<value>` | Rows with the normalized `<value>` among the tokens of `stated_rels` |
    | `two_way_links=none` | Rows with `two_way_link_count` 0 |

    A raw value with a comma or with white space does not match as one string.
    Each filter gives the link facts on each row, as `include=link_facts` does.
    The two filters together keep the rows that agree with both. A filter with
    another medium, an empty `stated_rel`, or a `two_way_links` value that is
    not `none` gives `400`.
25. A filtered request examines the rows in the sequence of the route, and it
    examines at most 1,000 rows. At that limit, it gives the rows that it kept,
    and a cursor at the last row that it examined. So a page can hold fewer rows
    than `limit`, or none, and give `has_more` true. A client follows the cursor
    until `has_more` is false. The filter compares facts that a row gives, and
    it derives no type.
26. On `GET /v1/feeds/by-name`, the filters keep the sequence, `from`, the cursor
    and `reverse` of the route. The sequence of a filtered list is the sequence
    of the same list with no filter. A row of a filtered name list has the same
    link facts as the same row of `GET /v1/feeds/recent` with the same filter.
    A client sends the filters on this route only when the capabilities list
    `sort` (item 51). `GET /v1/feeds/recent` takes no name order.
27. The two release lists take `artist_key`. With it, a route gives only the
    music releases that the `name` row with that `key` counts in
    `release_count`. Each route reads that set from the copy in memory (item
    36), so the count and the list come from one build. The sequence, `limit`,
    `cursor` and `reverse` do not change. A key with no `name` row gives an
    empty list. `artist_key` with a `medium` other than `music`, or with
    `named_feed_guid`, gives `400`.
28. ADR 0076 §2 gives the filter `named_feed_guid` on the two release lists.

### The Artist List

29. `GET /v1/artist-names` gives one row for each artist identity of v4vmm ADR
    0084 Decision 7, in the order of `sort_name`, then `kind`, then `key`. Each
    row gives `kind`:

    | `kind` | One row for each | `name` | `key` |
    |---|---|---|---|
    | `publisher` | Feed with the medium `publisher` | The feed title | The feed GUID |
    | `name` | Identity key of the `release_artist` of a release of the name level (item 33) | The raw `release_artist` that the most releases of the row state. With a tie, the first raw value in code point order | The identity key |

30. Each row also gives these fields:

    | Field | Value |
    |---|---|
    | `sort_name` | The order key of the feed title for a `publisher` row, and of the `key` for a `name` row |
    | `feed_guid` | The feed GUID for a `publisher` row. Null for a `name` row |
    | `image_url` | The image of the publisher feed (ADR 0054 §4). Null for a `name` row |
    | `release_count` | The number of music releases of the name level with that key, for a `name` row. Null for a `publisher` row |
    | `newest_release_at` | Item 39 |
    | `release_track_counts` | Item 17, on a `name` row |

31. The node merges no `publisher` row and no `name` row with the same text. The
    same text can thus show in two rows. A `name` row is a fact about the text
    of the releases, and the response does not state that it is an artist.
32. The route takes `limit`, `cursor` and `from` as the release lists do.
33. A music release is of the name level when one of these conditions is true:
    - No item inside its `<podcast:publisher>` names a publisher feed that the
      node holds.
    - Each such link has `role_agreement` `both`, and its agreed role set does
      not include `artist` (ADR 0049 §6a).
34. A release with a link that is not confirmed stays out of the name level.
    This is also true when its first named feed has an agreed role without
    `artist`. The rule uses the `rel` facts, not the sequence of the named
    feeds.
35. The name level uses the link facts of the `publisher` view. The node has no
    second rule for it.
36. The node builds the full artist list with items 29 to 35 and keeps it in
    memory. A page reads the copy in memory and no row of the list. It reads
    only the highest `seq` of the event log, to find a stale copy.
    - The copy records the highest `seq` of the event log at its build.
    - When the highest `seq` changes, the copy is stale. The next request builds
      the list again.
    - The node builds the list at most one time in each 30 seconds. During a
      burst of changes, a page can thus give a list that is at most 30 seconds
      old.
    - Only one build runs at a time. A request during a build waits for it.

    The node stores no derived table for the list.
37. The build computes the link facts of items 14 and 15 for each `publisher`
    row, with the functions of the `publisher` view. It also computes the dates
    of item 39 and the two sequences of item 40.
38. `GET /v1/artist-names` takes `stated_rel` and `two_way_links`, with the
    values of item 24. `stated_rel` keeps the rows with that role token in
    `stated_rels`, and `two_way_links=none` keeps the rows with
    `two_way_link_count` 0. An empty `stated_rel`, two role tokens, or another
    `two_way_links` value gives `400`. With a filter, the list gives only the
    `publisher` rows that match, so it gives no `name` row. The set of these
    rows is the set of `GET /v1/feeds/recent?medium=publisher` with the same
    filter. The filters work with `sort`, `reverse`, `from` and the cursor, and
    the cursor format does not change. A filtered page reads the rows of the
    sequence and skips each row that does not match.
39. `newest_release_at` is the newest `newest_item_at` of the releases of the
    row. For a `name` row, these are the releases of `artist_key`. For a
    `publisher` row, these are the music releases that name the feed, the set of
    `named_feed_guid` (ADR 0076 §2). The value is null when no such release has
    a date.
40. `GET /v1/artist-names` takes `sort`, with the value `name`, the default, or
    `date`. The build computes the two sequences.
    - `date` gives the newest `newest_release_at` first. The tie key is
      `sort_name`, `kind` and `key`, up.
    - A row with no date comes last in each direction.
    - With `reverse=true`, the oldest date comes first, and the tie key goes
      down.
    - `sort=date` with `from` gives `400`, because `from` names a letter.
      Another `sort` value gives `400`.
    - A cursor of `sort=date` works only with `sort=date`, and a cursor of
      `sort=name` only with `sort=name` (item 47).

### The Sort And The Direction

41. A filter selects the rows, and a sort puts them in a sequence. `q` and the
    filters decide which rows match. `sort` and `reverse` decide only the
    sequence. A filter does not change the rank of a row (ADR 0100).
42. `GET /v1/search` takes `sort`:

    | `sort` | Usual sequence | Key |
    |---|---|---|
    | `relevance` | Best match first | `effective_rank` (ADR 0100) |
    | `name` | A to Z | The order key |
    | `date` | Newest first | `newest_item_at` of a feed or a publisher, `pub_date` of a track |

    Each type of row uses the key of its own record. A feed and a publisher use
    `feeds.title_sort`, and a track uses `tracks.title_sort`. A request with no
    `type` merges feeds and tracks by the key. `relevance` needs a word in `q`.
43. `reverse=true` reverses the usual sequence:

    | Route | Usual sequence | With `reverse=true` |
    |---|---|---|
    | `GET /v1/feeds/recent` | Newest first | Oldest first |
    | `GET /v1/feeds/by-name` | A to Z | Z to A |
    | `GET /v1/artist-names` | A to Z, or newest first with `sort=date` | Z to A, or oldest first |
    | `GET /v1/search`, `sort=date` | Newest first | Oldest first |
    | `GET /v1/search`, `sort=name` | A to Z | Z to A |

    Oldest first uses the same key as newest first. `sort=relevance` does not
    reverse, and a client shows no reverse icon on it. The filters, the scan limit of item 25 and the fields of a row do
    not change with `reverse`.
44. A request with neither `sort` nor `reverse` gives the sequence and the
    response of the release before the sort. A search with a word in `q` has the
    default sort `relevance`. A search with no word has the default sort `date`.
45. Each search error of the sort uses the error body of ADR 0100. The checks of
    ADR 0100 run first, so `missing_positive_term` stays the first result of a
    request with no word and no positive filter.

    | Code | Condition |
    |---|---|
    | `unsupported_sort` | `sort` has a value that item 42 does not give, or `reverse` is not `true` or `false`. `filter` names the parameter |
    | `relevance_needs_word` | `sort=relevance`, and `q` has no word |
    | `relevance_cannot_reverse` | `reverse=true`, and the sort is `relevance`, also by default |

    `reverse` accepts only `true` and `false`. Each other value gives `400` on
    each route.
46. A row with no key comes after each row with a key, in the two directions,
    on `GET /v1/feeds/recent`, on `GET /v1/artist-names` and on search. So
    oldest first is not an exact mirror of newest first. The tie key follows the
    direction: `feed_guid` on the two release lists, `sort_name`, `kind` and
    `key` on the artist list, and `rowid` on search.
47. A cursor names the sort, the direction, the key and the tie key of its last
    row. A cursor with a different sort or direction than the request gives
    `400`. A cursor from before the sort has the default sort and the usual
    direction, so it stays valid. A cursor is valid only with the parameters of
    the request that gave it.
48. With `reverse=true`, `from` starts the list at the last row whose order key
    starts with the order key of `from`. When no key has that start, the list
    starts at the last row before that position. `from=#` gives the `#` group
    from its last row.

### The Publisher Rows Of A Feed Read

49. Each `include=publisher` row of `GET /v1/feeds/{guid}` gives
    `newest_item_at`: the `newest_item_at` of the named feed, from the same read
    as `remote_feed_title` (ADR 0059 §1). It is null when the node holds no feed
    for the entry, or when the feed has no dated item. On a
    `publisher_to_music` row it is the date of the release. On a
    `music_to_publisher` row it is mostly null, because most publisher feeds
    have no dated item.
50. With `sort=name`, each search row gives `sort_name`. With `sort=date`, each
    search row gives `sort_date`, and the value is absent on a row with no date.
    A search with no `sort` does not give these fields.

### The Capabilities

51. `GET /v1/node/capabilities` names each addition of this ADR:

    | Name | Where | The addition |
    |---|---|---|
    | `link_facts` | `include_params` of the list routes | Items 14 to 18 |
    | `named_feeds` | `include_params` of the two release lists | Item 23 |
    | `name_lists` | `capabilities` | `GET /v1/feeds/by-name` and `GET /v1/artist-names` |
    | `sort` | `capabilities` | Items 41 to 48 and 50, and the filters of item 26 |
    | `artist_newest` | `capabilities` | `artist_key` on `GET /v1/feeds/recent`, and items 39 and 40 |
    | `artist_role_filter` | `capabilities` | Item 38 |
    | `artist_row_release_artist` | `capabilities` | `release_artist` and `release_artist_source` of item 15 |
    | `publisher_row_date` | `capabilities` | Item 49 |
    | `release_track_counts` | `capabilities` | Items 17 and 18 |

    A client with no `release_track_counts` flag keeps the word "releases".
52. A list route ignores a parameter that it does not know. So a node with no
    such addition gives the usual list, with no error. A client sends a
    parameter of this ADR only when the node lists its name.

## Consequences

- One current ADR holds the rules of the lists. A reader does not merge nine
  ADRs to find one rule.
- The code and the tests keep their citations of the superseded ADRs. The item
  map gives the item of each citation.

## Invariants

- A list row gives the same value as a read of the same feed at the same time.
- A row of a feed that is not a publisher feed gives no link fact.
- A cursor continues a list with no gap and no repeat when no write occurs.
- The artist list merges no `publisher` row and no `name` row by text.
- The artist list in memory is at most 30 seconds older than the last change of
  the event log.
- A page of `GET /v1/artist-names` reads no row of the list. It reads only the
  highest `seq` of the event log.
- With a filter, `GET /v1/artist-names` gives no `name` row, and its set equals
  the set of `GET /v1/feeds/recent?medium=publisher` with the same filter.
- Each key of the `#` group comes before each key that starts with a letter. A
  run of digits sorts by its value, up to 20 digits.
- A row with no sort key comes last in each direction.
- The sum of `release_track_counts` is the total of its row.
- The node infers no release kind.

## How This Decision Is Checked

### Mechanical

These tests prove the items. Each file keeps the name of the ADR that added it.

| Items | Tests |
|---|---|
| 1, 3, 22 | `tests/adr0071_row_link_guard_tests.rs` |
| 7 to 12 | Unit tests of `order_key` in `src/model.rs`, `tests/adr0090_order_key_tests.rs` |
| 14 to 16 | `tests/adr0068_link_facts_tests.rs`, `tests/adr0071_artist_list_link_facts_tests.rs`, `tests/adr0091_role_filters_and_row_date_tests.rs` |
| 17, 18 | `tests/adr0092_release_track_counts_tests.rs` |
| 19 to 21 | `tests/adr0070_release_list_tests.rs`, `tests/adr0070_artist_list_tests.rs`, `tests/adr0090_order_key_tests.rs` |
| 23 | `tests/adr0071_named_feeds_tests.rs` |
| 24 to 26 | `tests/adr0068_link_fact_filter_tests.rs`, `tests/adr0085_name_filter_tests.rs` |
| 27 | `tests/adr0071_artist_key_tests.rs`, `tests/adr0089_artist_newest_tests.rs` |
| 29 to 37 | `tests/adr0070_artist_list_tests.rs`, `tests/adr0071_artist_list_link_facts_tests.rs` |
| 38 | `tests/adr0091_role_filters_and_row_date_tests.rs` |
| 39, 40 | `tests/adr0089_artist_newest_tests.rs` |
| 41 to 48, 50 | `tests/adr0084_sort_tests.rs`, `tests/adr0090_order_key_tests.rs` |
| 49 | `tests/adr0091_role_filters_and_row_date_tests.rs` |
| 51 | The capability tests of each file above |

The guards of ADR 0044 cover each parameter and field in the OpenAPI document
and in `tests/fixtures/v1-schema-fields.json`.

### Manual

Item 2: a review of each new count field. The contract does not record which
route lists a count, so a test cannot check it.

Item 13: `cargo deny` in CI checks the licenses of the three crates.

Item 39: for each row of the artist list on a copy of a backup,
`newest_release_at` must be the newest date of `db::get_feeds_naming_feed` for a
`publisher` row. A test cannot read that function on a copy of production data.
The check of 2026-11-08 found no difference in 2,090 rows.

### Visual

The v4vmm operator checks these:

- The v4vmm "Everything" scope shows the Library items and the Index items in
  one order.
- The v4vmm "Everything" scope shows a label feed apart from the artist feeds.
- The name level page of v4vmm shows its Index releases.

## Item Map

| Old | New |
|---|---|
| ADR 0068 §1 | Items 14 and 16. The raw values of §1 are role tokens since §5 |
| ADR 0068 §2 | Item 5 |
| ADR 0068 §3 | Item 16. The time check before the first deploy is in the archived ADR |
| ADR 0068 §4 | Items 24 and 25 |
| ADR 0068 §5 | Items 14 and 24 |
| ADR 0070 §1 | Items 7, 8, 10 and 12 |
| ADR 0070 §2 | Items 20 and 21 |
| ADR 0070 §3 | Items 29 to 32 |
| ADR 0070 §4 | Item 33 |
| ADR 0070 §4a | Item 36 |
| ADR 0070 §5 | Item 5 |
| ADR 0070 §6 | Items 6 and 51 |
| ADR 0071 §1, §1a to §1c | Items 1 to 4 |
| ADR 0071 §2 | Items 15 to 16 and 37 |
| ADR 0071 §3 | Items 22 and 23 |
| ADR 0071 §4 | Item 27 |
| ADR 0071 §5 | ADR 0100 |
| ADR 0071 §6 | Items 34 and 35 |
| ADR 0071 §7 | Items 6 and 51, and ADR 0100 item 27 for the search type |
| ADR 0084 §1 | Item 41 |
| ADR 0084 §2 | Item 42 |
| ADR 0084 §3 | Item 43 |
| ADR 0084 §4 | Item 44 |
| ADR 0084 §5 | Item 45 |
| ADR 0084 §6 | Item 46 |
| ADR 0084 §7 | Item 48 |
| ADR 0084 §8 | Item 12 |
| ADR 0084 §9 | Item 47 |
| ADR 0084 §10 | Item 50 |
| ADR 0084 §11 | Items 51 and 52 |
| ADR 0085 §1 | Items 24 and 26 |
| ADR 0085 §2 | Item 25 |
| ADR 0085 §3 | Item 26 |
| ADR 0085 §4 to §6 | ADR 0100 |
| ADR 0089 §1 | Item 27 |
| ADR 0089 §2 | Items 39 and 40 |
| ADR 0089 §3 | Item 51 |
| ADR 0090 §1 | Items 7 and 8 |
| ADR 0090 §2 | Items 9 and 10 |
| ADR 0090 §2a | Item 11 |
| ADR 0090 §3 | Items 12, 21, 30, 40 and 48 |
| ADR 0090 §4 | Item 12 |
| ADR 0090 §5 | Item 13 |
| ADR 0091 §1 | Item 38 |
| ADR 0091 §1a | Item 15 |
| ADR 0091 §2 | Item 49 |
| ADR 0091 §3 | Item 51 |
| ADR 0092 §1, §1a, §1b | Items 17 and 18 |
| ADR 0092 §2 | Item 51 |
