# ADR 0089: An Artist List Takes Newest First

## Status
Accepted on 2026-11-08 by the operator, for musicindex ADR 0053, the shared list
control rule. The build is complete. The primary runs it in the candidate `v0.19.0-rc.5`, and no release carries it.

It adds to ADR 0084 §3 and §11 and to ADR 0071 §4. ADR 0084 §2 does not change.

## Date
2026-11-08

## Context
musicindex ADR 0053 (commit `f88e9b8` in that repository) gives each list of the
three apps the controls A to Z, Newest and reverse, where they make sense. Two
lists of an artist cannot give Newest today:

1. **The releases of an artist.** `GET /v1/feeds/by-name` takes `artist_key`
   (ADR 0071 §4), but `GET /v1/feeds/recent` does not. A client cannot sort the
   pages of an A to Z list into a date sequence.
2. **The Artists list.** `GET /v1/artist-names` has only the name sequence, and a
   row gives no date.

The other lists of ADR 0053 sort on the node now (ADR 0084).

## Decision

### 1. `GET /v1/feeds/recent` takes `artist_key`
`artist_key` keeps the music releases that the `name` row of
`GET /v1/artist-names` with that `key` counts, as on `GET /v1/feeds/by-name`. The
two routes read the same set from the copy in memory, so the list agrees with
`release_count`.

- The sequence and `reverse` of `GET /v1/feeds/recent` do not change.
- `artist_key` needs `medium=music`, and conflicts with `named_feed_guid`. Each
  gives `400`, as on `GET /v1/feeds/by-name`.
- An unknown key gives an empty list.

### 2. `GET /v1/artist-names` takes `sort=date`
Each row gives `newest_release_at`, the newest `newest_item_at` of its releases:

| Row | Its releases |
|---|---|
| `name` | The releases that the row counts, the set of `artist_key` |
| `publisher` | The music releases that name the feed, the set of `named_feed_guid` (ADR 0076 §2) |

The value is null when no such release has a date.

`sort` takes `name`, the default, or `date`:

- `date` gives the newest `newest_release_at` first. The tie key is `sort_name`,
  `kind` and `key`.
- A row with no date comes last in each direction.
- With `reverse=true`, the oldest date comes first, and the tie key goes down.
- `from` names a letter, so `sort=date` with `from` gives `400`. Another `sort`
  value gives `400`.
- A cursor of `sort=date` works only with `sort=date`, and a cursor of
  `sort=name` only with `name`. The direction rule of ADR 0084 §9 also applies.

The node calculates the dates and the two date sequences in the build of the copy in
memory (ADR 0070 §4a). A page reads no database row.

### 3. A capability names the change
`GET /v1/node/capabilities` lists `artist_newest`. A node with `sort` and no
`artist_newest` ignores the two parameters with no error. It gives each release for
`artist_key` on `GET /v1/feeds/recent`, and the name sequence for `sort=date`. So a
client offers Newest on these two lists only when the node lists
`artist_newest`. `sort` cannot name this change, because each node of release
candidates `0.19.0-rc.2` to `0.19.0-rc.4` lists `sort`.

### What this ADR does not decide
- The other gaps of the list report of 2026-11-07: the tracks of an artist by
  name or date, and the live items by time.
- A date sort on `GET /v1/feeds/by-name`. `GET /v1/feeds/recent` gives it.

## Evidence
The measurements use a disk copy of the backup of 2026-11-07 20:56 UTC, with the
release binary. They are not a load test.

| Fact | Value |
|---|---|
| Rows of the artist list | 2,090 |
| Rows with a date | 2,058 |
| First build of the list, before this ADR | 2.42 to 2.52 s |
| First build of the list, with this ADR | 2.49 to 2.52 s |
| A page from the copy in memory | 0.3 to 0.4 ms |

The first form of the build called `db::get_feeds_naming_feed` for each of the
1,810 publisher feeds. The build then took approximately 4.5 to 7 s. The build now
resolves each of the 8,134 `<podcast:publisher>` items one time, and applies the
GUID step and the URL step of that function. For each of the 2,090 rows,
`newest_release_at` was the same in the two forms.

A query with no resolution gives a different date for 1 of the 1,810 publisher
feeds. In that case an album names the URL of the feed, and its GUID resolves to
another held feed.

## Alternatives Considered

### A date sort on `GET /v1/feeds/by-name`
This gives the releases of an artist by date on the route that has `artist_key`.
The route and its cursor are then by name or by date, and a second route gives
the date sequence now. One parameter on `GET /v1/feeds/recent` gives the same
list.

### The date of a publisher row from its own feed
A publisher feed has no dated item in most cases, so most publisher rows have no
date.

### A query with no resolution
One query with the GUID and the URL of each publisher item is faster to write. It
gives a different set from `named_feed_guid` for 1 publisher feed, so the date and
the list of the releases of that feed can disagree.

### The `sort` capability
See §3.

## Consequences
- A client can give Newest on the releases of an artist and on the Artists list.
- Each row of the artist list gives one more field. The change is an addition, so
  it belongs in a MINOR release (ADR 0066). No event and no table changes.
- A request with an unknown parameter value now gives `400` on
  `GET /v1/artist-names` for `sort`. Before this ADR the route ignored `sort`.

## Invariants
- A page of `GET /v1/artist-names` with `sort=date` reads no database row.
- The date of a row comes from the same release set as its list: `artist_key` for
  a `name` row, and `named_feed_guid` for a `publisher` row.
- A row with no date comes last in each direction.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0089_artist_newest_tests.rs` prove each of these:

1. §1: `artist_key` on `GET /v1/feeds/recent` gives the releases of the row,
   newest first, the set of `GET /v1/feeds/by-name`, and the length of
   `release_count`.
2. §1: with `reverse=true`, the oldest release comes first.
3. §1: `artist_key` with another medium, or with `named_feed_guid`, gives `400`.
4. §1: an unknown key gives an empty list.
5. §2: a `name` row and a `publisher` row each give the newest date of their
   releases, or null.
6. §2: `sort=date` gives the newest first, a row with no date last, and the tie
   key up.
7. §2: with `reverse=true`, the oldest first, a row with no date last, and the tie
   key down.
8. §2: `sort=name` and no `sort` give the same name sequence.
9. §2: pages of 1 and of 2 rows give each row one time, in each direction.
10. §2: `sort=date` with `from`, an unknown `sort`, and a cursor of the other sort
    each give `400`.
11. §3: the capabilities list `artist_newest`.

The guards of ADR 0044 cover the two parameters and the field
`newest_release_at`.

### Manual
For each row of the artist list on a copy of a backup, `newest_release_at` must
be the newest date of `db::get_feeds_naming_feed` for a publisher row. A test
cannot read that function on a copy of production data. The check of 2026-11-08
found no difference in 2,090 rows.

### Visual
None in this repository. The apps own their list controls (musicindex ADR 0053).
