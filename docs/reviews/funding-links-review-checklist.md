# Review Checklist: A Funding Link Is A Source Link

The orchestrator reviews each diff of
[ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md) against this list.
The ADR owns each rule. This list only names what to examine. The plan is
[the funding links plan](../plans/funding-links-phase-plan.md).

## Each Task

Reviewed on 2026-10-31, on the working tree before the commit.

| Task | Pass or fail | Required fixes | Merge |
|---|---|---|---|
| 001, the parser | Pass | None. The crawler gate also passed against the changed parser: 341 tests, clippy and fmt. | Yes |
| 002, the storage and the events | Pass | None. The packet missed the `MIGRATIONS` list, a second insert in `ingest_transaction` and the version assertions of `migration_tests`. The agent changed each one. | Yes |
| 003, the response and the documents | Pass, with one fix by the orchestrator | The `docs/API.md` sentence on `entity.podcast:funding` named only a track. It now names a track or a live item, because the feed include lists live item rows too. | Yes |

## Rules Of The ADR

- §1, the tag: `extract_links` reads `podcast:funding` in the podcast
  namespace only, in the channel, each item and each live item. The URL comes
  from the attribute `url`, and a tag with no usable URL gives no link.
- §1, the row: the label is the node value with the white space at its ends
  removed, and null when empty. No text is cut. The extraction paths are
  `feed.podcast:funding` and `entity.podcast:funding`. The position keeps the
  source sequence.
- §2: `label` is optional on `IngestLink` of the parser and of the node, on
  `SourceEntityLink` and in the table. `SourceEntityLink` has
  `skip_serializing_if`, so a payload with no label keeps its bytes. The
  compare function reads `label`. The dedupe keeps the first row of a key.
- §3: the `api.rs` mapping gives `podcast_funding` only to the type `funding`.
  Each other type keeps `rss_link`.
- §4: `SourceEntityLinkResponse` gives `label`, and `entity_link_response`
  copies it. The `website` and `web_page` arm changes not, and a `funding`
  row takes the `_` arm. No route, parameter or include is added.
- §5: no search change, no live route change, no payment route change.

## Conventions

- The parser denies `clippy::pedantic`. The node uses the lint set of its
  `Cargo.toml`. Each `#[expect]` has a reason.
- The migration is additive, and `src/schema.sql` matches the union of the
  migrations. The hash fixture has the new line.
- Each new field has a doc comment that names the ADR section.
- Each changed document passes the STE checker with no sentence-length or
  paragraph-length result on the new text.
- No test reaches the network.

## Drift To Look For

- A table, an event type or a response field only for funding.
- A cut of the label at 128 characters.
- The §4a rule applied to a `funding` row, or a `funding` row with
  `url_scheme_added` true.
- A change of the source `rss_link` for a `website`, `web_page`, `self_feed`
  or `content_stream` row.
- A change of `src/profile.rs`, `src/rule.rs` or `src/phase.rs` in the parser.
- `deny_unknown_fields` on a payload type.
- A document that names `label` as required, or that gives a funding link a
  display rule.

## Merge Recommendation

The three tasks passed on 2026-10-31. The orchestrator records:

1. The gate is green in each crate. The parser passes its 65 tests, and the
   crawler passes its 341 tests against the changed parser. The node passes
   its full test run, clippy and fmt. `gen_openapi` gives `label` in
   `SourceEntityLinkResponse`.
2. The parser and the node can release together as 0.18.0 (ADR 0066 §3). The
   crawler changes only its version and its parser dependency.
3. The count of cache bodies with the tag is not known before the release.
   Task 005 gives it.
