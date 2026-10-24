# ADR 0079: A Retired GUID Returns Only From Its Source

## Status
Accepted on 2026-10-23, after the review of ADR 0078. Tasks 001 to 003 give
the build. It amends ADR 0057 §4 and ADR 0067 §4. A retired feed comes back as a
new feed only through its last source URL.

## Date
2026-10-23

## Context

ADR 0051 stops a takeover of a feed by a publisher of a different URL. A
submission with a held GUID from a URL that is not the source URL is a mirror.
The node records the URL and does not apply the content. ADR 0051 names this
attacker: "Hosts any RSS body at any URL, and causes a crawl".

That protection needs a record. `classify_submission` (`src/db.rs:6337`) reads
only the `feeds` table. When no row has the GUID, a submission from each URL is
`NewFeed`, and the node takes its URL as the new source URL.

Three retire paths delete the record and write no row in `feed_blocks`:

| Path | Reason in `FeedRetired` | Owner |
|---|---|---|
| A `podcast:block` tag at the source URL | `podcast_block` | ADR 0057 §3 and §4 |
| Two gone answers from the source URL | `source_gone` | ADR 0067 §3 and §4 |
| `DELETE /v1/feeds/{guid}?block=false` | `admin retired via API` | ADR 0053 §1 |

The node writes no block on purpose. The publisher can then return the feed
through the feed itself, with no operator step (ADR 0057 §4, ADR 0067 §4).

After such a retire, no record holds the GUID. An attacker can host a copy of
the body with the same `podcast:guid`, other payment recipients and no block
tag. One crawl of that URL admits the copy as a new feed. The attacker can find
the free GUIDs, because each `FeedRetired` event is public and gives its
reason.

Then the publisher removes the tag, or the host serves the feed again. The
submission from the correct source URL is now a mirror, with the reason
`source_conflict`. The node does not apply it. ADR 0053 §4 makes a change of
payment recipients visible, but a new record has no earlier recipients to
compare. Only an operator relocation (`relocate_feed`, `src/api.rs:5284`)
repairs the record, and only when a person sees the problem.

The event log keeps the last source URL of each retired feed. Each
`feed_upserted` payload holds the full feed, with `feed_url`. In the backup of
2026-10-21, one GUID had a `FeedRetired` event and no record. It is the test
feed of the operator, with the reason `podcast_block`.

This review read the code. No test has shown the takeover yet.

## Decision

### 1. A retire without a block keeps a hold

A retire can delete a record and write no row in `feed_blocks`. Then the
primary writes a row in a new table `retired_source_holds`, in the same
transaction:

| Column | Value |
|---|---|
| `feed_guid` | The GUID of the record, in lower case. The primary key |
| `source_url` | The source URL of the record at the retire |
| `reason` | The reason of the `FeedRetired` event |
| `retired_at` | The time of the retire |

This applies to the three paths of the context. A retire that writes a block of
ADR 0053 writes no hold, because the block already rejects the GUID and the
URL. A retire of an old GUID by ADR 0052 §5 writes no hold, because
`feed_guid_supersessions` links that GUID to its successor.

The table is local to the primary, as `source_gone_answers` is. It makes no
event. A community node holds no row, because only the primary classifies a
submission.

### 2. A held GUID returns only through its source URL

The node applies this rule in the `NewFeed` case of ADR 0051 §2. It applies
the rule after the check of ADR 0053 and before the block rule of ADR 0057:

| Condition | Result |
|---|---|
| No hold has the GUID | The normal `NewFeed` case |
| A hold has the GUID, and `source_url` or `canonical_url` of the submission is the held URL | The normal `NewFeed` case. When the node admits the record, it deletes the hold in the same transaction |
| A hold has the GUID, and neither URL is the held URL | Write nothing. Reason `retired_source_conflict`. Log at `warn` level with the GUID, the held URL and the submitted URLs |

The comparison uses the exact stored string, as ADR 0051 §1 does. A redirect
from the held URL to a CDN thus passes, as it does for a record.

A submission from the held URL that keeps its `podcast:block` tag gets
`source_blocked`, and the hold stays. When the admitted feed declares a move,
ADR 0052 applies to the new record as usual.

A hold has no expiry. An expiry would open the same takeover again at a known
time.

### 3. The operator can release a hold

`DELETE /v1/retired-source-holds/{guid}` deletes the hold of one GUID. It needs
the admin token (ADR 0056). After the release, the next submission of the GUID
from each URL is the normal `NewFeed` case. The operator uses it when the
publisher moved the feed to a different URL while the feed was retired.

### 4. The primary fills the holds of earlier retires

On its first start with this decision, the primary writes a hold for each GUID
that meets all of these conditions:

- its last event of the type `feed_retired` has the reason `podcast_block`,
  `source_gone` or `admin retired via API`,
- no record has the GUID,
- no row of `feed_blocks` has the GUID.

The held URL is the `feed.feed_url` of the last `feed_upserted` event of the GUID
before that retire. A GUID with no such event gets no hold, and the primary logs
it at `warn` level. The fill writes no row for a GUID that already has a hold, so
a second start writes nothing.

## Alternatives Considered

### Write a block of ADR 0053 on each retire

A block rejects the true source URL as well. The publisher could not bring the
feed back without the operator. ADR 0057 rejected this as "Write a durable
block on the retirement".

### Keep the record and hide it

ADR 0057 rejected this as "Hide the feed and keep the record". Each read route
would need a filter, and a missed route would show a feed that asked to leave.

### Compare the payment recipients of the new feed with the retired feed

The node would need to keep the recipients of each retired feed. An attacker can
also copy the recipients first and change them after the admission. The source
URL is the fact that ADR 0051 already trusts.

### An expiry for a hold

After the expiry, the takeover is possible again, at a time that each person
can calculate from the public event.

## Consequences

- A retired feed comes back without the operator only through its last source
  URL. A publisher that also moved the feed to a new URL needs the operator
  release of §3.
- A Wavlake feed that ADR 0067 retires and that comes back only at its self link
  (ADR 0052) needs the release of §3. No feed is in that state in the backup of
  2026-10-21.
- The rule trusts the source URL. A person who gets control of the domain of an
  expired source URL can still claim the GUID. ADR 0051 has the same limit for a
  record.
- The rule does not change the attacker of ADR 0055, a holder of `CRAWL_TOKEN`.
- `docs/API.md` and the OpenAPI document give the reason
  `retired_source_conflict` and the route of §3. Each new enum value goes to
  `tests/fixtures/v1-schema-fields.json` (ADR 0044).
- The table needs a migration and an `ensure_` repair (ADR 0046), and a line in
  `tests/fixtures/migration_sha256.txt` (ADR 0023).

## Invariants

- A retire that writes no block of ADR 0053 writes a hold.
- A submission for a held GUID from a URL that is not the held URL writes no row.
- Only the admission from the held URL and the operator release delete a hold.

## Checks

Each criterion is mechanical. A test in `tests/` proves it.

1. §1: a test retires a feed by `podcast:block`, by two gone answers and by
   `DELETE /v1/feeds/{guid}?block=false`. After each retire,
   `retired_source_holds` has one row with the source URL of the record.
2. §1: a retire with a block, and a retire by a GUID change, write no hold.
3. §2: after each retire of check 1, a submission with the same GUID from a
   different URL gets `retired_source_conflict`. The node then holds no record
   and no new event for the GUID.
4. §2: after each retire of check 1, a submission from the held URL without the
   block tag admits the feed, with the same feed-scoped track identities, and
   deletes the hold. The present test
   `feed_removes_block_is_readmitted_with_same_track_ids` stays green.
5. §2: a submission from the held URL that keeps the block tag gets
   `source_blocked`, and the hold stays.
6. §3: after the release, a submission from a different URL admits the feed.
   The route without the admin token gives `401` or `403`, as each other admin
   route does.
7. §4: a database with a `feed_retired` event and no record for a GUID gets one
   hold with the URL of the last `feed_upserted` event. A second start writes
   no row.
8. §1: a community node that applies a `FeedRetired` event writes no hold.

No criterion is visual.
