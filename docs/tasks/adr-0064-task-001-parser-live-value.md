# ADR 0064 Task 001: The Parser Reads The Relay Link

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) section 3. Plan:
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer-parser`. The commit names `stophammer` ADR 0064.

## Goal

The parser reads `<podcast:liveValue uri="…" protocol="…"/>` on each
`<podcast:liveItem>`, and gives the two attributes in `IngestLiveItemData`.

## Files To Inspect

- `src/engine.rs`: `parse_live_item`
- `src/types.rs`: `IngestLiveItemData`
- The tests of `parse_live_item`

## Files Likely To Change

- `src/types.rs`: `live_value_uri: Option<String>` and
  `live_value_protocol: Option<String>`
- `src/engine.rs`: read the first `podcast:liveValue` child of the live item
- A test file for the live item

## Rules

- Keep each value as the feed gives it, with the white space trimmed. Do not
  change a `uri` that is only an identifier.
- A `liveValue` with no `uri` gives no link.
- Only a `liveValue` inside the live item counts. A `liveValue` on the channel
  or on a normal item does not.

## Acceptance

Mechanical:

- A live item with `liveValue` gives both fields.
- A live item with no `liveValue` gives `None` for both.
- A `uri` that is only an identifier stays as it is.
- The gate of this crate is green.
