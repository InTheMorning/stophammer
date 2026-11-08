# ADR 0090: A Name Sorts By A Folded Key

## Status
Accepted on 2026-11-08 by the operator, for the name order that v4vmm reported on
2026-11-08 and musicindex ADR 0053. The build is complete. The primary runs it in
the candidate `v0.19.0-rc.6`, and no release carries it.

It amends ADR 0070 §1 and ADR 0084 §8. ADR 0049 §7 and the identity rules of ADR
0070 §3 and ADR 0071 §4 keep their key.

## Date
2026-11-08

## Context
`model::sort_key` makes each name key of the node. It removes the white space at
each end, joins each internal run of white space into one space, and makes the
text lowercase. Two keys compare by Unicode code point (ADR 0070 §1).

The key has three uses for the sequence:

| Use | Stored in | Read by |
|---|---|---|
| The title of a feed | `feeds.title_sort`, with an index | `GET /v1/feeds/by-name`, `sort=name` of `GET /v1/search` |
| The title of a track | `tracks.title_sort` | `sort=name` of `GET /v1/search` (ADR 0084 §8) |
| A row of the artist list | `sort_name`, in memory | `GET /v1/artist-names` |

The key is also an identity. These compare names by it:

- the `key` of a `name` row and the value of `artist_key`;
- the release groups of the artist list;
- the artist count of ADR 0049 §7.

v4vmm reported that "✳ZBD✳Chill✳Vibes✳", "Øystein Berge" and "Zëmar Red" come
after Z. In the backup of 2026-11-07 20:56 UTC:

| Keys | Count | First character after `z` | Digit first |
|---|---|---|---|
| Music feed titles | 8,887 | 109 | 203 |
| Publisher feed titles | 1,810 | 6 | 12 |
| Artist name keys | 2,011 | 7 | 17 |
| Track titles | 28,609 | 301 | 734 |

A character after `z` is a symbol or an emoji (✳, ⛧, ₿, †), a letter such as `ø`,
an accented letter, or a letter of another script. Digits and ASCII symbols come
before `a`. A number is text, so "track 10" comes before "track 9". With numbers
as numbers, 470 of the 8,887 music titles move.

`from=O` does not reach "øystein". No value of `from` selects the names before
`a`, because they are the start of the list.

## Decision

### 1. The identity key does not change
`sort_key` stays as ADR 0070 §1 gives it. It makes the `key` of a `name` row,
`artist_key`, the release groups and the artist count of ADR 0049 §7.

### 2. An order key gives the sequence
`order_key(text)` makes the key of each sequence. It starts with `sort_key(text)`
and takes four steps:

1. **Fold.** The key takes the NFKD decomposition. Then it removes each
   combining mark. So `ë` becomes `e` and `é` becomes `e`. A table changes the
   letters that have no decomposition. It changes `ø` to `o`, `æ` to `ae`, `œ`
   to `oe` and `ß` to `ss`. It also changes `ł` to `l`, `đ` and `ð` to `d`, `þ`
   to `th` and `ı` to `i`.
2. **Numbers.** Each run of ASCII digits gets zeros at its start, to 20 digits. A
   longer run does not change. So `2` comes before `10`. NFKD changes a full-width
   digit into an ASCII digit first.
3. **Group.** When the first character is not a letter from `a` to `z`, the key
   starts with `#`. Digits, symbols, emoji and other scripts are in this group. The
   group comes before `a`.
4. The key keeps the leading article and the punctuation, as now.

An empty text gives an empty key, so an empty `from` names no position.

Two order keys compare by code point, as now. In the `#` group, the folded text
gives the sequence.

### 3. Where the order key applies
- `feeds.title_sort` and `tracks.title_sort` hold the order key of the title. Each
  write makes it, as now.
- The `sort_name` of each row of `GET /v1/artist-names` is the order key: of the
  feed title for a `publisher` row, and of the `key` for a `name` row. The `key`
  stays the identity key.
- `from` on `GET /v1/feeds/by-name` and `GET /v1/artist-names` reads the order key
  of its text. `from=#` starts the list at the `#` group. `from=O` starts at the
  first key at or after `o`. A name that folds to `o`, such as "Øystein", is in its
  place among the `O` names. With `reverse=true`, `from=#` gives the `#` group
  from its last row (ADR 0084 §7). Each other `from` text that does not start
  with a letter is a position in the `#` group.
- The date sequence of ADR 0089 §2 uses `sort_name` as its tie key, so it uses the
  order key.

### 4. A new key for each stored row
Migration 0063 sets `title_sort` to null in `feeds` and in `tracks`. At the next
start, the repairs of ADR 0070 §1 and ADR 0084 §8 write the order key into each
row. The index of `feeds.title_sort` follows. The search index and the event log
do not change.

### 5. The dependency
The crate `unicode-normalization` 0.1 gives the NFKD decomposition and the test
for a combining mark. The Rust standard library has no Unicode normalization. A
table of the Latin range can fold the accented Latin letters, but it is long and
it covers less.

The crate brings `tinyvec` and `tinyvec_macros`. The licenses are
`(MIT OR Apache-2.0) AND Unicode-3.0` and `Zlib OR Apache-2.0 OR MIT`, and
`deny.toml` allows each of them. `cargo deny` in CI checks the three crates.

### What this ADR does not decide
- An order for the language of a reader (musicindex ADR 0053 R-LIST-12). One
  server key cannot be correct for each language. A client that holds the full
  list can sort it again.
- The removal of a leading article.
- A letter bar for each script other than Latin.

## Consequences
- A name with an accent, `ø` or another such letter sorts with its base letter.
  Each name with no letter from `a` to `z` at its start is in the `#` group, before
  `A`. A letter bar can give `#`.
- **One-time cursor effect.** The order key of a row changes at the upgrade. A
  cursor of `GET /v1/feeds/by-name`, `GET /v1/artist-names` or `sort=name` of
  `GET /v1/search` from before the upgrade can repeat or skip rows one time.
- `sort_name` gives a different value for many rows. The field keeps its meaning:
  the key of the sequence. No field is removed and no parameter changes its
  meaning, so the change belongs in a MINOR release (ADR 0066).
- The first start after the upgrade writes approximately 10,700 feed keys and
  28,600 track keys.
- A primary and a community node each make the keys at their own writes. No event
  changes, so the nodes can upgrade in each sequence.

## Invariants
- The identity key of a name does not depend on the order key.
- Each key of the `#` group comes before each key that starts with a letter.
- A run of digits sorts by its value, up to 20 digits.

## Acceptance Criteria

### Mechanical
Unit tests of `order_key` in `src/model.rs`, and tests in
`tests/adr0090_order_key_tests.rs`, prove each of these:

1. §2: `ë`, `é` and `ø` sort with `e`, `e` and `o`. `ß` gives `ss`.
2. §2: a text can start with a symbol, an emoji, a digit or another script. Its key
   starts with `#`, and comes before a key that starts with `a`.
3. §2: "track 2" comes before "track 10".
4. §1: the `key` of a `name` row and `artist_key` stay the identity key, also for a
   name with an accent.
5. §3: `GET /v1/feeds/by-name` and `GET /v1/artist-names` give the `#` group first,
   and `from=#`, `from=O` and `reverse=true` with `from=#` give the rows of §3.
6. §3: `sort=name` of `GET /v1/search` gives the order key sequence for feeds and
   tracks.
7. §4: after migration 0063 and a start, each row has the order key.

### Visual
None in this repository. The apps own their letter bars.
