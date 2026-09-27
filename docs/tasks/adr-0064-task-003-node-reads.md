# ADR 0064 Task 003: The Reads Of Live Items

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) sections 3 and
6. Plan: [ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`.

## Goal

`GET /v1/feeds/{guid}` gives `live_items`. `GET /v1/live-items` gives the rows
of all feeds, with the views `now`, `upcoming` and `all`, the raw filters, and
`confirming_relay` from the setting `CONFIRMING_RELAY_HOSTS`.

## Files To Inspect

- `src/query.rs`: the feed response, a list route with `cursor` and `limit`,
  `response_schemas()`
- `src/openapi.rs`: `spec_value()`, `envelope_schema`
- `src/model.rs`: `web_url_or_none`
- `src/blocks.rs`: `read_csv_env`, for the setting
- `tests/adr0044_schema_refs_tests.rs`, `tests/adr0044_contract_guard_tests.rs`
- `docs/plans/adr-0064-live-item-cases.md`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`, `src/api.rs` (`build_router()`)
- `docs/API.md`, `docs/operations.md`, `api.html`
- `tests/adr0064_live_reads_tests.rs`, new

## Rules

- Each row gives the fields of section 6. `content_link` and a `uri` that is a
  URL go through `web_url_or_none`.
- `confirming_relay` is true only for an `https` `uri` on a host of
  `CONFIRMING_RELAY_HOSTS`. The setting is a comma-separated host list, empty
  by default.
- `view` defaults to `now`. The views follow the table of section 6, with a
  margin of 1 hour. The raw filters apply only with `view=all`.
- The list pages in the order of `feed_guid`, then `live_item_guid`.
- A row of a feed that is not public is in no read.
- `docs/API.md` gives the three views, what `confirming_relay` means, and how a
  client asks the relay.

## Acceptance

Mechanical, each an integration test:

- Each row of the case table is in the view that the table gives, and not in
  the others. Use a fixed clock.
- `now` gives a `live` row with no confirming relay until 1 hour after its
  `end`, and not after.
- A `uri` on a host that the setting does not list, and a `uri` that is only an
  identifier, give `confirming_relay: false`.
- A deleted feed gives no row.
- The guards of ADR 0044 pass. `cargo run --bin gen_openapi` gives the route.
- The gate is green.

Visual, for the operator:

- `api.html` shows the new route and the three views.
