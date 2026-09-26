# ADR 0061: A Publisher Kind Is Derived And Says So

## Status
Proposed

## Date
2026-09-26

## Context
A client shows a publisher feed as an artist or as a label. The feed seldom
says which. On 2026-09-26 a sample of 60 of the 1,772 publisher feeds held
249 album links, and no side of any link stated `rel`. Thus ADR 0049 §6 gave
each link `role: "artist"` with `role_source: "default"`, and that value is an
assumption.

ADR 0049 §7 gives `distinct_release_artist_count`, the count of the distinct
`itunes:author` values of the albums that **name** the publisher. An album
that the publisher lists, but that does not name the publisher, is not in the
count. The publisher "Master's Scroll" lists 81 albums by 33 different
artists. No album names it. So the count is 0, and musicindex.org shows
"0 artists" on a publisher of 33 artists.

ADR 0059 gives each `publisher` entry `remote_release_artist` and
`remote_release_artist_source`, from the album that the entry resolves to. So
the node can read the artists of each **listed** album at read time.

A test of a rule on 2026-09-26, over 240 publisher feeds of the live API and
"Master's Scroll":

| Result | Publishers |
|---|---|
| Each album artist holds the publisher name | 238 |
| One album artist with a different name | 2, both test feeds: "MSP 2.0 test" and "null" |
| Two or more album artists with other names | 1, "Master's Scroll" |

A first version of the rule compared the names for equality. It classified
"Mishkin Fitzgerald" as a label, because her albums name "Mishkin Fitzgerald"
and "Birdeatsbaby (Mishkin Fitzgerald)". The rule below compares whole words,
and gives that publisher `artist`.

## Decision

### 1. The listed artists

A feed read of a publisher feed adds two fields:

| Field | Value |
|---|---|
| `listed_release_artists` | The distinct `release_artist` values of the listed albums that resolve, raw, in the order of first listing |
| `listed_release_artist_count` | The count of `listed_release_artists` |

A listed album is a row of the direction `publisher_to_music` that does not
resolve to `unresolved` (ADR 0049 §3). An album whose
`release_artist_source` is `placeholder` gives no value. Two values are the
same when they are equal after the normalization of ADR 0049 §7.

`distinct_release_artist_count` and `distinct_release_artists` of ADR 0049 §7
do not change. They count the albums that name the publisher, and a `v1`
field keeps its meaning (ADR 0044 §4).

### 2. The kind

A feed read of a publisher feed adds `publisher_kind` and
`publisher_kind_source`. The node applies the first step that matches:

1. **Stated.** One or more listed rows have a `role` with a `role_source`
   other than `default`, and each such role is the same set. When that set is
   `label`, the kind is `label`. When it is `artist`, the kind is `artist`.
   The source is `rel`.
2. **No artist.** `listed_release_artists` is empty. The kind is `unknown`,
   and the source is `no_listed_artist`.
3. **Name match.** Each value of `listed_release_artists` holds the publisher
   name as whole words. The kind is `artist`, and the source is
   `name_match`.
4. **Other artists.** Two or more values do not hold the publisher name. The
   kind is `label`, and the source is `multiple_artists`.
5. **Else.** The kind is `unknown`, and the source is `one_other_artist`.

The publisher name is the `title` of the publisher feed. "Holds as whole
words" means: after the normalization of ADR 0049 §7, split each value at each
character that is not a letter or a digit. The words of the album artist hold
the words of the publisher name as one run in the same sequence.

A stated role with a set other than `label` or `artist`, or two different
stated sets, does not stop the rule. The node continues at step 2.

### 3. The kind on an album

Each `music_to_publisher` row of the `publisher` view gets
`remote_publisher_kind` and `remote_publisher_kind_source`. They are the
values of §2 for the publisher that the row resolves to. They are null when
the row does not resolve.

### 4. The node derives at each read

The node stores no kind. It derives the values at each read, as ADR 0049 §3
requires for a resolution. A publisher read already resolves each listed
album and reads its summary (ADR 0059). An album read costs the rows of one
more publisher.

### 5. The kind is not a fact of the feed

The API documentation says that `publisher_kind` is derived, and gives each
source. A client shows a derived kind as derived, for example "Label
(derived from 33 artists)". Only the source `rel` is a statement of the feed.

## Alternatives Considered

### Derive the kind in each client
Each client would write its own rule, and two clients would show different
kinds for one publisher. Rejected.

### Use `distinct_release_artist_count`
It counts only the albums that name the publisher. It gives 0 for "Master's
Scroll". Rejected as the input. It stays in the API with its present meaning.

### Compare the names for equality
It classifies an artist with a band name that holds the artist name as a
label. The test found one such publisher in 240. Rejected.

### Classify one other artist as a label
A publisher with one album artist of a different name can be a label with
one artist, or an artist with an alias. The data cannot tell them apart.
Rejected. The kind is `unknown`.

### Use the `itunes:author` of the publisher feed as its name
Publisher feeds often give no author. "Master's Scroll" gives none. The title
is always present. Rejected.

## Consequences

- musicindex.org and v4vmm show the same kind for each publisher, with its
  source.
- "Master's Scroll" shows 33 listed artists and the kind `label`.
- A publisher that states `rel` controls its kind.
- A label with one artist shows `unknown`, not `label`.
- An album read with `include=publisher` reads the listed albums of its
  publisher.

## Invariants

- The node stores no kind.
- `publisher_kind_source` is `rel` only when a listed row states a role.
- A derived kind never replaces a stated role.

## Guards

- A publisher that lists 3 albums by 3 other artists gives `label` and
  `multiple_artists`.
- A publisher "Mishkin Fitzgerald" that lists albums by "Mishkin Fitzgerald"
  and "Birdeatsbaby (Mishkin Fitzgerald)" gives `artist` and `name_match`.
- A publisher "Ann" that lists an album by "Joanne" gives `unknown` and
  `one_other_artist`, because "Ann" is not a whole word of "Joanne".
- A publisher with rows that state `rel="label"` gives `label` and `rel`, also
  when each album artist holds its name.
- A publisher with only `placeholder` album artists gives `unknown` and
  `no_listed_artist`.
- A publisher that lists 2 albums, of which no album names it, gives
  `listed_release_artist_count` 2 and `distinct_release_artist_count` 0.
- An album row of the direction `music_to_publisher` gives the kind of its
  publisher.
