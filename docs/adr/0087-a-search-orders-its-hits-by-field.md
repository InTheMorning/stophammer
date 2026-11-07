# ADR 0087: A Search Orders Its Hits By Field

## Status
Accepted on 2026-11-07 by the operator, for musicindex.org request 25. The build is
complete and not deployed.

It amends ADR 0086 §2, the invariants of ADR 0086 and its consequence about a `rank`
above 0.

## Date
2026-11-07

## Context
Request 25 states that `q=love&type=publisher` gives Callum in row 4. The word is in
the description of Callum and not in its title. "Love Ethic" and
"Strange Love (Wavlake)" have the word in the title, and they are in rows 6 and 8.
The site shows no description in a row, so this match looks random to a person.

The backup of 2026-11-07 shows three causes:

1. `bm25(search_index)` gives the same weight to `name`, `title` and `description`.
2. BM25 counts the length of the full row. A long description lowers the score of a
   title hit.
3. The quality weight can change the sequence of two scores. "Love Ethic" has a better `rank` than
   Callum, -5.16 and -4.87, and its quality is 35 and not 50.

ADR 0086 §2 adds 1,000,000 to the `rank` of a hit by prefix. Its reason is that a
client can sort by `rank`. This reason is no longer correct:

| Client | What it does with `rank` |
|---|---|
| musicindex.org | Commit `4fa24eb` removed the sort from `search.html`. No other code of the site reads the field |
| v4vmm | It keeps the field, and only its tests read the value |
| musicindex-studio | It does not read the field on a search row |
| musicindex-lib | It does not read the field on a search row |

The node reads one number for the order: `effective_rank`. The cursor holds that
number and a `rowid` (`src/query.rs`).

## Decision

### 1. Three groups of hits
With `sort=relevance`, the node puts each hit in the first group that matches. The
rule is the same for each type.

| Group | The hit |
|---|---|
| 1 | All words of `q` are in the title, or all words of `q` are in the artist name |
| 2 | All words of `q` are in the title and the artist name together |
| 3 | Each other hit. This includes a match in the description |

In each group, a hit with the whole words comes before a hit by prefix. In one such
set, the score and the quality weight give the sequence, as ADR 0022 states.

An artist name has the group of a title. The operator selected this on 2026-11-07.
A search for an artist thus keeps the rows of that artist near the top.

### 2. The offset of a group goes only into `effective_rank`
- `effective_rank` is the score with the quality weight, plus the offset of the group.
  The reason for the offset is the cursor, which holds one number and a `rowid`. The
  cursor keeps its form.
- `rank` is the BM25 score with no offset, so it is 0 or less. It has the same value
  on each sort.
- The sequence of the rows is the order. `rank` does not give the order, and a client
  must not sort by it.

### 3. The search does less work
Three changes hold the search near its earlier time. Each one keeps the rows and the
scores.

- A request with no `type` runs one statement for feeds and tracks. It does not run
  two statements and then merge them.
- The node checks the type of a row before it calculates BM25. A row of another type
  costs one lookup and no score.
- The hits are the outer loop of the join. The planner does not know the size of a
  materialized set. With no such join, it can read each row of a type.

For one phrase, the node reads one column filter, because the second group cannot
have a hit. The node also keeps the prepared rank statement on each connection.

### 4. The index does not change
This change needs no migration, no repair and no rebuild.

### What this ADR does not decide
- The placeholder "Unknown Artist" in the `name` column. It stays a separate change.
- A search for a `musicL` feed (request 23).
- A field that states the group of a hit. No client asks for it.

## Evidence
The backup of 2026-11-07 16:06 UTC holds 39,275 index rows. Each time comes from the
release binary, through HTTP, on a disk copy with the bundled SQLite 3.45.0. Each
request has the default limit. These times are not a load test.
Other sessions used the machine at the same time.

| `q` | No type | Ratio | Page 2 ratio | `type=feed` | Ratio | `type=publisher` | Ratio | Cold | Ratio | Cold disk read, before and after |
|---|---|---|---|---|---|---|---|---|---|---|
| `s` | 77.0 ms | 0.90 | 0.91 | 46.0 ms | 1.24 | 35.8 ms | 1.22 | 815 ms | 0.79 | 14.2 and 14.3 MB |
| `a` | 64.6 ms | 0.92 | 0.89 | 42.9 ms | 1.31 | 32.7 ms | 1.21 | 790 ms | 1.00 | 14.2 and 14.3 MB |
| `the` | 40.5 ms | 1.09 | 1.06 | 23.1 ms | 1.38 | 15.9 ms | 1.33 | 815 ms | 1.14 | 13.5 and 13.6 MB |
| `the l` | 32.7 ms | 0.98 | 1.00 | 27.2 ms | 1.60 | 22.2 ms | 1.61 | 862 ms | 1.35 | 12.3 and 12.6 MB |
| `lo` | 14.2 ms | 0.95 | 1.00 | 8.0 ms | 1.18 | 5.6 ms | 1.21 | 823 ms | 1.29 | 11.8 and 12.0 MB |
| `jsmo` | 1.1 ms | 0.87 | 0.80 | 0.9 ms | 0.97 | 0.7 ms | 0.93 | 43 ms | 1.00 | 0.8 and 0.8 MB |

"Ratio" is the time divided by the time of the binary before this ADR. The warm time
is the median of 15 requests, and each binary ran two times. The cold time is the
first search after a restart and a removal of the database from the page cache.

The cold time changes between runs for one binary. Before this ADR, `the` took 700 ms
in one run and 922 ms in another. Over the runs of the last form of the code, the cold
ratio of `the`, `the l` and `lo` was between 1.01 and 1.36. So this ADR can add up to
approximately one third to a cold search of these queries. The disk reads change by at
most 0.3 MB.

The first build was faster cold on five queries, by up to 28%. That result came from
the method. The harness removed the pages before the node start, and the start read
29 MB again. The plan of that build then read 3.5 MB less. When the harness removes
the pages after the start, the two binaries read the same 14.2 MB for `s`, and the
advantage goes.

The first build also took `jsmo` from 1.0 ms to 2.7 ms warm, with 4.5 MB read cold in
place of 0.8 MB. The query plan on SQLite 3.45.0 gave the cause: a scan of each
`search_entities` row of a type. The join in §3 removes it. In SQL only, `jsmo` went
from 2.21 ms to 0.18 ms.

The sequences from the real node, with this ADR:

| Request | Rows |
|---|---|
| `q=love&type=publisher` | Strange Love, Love United, I love dafneestre, Love Ethic, Strange Love (Wavlake), Loveska, Lovenix Music, LoveFinger, LoveStroke Radio, Callum |
| `q=lov&type=publisher` | Strange Love, Love United, I love dafneestre, Loveska, Love Ethic, Lovenix Music, LoveFinger, LoveStroke Radio, Strange Love (Wavlake), JACK_1986 |
| `q=the l&type=feed` | LVX3 l Into The Unknown l, LVX1 l Answering the initial call l, On The Low, The Truth of Life (The Bugle), Save The Young |
| `q=burna` | OLUWA BURNA, 4 KAMPÉ II, DANGOTE, SOKE, WHISKEY, TONIGHT, SAMPUDI, WONDERFUL, BEBO, KABIYESI. Each row is by BURNA BOY_ |

A sample of real queries compared the orders, in Python on SQLite 3.54.0. The sample
has 285 searches for one word of an artist name, with no type. It also has 387 feed
searches for a word of a title.

| Order | Rows with the word in the title or artist name that follow another row, feed search | Artist rows in the first 10, artist search |
|---|---|---|
| Before this ADR | 63.0% | 79.2% |
| BM25 column weights 5, 10, 1 | 43.2% | Not measured |
| Title, then name, then description | 0% | 67.2% |
| This ADR | 0% | 79.7% |

## Alternatives Considered

### BM25 column weights
This is the smallest change, and it adds almost no time. It cannot give the order. A
long row or a quality weight can still put a description hit first.

### Title, then artist name, then description
This is the literal form of the request. In 33 of the 285 artist searches, the rows of
the artist lost at least half of their places in the first 10. With `q=burna`, tracks
with "FT. BURNA BOY" come before the tracks of Burna Boy.

### The offset also in `rank`
This is the form of ADR 0086 §2. It keeps a reason that no client has, and each client
must then accept a `rank` above 0.

### A group field in the cursor
This gives §1 with no offset. It needs a new cursor form. The offset gives the same
order with the current form.

## Consequences
- `q=love&type=publisher` gives each title hit before Callum, with no client change.
- A search for an artist keeps the rows of the artist near the top.
- A prefix hit in a title can come before a whole word in a description, also when
  the person typed the complete word.
- A request with a filter and no `type` uses the rule of ADR 0084 §6 for each row.
  Tests compare the rows with the rows of each type, for four filters.
- A site with no `4fa24eb` sorts each page by `rank`, and it loses the group order in
  that page. On 2026-11-07 the live `search.html` equals musicindex commit `1f18039`,
  which holds `4fa24eb`. So the site does not have to deploy first.
- A cursor from before the upgrade can repeat or skip rows one time.
- `rank` is 0 or less again. This removes a consequence of ADR 0086.
- No event payload changes, so a primary and a community node can upgrade in each
  sequence.
- `docs/API.md` and the `q` description in `src/openapi.rs` change in the same commit
  as the code.

## Invariants
This ADR replaces the invariant of ADR 0086 "A hit with the whole word comes before
each hit by prefix only". That invariant is no longer correct: a title prefix hit can
now come before a whole-word hit across two fields, or in a description. "On The Low"
comes before "THE FLOOD" by T.E.A.L for `q=the l`.

The new invariants:

- A hit in an earlier group comes before each hit in a later group.
- In one group, a hit with the whole words comes before each hit by prefix.
- A hit with the whole word has the `rank` of the release before ADR 0086.
- `rank` is a BM25 score. The group and the quality weight do not change it.
- A filter never changes `rank` (ADR 0080 §2).

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0087_match_group_tests.rs` prove each of these:

1. §1: a title hit comes before a description hit, also with a longer row and a lower
   quality.
2. §1: quality can put an artist name hit before a title hit, and never a description
   hit.
3. §1: a title prefix hit comes before a description hit with the whole word.
4. Invariants: a title prefix hit comes before a whole-word hit across two fields.
5. §1: one field comes before two fields, and two fields come before each other hit.
6. §1: in each group, quality does not put a prefix hit before a whole word.
7. §1: text in double quotes has the groups and no prefix hit.
8. §2: `rank` is 0 or less in each group, and it has the same value with `sort=name`.
9. §2: pages with limit 1 give each row one time, in the sequence of one large page.
10. §3: one statement gives the rows and the ranks of two merged searches.
11. §3: a typed search gives the score of a search of each type.
12. §3: with a text filter and no type, the rows are the rows of each type.

The tests of ADR 0086 stay. Its test of a `rank` above 0 now checks `effective_rank`.

### Manual
A test cannot read the query plan through the public functions, so the join of §3 has
a manual check. On a copy of a backup, the first search of `q=jsmo` after a restart
and a removal of the pages reads less than 1 MB.

### Visual
None in this repository. The site owns its rows.
