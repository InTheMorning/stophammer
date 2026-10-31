# Phase Plan: A Funding Link Is A Source Link

Owner: [ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md). This plan
states no rule. It gives the sequence of the build, the release and the refill
pass. Delete it when release 0.18.0 is deployed and the refill check passes.

## Goal

Each `podcast:funding` tag of a channel, an item or a live item is a
`source_links` row of the type `funding`. Its node value is the `label` of
the row. The `v1` feed route and track route give `label` on each link row. A forced
`refresh` pass fills the rows of the feeds that the index holds.

## Non-Goals

- A new route, parameter or include.
- A search filter, or index text from a funding link.
- A display rule for an application.
- `source_links` on the live item routes of ADR 0064.
- A change of `podcast:value` or of the payment routes.

## Assumptions

- The operator accepted ADR 0082 with the proposed values on 2026-10-31.
- The parser and the node are independent crates. The crawler sends the parser
  types, so the crawler changes only its version and its parser dependency.
- An event keeps its `payload_json` as signed. A new optional field with
  `skip_serializing_if` changes no stored payload and no signature.
- Release 0.17.0 is deployed. The next release is 0.18.0 (ADR 0066 §4, a new
  field).

## Affected Modules

| Repository | File | Change |
|---|---|---|
| `stophammer-parser` | `src/types.rs` | `IngestLink.label` |
| `stophammer-parser` | `src/engine.rs` | `extract_links` reads `podcast:funding` |
| `stophammer` | `src/ingest.rs`, `src/model.rs`, `src/event.rs` | `label` on the link types. The payload carries it |
| `stophammer` | `migrations/0058_source_link_label.sql`, `src/schema.sql` | the column `label` |
| `stophammer` | `src/db.rs` | the link reads, the write, the compare |
| `stophammer` | `src/api.rs` | the source `podcast_funding`, the label |
| `stophammer` | `src/query.rs` | `SourceEntityLinkResponse.label` |
| `stophammer` | `tests/fixtures/v1-schema-fields.json`, `tests/fixtures/migration_sha256.txt` | the guards |
| `stophammer` | `docs/API.md`, `docs/schema-reference.md` | the documents |
| `stophammer-crawler` | `Cargo.toml`, `Cargo.lock` | the version only |

## Sequence

1. [Task 001](../tasks/adr-0082-task-001-parser-funding-links.md): the
   parser gives each funding tag as a link with a label.
2. [Task 002](../tasks/adr-0082-task-002-node-link-label-storage.md): the
   node stores the label and the source `podcast_funding`, and a label change
   gives an event.
3. [Task 003](../tasks/adr-0082-task-003-link-label-response-and-documents.md):
   the response gives `label`, the contract guards hold it, and the documents
   describe it.
4. [Task 004](../tasks/adr-0082-task-004-release-candidate.md): release
   candidate 0.18.0.
5. [Task 005](../tasks/adr-0082-task-005-deploy-refill-and-record.md): the
   deploy, the refill pass of ADR 0077, its check and the record.

Tasks 001 and 002 are independent, and can run at the same time. Task 003
needs task 002. Task 004 needs the commits of tasks 001 to 003. Task 005 needs
the candidate.

## Schema And API Implications

- One migration adds the column `label TEXT` to `source_entity_links`. No
  index, no fill. Existing rows keep a null label.
- `SourceEntityLink` and the payload of `SourceEntityLinksReplaced` get the
  optional field. A node of 0.17.0 applies a payload with the field and drops
  the label.
- `SourceEntityLinkResponse` gets `label`. No field is removed or renamed, so
  the guards of the archived ADR 0044 need only the new fixture entry.
- The release is MINOR (ADR 0066 §4). The three repositories get 0.18.0
  together (ADR 0066 §3).

## Risk Areas

- **A second storage path.** A table or a field only for funding would split
  the links. The review checks that each change goes through `source_links`.
- **The scheme rule.** `entity_link_response` gives `website` and `web_page`
  the rule of ADR 0054 §4a. A `funding` row must take the `_` arm. A test
  holds the no-scheme case.
- **A dropped label on a community node.** The release notes tell a community
  operator to upgrade before the refill pass.
- **The refill cost.** A forced `refresh` fetches each held feed one time,
  approximately 8,500 fetches at the host delay. The pass of the archived
  ADR 0058 §1c had the same form.
- **The migration version.** The runner skips a migration whose version an
  existing database recorded. `migration_tests` and the deploy log show it.

## Test Strategy

Each criterion of the ADR "Verification" section is a test in its crate. The
parser tests use inline XML, as `tests/adr0069_link_provenance_tests.rs` does.
The node tests post an ingest with links, as
`tests/adr0054_host_name_link_tests.rs` does, and read the feed route with
`include=source_links`. The gate of each crate runs before each commit.

## Rollback Strategy

A rollback of release 0.18.0 runs `deploy.sh --release 0.17.0`. The column
`label` stays in the database. The statements of 0.17.0 name their columns, so
the extra column changes no read and no write. The migration table records
version 58. Whether the runner of 0.17.0 accepts a recorded version above its
own list is not examined. When the node of 0.17.0 does not start, restore the
backup of the deploy, as `docs/operations.md` "Restore" gives.

## After The Tasks

- `AGENTS.md` says that release 0.18.0 is deployed and that the refill check
  passed. Until the check passes, "Open Items" names the pass (ADR 0077 §2).
- `docs/reviews/adr-0082-refill-measurement.md` keeps the counts of the pass.
- This plan, the review checklist and tasks 001 to 005 are deleted.
