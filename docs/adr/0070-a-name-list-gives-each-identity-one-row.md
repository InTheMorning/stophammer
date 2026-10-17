# ADR 0070: A Name List Gives Each Identity One Row

## Status
Proposed on 2026-10-17, for v4vmm request 6.

## Date
2026-10-17

## Context

v4vmm request 6 of 2026-10-17 asks for two lists in name order, each with a
cursor. One list holds the music releases, and one holds the artists. v4vmm ADR 0084
Decision 8 gives its Releases list and its Artists list the scope
"Everything". That scope shows the Library items of the client and the Index
items of the node in one name order.

No route gives either list now:

- `GET /v1/feeds/recent` gives the feeds newest first.
- `GET /v1/publishers` needs a search text `q`. It groups the `publisher`
  text of the music feeds. It does not list the publisher feeds.

v4vmm ADR 0084 Decision 7 gives an artist page one of two identities:

- The **publisher level** is the page of a publisher feed, keyed by its feed
  GUID.
- The **name level** groups the releases that state an artist name and do not
  name a publisher feed. A name is not an identity, and this page never merges
  with a publisher page.

A release opens the publisher level when its `<podcast:publisher>` names a
publisher feed. The v4vmm operator decided on 2026-10-08 on one exception. A
link whose agreed roles do not include `artist`, for example a label, opens the
name level.

The production backup of 2026-09-26 gave these counts:

| Measurement | Count |
|---|---|
| Music feeds | 8,641 |
| Publisher feeds | 1,772 |
| Different artist names, after a trim and a change to lowercase | 1,977 |
| Music feeds that name a publisher feed that the node holds | 7,999 |
| Links where the publisher feed title is equal to the artist name | 1,742 of 1,784 |
| Names with no such publisher feed on any release | 223 |
| Names with some releases that name a publisher feed and some that do not | 34 |
| Names that point at more than one publisher feed | 28 |
| Publisher feeds that no music feed of the node names | 19 |

Of the music feed titles, 8,289 start with a letter from `a` to `z` and 188
start with a digit. 106 start with a character that is not ASCII, and 58 start
with punctuation. 20
titles have white space at the end or a run of white space inside.

`feeds.title_lower` holds the title in Rust lowercase, with no trim. The index
`idx_feeds_title_guid (title_lower, feed_guid)` exists. `release_artist_sort`
is always null, because the ingest never sets it.

## Decision

### 1. One sort key, and each row gives it

The **sort key** of a text is the text after the normalization of ADR 0049
§7:

1. Remove the white space at the start and at the end.
2. Replace each internal run of white space with one space.
3. Change each character to Unicode lowercase.

The node does not remove a leading article such as "The", and it does not
remove punctuation or accents. A client cannot repeat such a rule exactly.

Two lists compare the sort keys by Unicode code point. This is the byte order
of UTF-8 and the order of the SQLite `BINARY` collation. Each row gives its key
in `sort_name`. Thus a client can put its Library items into the same order
with no rule of its own.

The node stores the sort key of each feed title, so that the release list
reads an index.

### 2. The release list

`GET /v1/feeds/by-name` gives the feeds of one medium in the order of
`sort_name`, then of `feed_guid`. The default medium is `music`.

Each row gives the fields of a row of `GET /v1/feeds/recent`, and
`sort_name`. The route takes `limit` and `cursor` as the recent list does.

The route takes `from`. The list then starts at the first row whose sort key is
equal to or after the sort key of `from`. `from=M` starts at the first title
that starts with `m`. A request with `cursor` ignores `from`.

### 3. The artist list gives each identity one row

`GET /v1/artist-names` gives one row for each artist identity of v4vmm ADR
0084 Decision 7, in the order of `sort_name`, then `kind`, then `key`. Each row
gives `kind`:

| `kind` | One row for each | `name` | `key` |
|---|---|---|---|
| `publisher` | Feed with the medium `publisher` | The feed title | The feed GUID |
| `name` | Sort key of the `release_artist` of a release of the name level (section 4) | The raw `release_artist` that the most releases of the row state. With a tie, the first raw value in code point order | The sort key |

Each row also gives:

| Field | Value |
|---|---|
| `sort_name` | The sort key of `name` for a `publisher` row. The key of the row for a `name` row |
| `feed_guid` | The feed GUID for a `publisher` row. Null for a `name` row |
| `image_url` | The image of the publisher feed (ADR 0054 §4). Null for a `name` row |
| `release_count` | The number of music releases of the name level with that key, for a `name` row. Null for a `publisher` row |

The node does not merge a `publisher` row and a `name` row with the same text.
The same text can thus show in two rows. A `name` row is a fact about the text
of the releases, and the response does not state that it is an artist.

The route takes `limit`, `cursor` and `from` as the release list does.

### 4. A release of the name level

A music release is of the name level when one of these conditions is true:

- No item inside its `<podcast:publisher>` names a publisher feed that the node
  holds.
- Each such link has `role_agreement` `both`, and its agreed role set does not
  include `artist` (ADR 0049 §6a).

Thus the `release_count` of a `name` row counts the releases that open the
name level of v4vmm ADR 0084 Decision 7.

### 5. The node derives no artist

The node gives facts about feeds and texts. It does not decide that a name and
a publisher feed are the same artist, and it does not split a credit such as
"feat.". The client shows the rows and their kinds.

### 6. The routes are additions

The two routes are new `v1` routes (ADR 0044 §4). `/v1/node/capabilities`
lists them. Each one is in the OpenAPI document with the schema of its row
type.

## Alternatives Considered

### One row for each name, with a list of publisher feed GUIDs

Each name shows one time. But the node must decide that the same text is the
same artist, and ADR 0049 §7 and v4vmm ADR 0084 Decision 7 refuse that guess.
On 2026-09-26, 28 names pointed at more than one publisher feed, and 19
publisher feeds had no music feed. Such a list does not show those 19 feeds.

### Sort by `title_lower`

The index exists, and no migration is necessary. But `title_lower` keeps the
white space at the start and at the end, and the client must know a second
rule. One rule for the two lists is simpler for a client.

### A sort without a leading article

"The Beatles" then sorts under B. The rule for each language is long, and a
client cannot repeat it exactly. The client can show a different order of its
own when it does not merge the two sources.

### The path `/v1/artists`

Commit `e5d9dfb` removed a route with this path and the artist records that it
gave. A path that comes back with a different meaning can confuse an old
client.

## Consequences

- v4vmm packet 016 can merge its Library rows with the Index rows by
  `sort_name`.
- The node writes a sort key for each feed title. An existing database gets the
  values from a migration.
- The artist list needs the link facts of ADR 0049 §6a for each release.
  Task 001 measures the cost of a page on the backup, and it chooses a
  calculation at each read or a derived table.

## Acceptance Criteria

Mechanical, each one checked by an integration test:

- The release list gives the feeds in code point order of `sort_name`, and a
  cursor continues with no gap and no repeat.
- `from` starts the release list and the artist list at the first key equal to
  or after the key of `from`.
- The artist list gives one `publisher` row for each publisher feed, also for a
  feed that no music feed names.
- A release whose only link is a label link with `role_agreement` `both` counts
  in the `release_count` of its `name` row.
- A release with a link that is not a label link does not count in a `name`
  row.
- The OpenAPI guards of ADR 0044 pass for the two routes.

Visual, checked by the v4vmm operator:

- The v4vmm "Everything" scope shows the Library items and the Index items in
  one order.
