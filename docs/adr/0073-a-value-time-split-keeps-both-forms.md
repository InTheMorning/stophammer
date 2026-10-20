# ADR 0073: A Value Time Split Keeps Both Forms

## Status
Proposed on 2026-10-20, for the research request on value time splits. It
supersedes the section "`podcast:valueTimeSplit` with `remotePercentage`" of
ADR 0011.

## Date
2026-10-20

## Context

A `podcast:valueTimeSplit` tells a player that, from one time in a track and
for a duration, a part of the payment goes to other recipients. The
[specification](https://github.com/Podcastindex-org/podcast-namespace/blob/d4036dcb1047ad654682e1142734e3acf2c13f37/docs/tags/value-time-split.md)
at commit `d4036dc` permits two forms:

- **Remote form.** Exactly one `podcast:remoteItem` child. The optional
  `remotePercentage` gives the part that the recipients of the remote item get.
  Its default is 100. A value below 0 is 0, and a value above 100 is 100. The
  optional `remoteStartTime` gives the time in the remote item, with the
  default 0.
- **Local form.** One or more `podcast:valueRecipient` children.

`startTime` and `duration` are required, in seconds. The specification has no
`split` attribute on this element.

[The verification record](../reviews/research-crawler-compression-and-value-time-split-verification.md)
confirmed that `extract_value_time_splits` in stophammer-parser `9021262` drops
each split of both examples of the specification:

- It skips each split with `remotePercentage`. ADR 0011 decides this skip. Its
  reason is that a split with `remotePercentage` "instead of `split`" needs a
  revenue calculation at each play. The specification does not agree. It has no
  `split`, and `remotePercentage` is a fixed share.
- It skips each split in the local form.
- It skips a fractional `startTime`.
- It reads a `split` attribute that the specification does not have, with
  the default 0.

The production backup of 2026-10-19 has 0 rows in `value_time_splits`. One
feed in the index, The Satellite Skirmish, GUID
`b39fc3b2-2742-5944-a800-e87c42a093a3`, has three splits in the remote form.
The index keeps none of them, and gives no warning.

`IngestValueTimeSplit`, `model::ValueTimeSplit`, the event payload
`TrackUpsertedPayload` and the table `value_time_splits` hold one integer
`split` and one remote item. They cannot hold a percentage, a remote start time
or a list of recipients.

ADR 0044 §4 states that a field rename or a field removal in a `v1` response is
a breaking change.

## Decision

### 1. The parser keeps each split that the specification permits

The parser reads a `podcast:valueTimeSplit` in a `podcast:value` of an item.

- A split with one `podcast:remoteItem` child and a non-empty `feedGuid` is in
  the remote form.
- A split with one or more `podcast:valueRecipient` children is in the local
  form. Each recipient uses the rules of a track payment route.
- `startTime`, `duration` and `remoteStartTime` are decimal numbers of
  seconds. The parser keeps a fractional value.
- The parser drops a split with no readable `startTime`, or with no child of
  the two forms.
- The parser drops a split with a `remotePercentage` that is not a number. The
  default of 100 applies only when the source has no attribute.
- The parser ignores a `split` attribute on the element.

### 2. The index stores what the source states

The node stores the stated values, and does not apply a default or a limit in
the store:

| Value | Stored |
|---|---|
| `startTime` | The number |
| `duration` | The number, or null when the source has none |
| Form | `remote` or `local` |
| Remote `feedGuid`, `itemGuid` | The text, or null in the local form. `itemGuid` can be null |
| `remoteStartTime` | The number, or null when the source has none |
| `remotePercentage` | The number as stated, or null when the source has none |
| Local recipients | One row for each recipient, in source order, in a child table |
| Position | The order of the split in its `podcast:value` |

A migration rebuilds `value_time_splits` with these columns, and adds the
child table `value_time_split_recipients`. The child table has the columns of
`payment_routes` that describe a recipient. The migration copies each existing
row as the remote form, with a null `remotePercentage`.

### 3. The event payload carries both forms

`TrackUpsertedPayload` carries each split with the values of §2. A node reads
an older payload with the old fields. This is a change to the event protocol,
so community nodes upgrade first (ADR 0066).

The node is deployed before the crawler. A new node accepts the ingest of an
old crawler.

### 4. The `v1` response adds fields and removes none

The include `value_time_splits` keeps each field it has today, with the same
type:

- The list holds only the remote form, because `remote_feed_guid` is not
  nullable.
- `start_time_secs` and `duration_secs` give the stated value rounded down to
  an integer.
- `remote_item_guid` gives an empty string when the source states none.
- `split` gives the effective remote percentage: the stated value limited to
  0 to 100, or 100 when the source has none. Before this decision, no row held a value.

Each remote-form row adds `start_time`, `duration`, `remote_start_time` and
`remote_percentage`, with the stated values of §2. A null value means that the
source states none.

A new include, `local_value_time_splits`, gives the local form. Each row gives
`start_time`, `duration`, `position` and `recipients`. Each recipient has the
fields of a payment route.

The contract declares each field and the new include (ADR 0044, ADR 0071 §1).

### 5. ADR 0011

This section, with ADR 0072, supersedes the last two sections of ADR 0011 that
are in force. The other sections of ADR 0011 no longer describe the code:

| ADR 0011 section | Current owner |
|---|---|
| Runtime and XML parser | ADR 0017. The crawler and the parser are Rust |
| Separate process | ADR 0006 |
| Content hash strategy | ADR 0072 §4 |
| `podcast:valueTimeSplit` with `remotePercentage` | This ADR |
| A stateless crawler | ADR 0050 |
| No redirect resolution | ADR 0052 |
| Scheduling | ADR 0031 and ADR 0047 |

When this ADR and ADR 0072 are Accepted, ADR 0011 moves to `docs/adr/archive/`.

## Not Decided Here

- **A feed with no `podcast:guid`.** ADR 0011 says that the crawler submits
  an empty GUID, which the node rejects. The parser now gives an error, and the
  crawler submits nothing. No ADR states the current behavior. It needs an owner
  before ADR 0011 moves to the archive.
- **Route history.** ADR 0053 shows each change of the payment recipients of a
  feed. This ADR does not add the recipients of a split to that history.
- **A parse warning.** The parser has no channel for a warning. A dropped split
  stays silent.
- **Payment.** The index gives the source facts. A player computes the payment.

## Alternatives Considered

### Keep the skip

This needs no work. But the index drops each real split, and the reason in
ADR 0011 does not agree with the specification.

### Put the percentage in the `split` column

This is a small change, and it keeps the Satellite Skirmish splits. But the
local form, `remoteStartTime` and the fractional times stay lost. A column
named `split` then holds a value that is different from a recipient `split`.

### Store the raw element

Nothing is lost, and the model can wait. But a client cannot read a raw element
from the API, and the decision only moves to a later time.

### Change the `v1` fields

One list with nullable remote GUIDs and number times is simpler. But ADR 0044
§4 makes a removal a breaking change, and a client that decodes strict types
fails on a null or a fraction. The additive form of §4 breaks no client.

## Consequences

- The index keeps each split that the specification permits, in both forms.
- The change touches the parser, the crawler ingest, the node ingest, the
  event payload, the store, the apply step of a community node and the API.
- Community nodes upgrade before the primary.
- A crawl of the Satellite Skirmish feed after the deploy writes three
  remote-form rows.

## Guards

- The six parser tests of the research request, with assertions for this
  model. The control test `control_non_spec_split_attribute_parses` changes:
  the parser ignores `split`, and the split stays.
- A node test: an ingest with both forms writes the rows of §2, and the event
  gives them to a community node.
- A node test: an older event payload applies.
- A contract test: the response gives the fields of §4.
