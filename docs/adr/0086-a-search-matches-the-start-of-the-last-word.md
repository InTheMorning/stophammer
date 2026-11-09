# ADR 0086: A Search Matches The Start Of The Last Word

## Status
Accepted on 2026-11-07 by the operator, for the prefix match of
[the research request of 2026-10-23](../plans/research-search-index-request.md).
The build is complete. The primary runs this work in the candidate `v0.19.0-rc.7`, and no release carries it. It adds to ADR 0080 §1 and ADR 0078 §5,
which give no prefix match. ADR 0080 §2 does not change.
§2, the invariants and the consequence about a `rank` above 0 amended on 2026-11-07
by ADR 0087: the groups of that ADR give the order, and `rank` has no offset.

## Date
2026-11-07

## Context
A search with `q` matches whole words only. On 2026-11-06, `q=jsmo` gave 0 rows,
and `q=jsmovie` gave rows. The pickers of musicindex-studio send `q`, so a picker
finds an artist only after the last letter of a word. The live searches of v4vmm
(v4vmm ADR 0087 D4) and of the website have the same limit. The operator put this
change before the fast routes for a sort.

The request asks for two items: a prefix index with `prefix='2 3'`, and a prefix
match on the last word of `q`.

A prefix match also adds rows to a search for a complete word. In the backup of
2026-11-07, `love` has 1,182 hits, and `love*` has 1,375. BM25 prefers short text,
so `Lover` can come before `Love Song`. A picker must show the row with the exact
word first.

The website sorts each page again by `rank` (`search.html:5154` in the website).
The response gives `rank` and no `effective_rank`. So a client can see the order
only through `rank`.

## Decision

### 1. The last word of `q` matches as a prefix
The node reads `q` as ADR 0080 §1 states. If the last item of `q` is a word, FTS5
reads that word as the start of a word. Each other word and phrase stays whole.

| `q` | Matches |
|---|---|
| `jsmo` | A row with a word that starts with `jsmo` |
| `night dr` | A row with the word `night`, and a word that starts with `dr` |
| `"night dr"` | A row with the words `night dr` in sequence, as whole words |
| `jsmo ""` | The same rows as `jsmo`, because `""` adds nothing (ADR 0080 §1) |
| `"jsmo` | The same rows as `jsmo`, because the double quote is part of the word |

- Text in double quotes never matches as a prefix. A client sends `"love"` when it
  wants only the whole word.
- White space after the last word does not change the match.
- The rule has no minimum length. So each new letter of a live search can only
  remove rows.
- No character of `q` is an FTS5 operator. A `*` in `q` stays part of its word.

### 2. A hit with the whole word comes first
The node runs two matches: the exact query, and the prefix query for the other
rows.

- A hit of the exact query keeps its `rank` of the release before this ADR, and its
  sequence.
- Each other hit has `rank` equal to its BM25 score of the prefix query, plus
  1,000,000. Thus each hit by prefix only comes after each hit with the whole word,
  also when a client sorts by `rank`.
- The quality weight of ADR 0022 applies to the score, and not to the 1,000,000.

`rank` stays one number, and a lower number stays a better match. The cursor keeps
its form, so a cursor of the release before this ADR stays valid. `sort=name` and
`sort=date` read the same hits, with the same `rank` (ADR 0084 §10).

### 3. A filter and an exclusion stay whole words
`artist`, `title`, `release` and each `not_` parameter keep the rule of ADR 0080
§2. `artist=jsmo` does not match `jSmovie`. A prefix on an exclusion removes rows
that the person did not name, and no client asks for a prefix on a filter.

### 4. The index does not change
`search_index` gets no `prefix` option. FTS5 reads a prefix from its usual word
index, and the evidence shows that the option has almost no effect on the time.
This change needs no migration, no repair and no rebuild.

### What this ADR does not decide
- A minimum length of the last word. See the consequences.
- A prefix on a filter.
- The other changes of the research request.

## Evidence
The backup of 2026-11-07 holds 39,273 index rows. The measurements use the bundled
SQLite 3.45.0, on a local copy of the index, with a warm cache. Each time is the best
of 5 runs of a request with no `type`, so the node runs the query for feeds and for
tracks. They are not a load test.

| `q` | Hits of the whole word | All hits | Time today | Time with this ADR |
|---|---|---|---|---|
| `s` | 2,809 | 18,382 | 9.1 ms | 89.3 ms |
| `a` | 4,931 | 16,091 | 13.3 ms | 72.2 ms |
| `th` | 1 | 11,447 | 0.2 ms | 40.2 ms |
| `the` | 9,306 | 10,118 | 26.1 ms | 38.9 ms |
| `the l` | 87 | 4,494 | 1.3 ms | 29.4 ms |
| `lo` | 147 | 3,868 | 0.8 ms | 15.5 ms |
| `love` | 1,182 | 1,375 | 4.7 ms | 6.8 ms |
| `jsmo` | 0 | 35 | 0.1 ms | 0.3 ms |

One prefix query on the two index forms, with the same row selection:

| Fact | No `prefix` option | `prefix='2 3'` |
|---|---|---|
| Size of the index, with no copy of the text | 7.9 MB | 18.7 MB |
| `the*` | 25.3 ms | 22.8 ms |
| `th*` | 29.9 ms | 25.8 ms |
| `lo*` | 10.6 ms | 9.6 ms |
| `love*` | 4.1 ms | 4.2 ms |
| `a*` | 46.8 ms | 42.4 ms |

The cost is the score and the sort of each hit, and not the search for the words.
The option cannot remove that cost.

The lowest BM25 score over 4,000 one-word queries of the backup is −18.7. The quality
weight at most doubles a score.

## Alternatives Considered

### A prefix index
This is the form of the request. It adds 10.8 MB and one rebuild at the first open.
It saves at most approximately 4 ms on one query in the table above.

### A parameter that a client sends for a prefix match
The rows of a search stay the same with no new parameter. Each of the three clients
must then change before a person sees a result. §2 keeps each earlier hit and its
sequence at the top of the list, so a client that wants only the whole word has one
loss: more rows after the last exact row.

### One prefix query with no second match
This costs less, because it scores each hit one time. A hit by prefix can then come
before a hit with the whole word, and each `rank` of a search changes.

### A tier field in the cursor and on the row
This also gives §2. It needs a new cursor form, and the merge of a request with no
`type` needs a second key. A client that sorts by `rank` still mixes the two groups.

### A minimum length of two letters
This removes the worst times of the table. A live search then gives the rows of the
whole word `s`, and then more rows for `sm`, so a new letter can add rows. A later
change can add the minimum if a measurement on the primary shows a need.

## Consequences
- `q=jsmo` finds `jSmovie` on each client, with no client change.
- A search for a complete word can give more rows than before, after the rows with
  the whole word. `has_more` can be true where it was false.
- A `rank` can be larger than 0. A client must not show `rank` as a score, or assume
  that it is negative. No client code that this ADR examined reads its sign. The
  website only sorts by it.
- A last word with no letter or digit adds no prefix. `jsmo -` matches `jsmo` as a
  whole word.
- A no-`type` search for one letter can take approximately 90 ms in place of 13 ms.
  The rate limit of `AGENTS.md` stays.
- No event payload changes, so a primary and a community node can upgrade in each
  sequence. The change is an addition, so it belongs in a MINOR release (ADR 0066).
- `docs/API.md` and the `q` description in `src/openapi.rs` change in the same commit
  as the code. `api.html` reads the OpenAPI document, so it needs no change.

## Invariants
- A hit with the whole word comes before each hit by prefix only.
- A hit with the whole word has the `rank` of the release before this ADR.
- A filter never changes `rank` (ADR 0080 §2).

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0086_prefix_match_tests.rs` prove each of these:

1. §1: `fts5_prefix_query` gives the prefix form for a last word, and gives `None`
   for text in double quotes and for no word.
2. §1: `q=jsmo` gives a row with `jSmovie`. `q=jsmo dr` needs `jsmo` as a whole word.
   `q="jsmo"` gives no row with `jSmovie`.
3. §1: a `q` with a last word of `-`, `*`, `'`, `o'bri`, `jsmo*` or `a"` gives `200`.
4. §2: for `q=love`, each row with the whole word comes before each other row, also
   when the other row has shorter text.
5. §2: a row with the whole word has the same `rank` with `q=love` and with
   `q="love"`.
6. §2: a higher quality score puts a row first among the hits by prefix only, and
   never before a hit with the whole word.
7. §2: the pages of a search with limit 1 and no `type` give each row one time, in
   the sequence of one large page.
8. §2: `sort=name` gives the hits by prefix, with the same `rank` as the default sort.
9. §3: `artist=jsmo` gives no row with the artist `jSmovie`, and `not_q=jsmo` does
   not remove that row.

### Visual
None in this repository. The clients own their pickers and their live searches.
