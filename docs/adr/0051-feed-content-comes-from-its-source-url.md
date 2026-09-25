# ADR 0051: Feed Content Comes From Its Source URL

## Status
Proposed

## Date
2026-09-24

## Context
A feed record has two identifiers: its `podcast:guid` and its stored
`feed_url`. The ingest handler finds the existing record by the submitted URL
(`src/api.rs:1645`). The write goes to the row for the declared GUID
(`src/db.rs:1320`, `ON CONFLICT(feed_guid)`). No step compares the two.

Thus a feed at any URL can declare the GUID of a held feed. The node then
replaces the title, the tracks and the payment routes of that held feed. The
stored `feed_url` does not change (`src/api.rs:1836`). An integration test
on 2026-09-24 showed this result:

```text
FEED url=https://victim.example/feed.xml title=Attacker
TRACKS [("Attacker track", Some("attacker@ln.example"))]
FEEDROUTES ["attacker@ln.example"]
```

The attacker needs no credential. A podping from any Hive account, or a
`podcast:remoteItem` link that the crawler follows (ADR 0049 section 2),
causes the crawl. A subsequent crawl of the victim URL does not repair the
record. The hash cache uses the submitted URL as its key
(`src/api.rs:2427`), so the unchanged victim body gives `NO_CHANGE`.

ADR 0049 section 1 already states the rule that the code breaks: "That ingest
adds only a URL observation." The code adds the observation and then also
applies the content. The defect is in the first commit, `4a81de0`.

A second defect has the same cause. When a held URL starts to declare a new
GUID, the insert fails on the `UNIQUE` constraint of `feeds.feed_url`. The
node answers HTTP `500`, and the crawler retries. On 2026-09-24 this
occurred for a Doerfelverse feed. The publisher tool changed the channel
GUID and kept the five item GUIDs.

Two more gaps let a configuration remove authentication:

- `VERIFIER_CHAIN` can omit `crawl_token`, or put `content_hash` before it
  (`src/verify.rs:241`). A local probe on 2026-09-24 showed that an
  unauthenticated ingest then writes a feed.
- `CRAWL_TOKEN=""` starts the node. A request with an empty token then
  passes the check (`src/main.rs:83`).

The evidence and the scenario analysis are in
[the conflict evidence](../reviews/feed-identity-conflict-evidence.md) and
[the security audit](../reviews/security-audit-2026-09-24-feed-trust.md).

### Two attackers

This decision separates two attackers, because their costs are different:

| Attacker | Capability | This ADR |
|---|---|---|
| A feed publisher | Hosts any RSS body at any URL, and causes a crawl | Stops the takeover |
| A holder of `CRAWL_TOKEN` | Sends any URL and any parsed content to ingest | Does not stop. ADR 0055 owns this attacker |

The first attacker is any person on the internet. The second attacker must
steal a secret from the operator. This decision closes the first attack now,
with a small change and no new component.

The classification in this ADR takes the crawler report of `source_url` as
true. A feed publisher cannot change that report. A holder of the token can.

## Decision

### 1. The source URL of a record

The stored `feeds.feed_url` is the **source URL** of the record. Only content
that the crawler fetched through the source URL can change the record. The
source URL changes only by ADR 0018 relocation, or by a move under ADR 0052.

A submission reaches the record through its source URL when `source_url` or
`canonical_url` of the submission is equal to the source URL. This covers a
redirect from the source URL to a CDN. It also covers a fetch of an old URL
that redirects to the source URL.

The comparison uses the exact stored string. This decision adds no URL
normalization. `http` and `https`, a trailing slash, the case of the path and
a query string can each name a different feed.

### 2. The node classifies each submission before it writes

The node classifies each authenticated submission with a feed body. `G` is
the declared GUID. `S` is `source_url` and `C` is `canonical_url`.

The node applies the first case that matches:

| Case | Condition | Result |
|---|---|---|
| Record conflict | `S` or `C` is the source URL of a record with a GUID that is not `G`, and a record has GUID `G` | Write nothing. Reason `record_conflict` |
| GUID change | `S` or `C` is the source URL of a record with a GUID that is not `G`, and no record has GUID `G` | Write nothing. Reason `guid_change_pending`. ADR 0052 owns what follows |
| Update | A record has GUID `G`, and its source URL is `S` or `C` | Apply the content to that record |
| Mirror | A record has GUID `G`, and its source URL is not `S` or `C` | Record the URL observation. Do not apply the content. Reason `source_conflict` |
| New feed | No record has GUID `G` | Admit a new record with `C` as its source URL |

ADR 0052 adds two exceptions to the mirror case: a permanent redirect from the
source URL, and an `itunes:new-feed-url` declaration at the source URL.

A mirror observation is a true RSS fact. ADR 0049 uses it to resolve a
publisher link. It gives no permission to change the record.

The record conflict case writes no observation. An observation for `S` or `C`
would change how ADR 0049 resolves a link to the other record.

### 3. The writer checks the classification again

The verifier chain runs on a reader connection (`src/api.rs:1637`). Two
submissions can pass it at the same time. The writer does the
classification of section 2 again in the transaction that applies the
content. The result of the writer is the final result.

All writes of one accepted submission go in one transaction. These writes
include the artist-credit rows. Today `get_or_create_feed_scoped_source_text_credit`
writes before the ingest transaction (`src/api.rs:1798`). A rejected
submission must leave no row.

### 4. Authentication comes first and cannot be removed

- The node checks `crawl_token` before any database read, before the verifier
  chain and before the hash shortcut.
- `crawl_token` is not a configurable verifier. `VERIFIER_CHAIN` holds quality
  rules only. A chain that names `crawl_token` fails at startup with a message
  that names this ADR.
- The primary does not start when `CRAWL_TOKEN` is empty or has only white
  space.

This narrows ADR 0005 and ADR 0015. They let an operator remove or reorder
any verifier. After this ADR, they cover quality rules only.

`force_reingest` stays available to the crawler. It skips only the hash
shortcut. It gives no exception to sections 1 to 4.

### 5. The response names the result

The node keeps the current envelope: HTTP `200`, `accepted: false` and a
`reason`. The current crawler reads this envelope as a final rejection and
does not retry it. The reasons are:

| Reason | Meaning | The crawler |
|---|---|---|
| `source_conflict` | The body is a mirror of a held record | Does not retry this URL. It can fetch the source URL |
| `record_conflict` | The submission touches two held records | Does not retry. An operator must decide |
| `guid_change_pending` | The source URL declares a new GUID | Does not retry. ADR 0052 owns the next step |

The response adds one optional field, `source_url`. It is present with
`source_conflict`, and it gives the source URL of the held record. The field
is public data. `GET /v1/feeds/{guid}` already returns it. ADR 0044 requires
the field in the OpenAPI document.

The crawler does not store `source_conflict`, `record_conflict` or
`guid_change_pending` as the node answer of ADR 0050. After a `304`, it
sends the kept body again. Thus an operator decision takes effect on the next
crawl without a changed body.

### 6. The repair of damaged records

The corrective pass of ADR 0047 reads the stored source URLs. After this ADR
is deployed, one pass with `--force --no-revalidate` fetches each source URL
and applies its body again. Only the source can write, so the pass restores
the content and the payment routes of each damaged record.

- `--no-revalidate` gets a fresh body. Without it, a `304` can give a kept
  body from before the repair.
- `--force` skips the hash shortcut. Without it, an unchanged source body
  gives `NO_CHANGE`, and the damaged content stays.

The operator keeps a backup of the database before the pass. The pass emits
normal signed events, so community nodes get the same repair.

A copied GUID at a mirror URL leaves a true observation. The repair keeps
it.

## Alternatives Considered

### Wait for the verification worker
ADR 0055 removes the trust in the crawler report. It needs a new component
and a new protocol. The takeover by a feed publisher needs neither. Waiting
leaves the payment routes of each feed open to any person on the internet.
Rejected.

### Reject a second URL for a held GUID with no observation
This breaks ADR 0049. Wavlake serves one album at two URL forms, and a
publisher link can name either form. The observation is needed for link
resolution. Rejected.

### Apply content from the newest submission of any URL
This is the current behavior. It is the defect. Rejected.

### Retire the old record when its source URL declares a new GUID
Retirement deletes rows with a cascade. A tool error that changes the GUID
for one fetch would then delete the record. ADR 0052 owns this case.
Rejected here.

## Consequences

- A feed publisher cannot change a record that it does not serve. The
  takeover of payment routes by a copied GUID stops.
- A holder of `CRAWL_TOKEN` can still write any record. It sends the stored
  source URL as `source_url`. ADR 0055 owns that attacker.
- A podping for a mirror URL no longer updates the record. The update comes
  on the next crawl of the source URL. The crawler can shorten the delay with
  the `source_url` field.
- A real move to a new host does not update the record until ADR 0052 is
  implemented. Today a move also does not change the stored URL, so the
  change in behavior is small.
- The Doerfelverse case gets `guid_change_pending`, not HTTP `500`, and the
  crawler stops the retries.
- Tests in `tests/adr0049_url_observation_tests.rs` that expect content from a
  second URL change. The tests that expect an observation stay.

## Invariants

- Content that changes a held record came through its source URL.
- A rejected submission writes no row except a URL observation in the mirror
  case.
- No configuration lets an unauthenticated submission read or write the
  database.

## Guards

Each guard has an incident: the takeover test of 2026-09-24, the
configuration probes of 2026-09-24 and the Doerfelverse `500`.

- A test submits a mirror body with a held GUID and different payment
  routes. The record, its tracks and its routes do not change. One
  observation is added. The failure message names ADR 0051 section 2.
- A test submits a redirect from the source URL to a new `canonical_url`. The
  content applies.
- A test submits a held URL with a new GUID. The response is
  `guid_change_pending`, not HTTP `500`, and no row changes.
- A test submits a held URL that declares the GUID of a different held
  record. Neither record changes, and no observation changes.
- A test starts the chain with `crawl_token` in `VERIFIER_CHAIN`. Startup
  fails. A test starts the primary with an empty `CRAWL_TOKEN`. Startup fails.
- A test rejects a submission after the artist-credit step would run. No
  artist-credit row remains.
