# Current Decisions

Each ADR in this directory constrains a change today. This file is the answer
to what binds a change now.

`AGENTS.md` and every other document restate these rules and name the owner.
They do not create a rule. ADR 0045 owns this index and the governance model.

Status values come from the ADR file itself. This index does not judge a
status. See "Status Needs A Check" for the rows where the file and the code
disagree.

## Governance

| ADR | Scope | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | ADRs record decisions. Sequential numbering, Nygard structure | Accepted |
| [0045](0045-governance-model-and-contract-ownership.md) | Adopts v4vmm ADR 0061. Names the MusicIndex API owner. Covers the three repositories | Proposed |
| [0046](0046-migration-versions-are-array-positions.md) | A migration version is its array position. A skipped migration needs a repair, and the runner reports the condition | Accepted |

## Foundation And Storage

| ADR | Scope | Status |
|---|---|---|
| [0002](0002-rust-static-binary-core.md) | Rust for the core node binary | Accepted |
| [0003](0003-sqlite-wal-primary-store.md) | SQLite with WAL mode as the primary store | Accepted |
| [0023](0023-schema-migrations.md) | Versioned schema migrations | Accepted |
| [0024](0024-sqlite-wal-connection-pool.md) | SQLite WAL connection pool | Accepted |
| [0025](0025-source-claims-and-canonical-music-layers.md) | Source claims and canonical music layers | Accepted |
| [0034](0034-adopt-rebuild-first-source-first-v1-music-schema.md) | Rebuild-first source-first v1 music schema | Proposed |
| [0040](0040-store-track-identity-as-feed-scoped.md) | Track identity stored as feed-scoped | Accepted |
| [0041](0041-contributor-npub-source-evidence.md) | Contributor npub kept as source evidence | Accepted |

## Ingest, Parser And Crawlers

| ADR | Scope | Status |
|---|---|---|
| [0005](0005-pluggable-verifier-chain.md) | Pluggable verifier chain for feed ingest | Accepted |
| [0006](0006-crawlers-as-untrusted-clients.md) | Crawlers are separate, untrusted HTTP clients | Accepted |
| [0011](0011-rss-crawler.md) | RSS crawler implementation | Accepted |
| [0015](0015-verifier-plugin-architecture.md) | Verifier plugin architecture | Accepted |
| [0017](0017-canonical-rss-parser-crate.md) | Canonical RSS parser crate | Accepted |
| [0021](0021-live-events.md) | Live event support. Scheduler follow-up remains | Accepted |
| [0030](0030-podcastindex-importer-durable-attempt-memory.md) | PodcastIndex importer durable attempt memory | Accepted |
| [0031](0031-archive-backed-gossip-with-feed-memory.md) | Archive-backed gossip with durable feed memory | Accepted |
| [0033](0033-music-first-import-cursor-and-conditional-snapshot-refresh.md) | Music-first import cursor and conditional snapshot refresh | Proposed |
| [0047](0047-a-corrective-pass-reads-the-index.md) | A corrective pass takes its corpus from the node's feed list | Accepted |
| [0038](0038-item-level-publisher-remote-items.md) | Item-level `remoteItem` extraction and non-music filter. Wavlake caveat superseded by ADR 0049 | Proposed |
| [0043](0043-feed-publication-date-records-its-source-element.md) | A feed publication date records its source element | Accepted |
| [0048](0048-every-track-resolves-to-a-payment-route.md) | The V4V gate is track coverage, not a feed-level block | Accepted |
| [0050](0050-the-crawler-revalidates-a-feed.md) | The crawler sends a conditional GET, keeps the last body, and uses it after a `304` | Accepted |
| [0049](0049-publisher-relationships-are-rss-facts.md) | A publisher relationship is a set of RSS facts. The crawler follows publisher links, and the node resolves a back-link by GUID or by an observed URL. Replaces the Wavlake exception of ADR 0035 | Accepted |

## HTTP API And Contract

| ADR | Scope | Status |
|---|---|---|
| [0022](0022-fts5-search-architecture.md) | FTS5 contentless search with a companion table | Accepted |
| [0037](0037-defer-public-sse-route.md) | The public SSE route is deferred | Accepted |
| [0039](0039-feed-scoped-track-identity-routes.md) | Feed-scoped public track identity routes | Accepted |
| [0042](0042-query-responses-name-the-field-owner.md) | A query response field names its owner | Accepted |
| [0044](0044-api-contract-declares-its-fields.md) | The API contract declares its fields. A `v1` rename needs a version | Accepted |

## Identity, Signing And Security

| ADR | Scope | Status |
|---|---|---|
| [0004](0004-signed-event-log.md) | Nostr-style ed25519 signed event log. Sequence-number signing superseded by ADR 0036 | Accepted in part |
| [0026](0026-signed-peer-registration.md) | Signed peer registration | Accepted |
| [0027](0027-authenticate-sync-read-endpoints.md) | Authenticate sync read endpoints | Accepted |
| [0028](0028-require-dedicated-sync-token.md) | A dedicated sync token is required | Accepted |
| [0036](0036-sign-event-sequence-numbers.md) | Sign event sequence numbers | Accepted |
| [0051](0051-feed-content-comes-from-its-source-url.md) | Only content from the stored source URL changes a feed record. Authentication is first and cannot be removed | Proposed |
| [0052](0052-a-source-moves-its-own-feed.md) | A feed moves by a permanent redirect, `itunes:new-feed-url` or its self link at its source URL, or by the operator. A GUID change at the source URL links the old record | Proposed |
| [0053](0053-a-correction-stays-applied.md) | A block is a signed, replicated fact. An older copy does not replace a newer copy. Payment-recipient changes are visible | Proposed |
| [0054](0054-a-fetch-reaches-only-public-feed-hosts.md) | Each fetch of a URL from RSS or a podping reaches only public addresses, with body, redirect and follow limits | Proposed |
| [0057](0057-a-feed-can-block-this-index.md) | A `podcast:block` at the source URL retires the feed, with no durable block. The slug of this index is `musicindex` | Accepted |
| [0056](0056-the-public-proof-flow-is-offline.md) | The public proof flow is removed. Each write route needs the admin token. Supersedes ADR 0018 | Accepted |
| [0055](0055-the-primary-fetches-what-it-signs.md) | Crawlers nominate URLs. A primary-controlled fetch worker is the only source of ingest content | Proposed |

## Nodes, Replication And Deployment

| ADR | Scope | Status |
|---|---|---|
| [0008](0008-cloudflare-tracker-implementation.md) | Cloudflare Workers tracker implementation. Supersedes ADR 0007 | Accepted |
| [0009](0009-community-node-mode.md) | Community node mode. Sequence-signing consequence superseded by ADR 0036 | Accepted in part |
| [0010](0010-distribution-and-deployment.md) | Distribution and deployment | Accepted |
| [0016](0016-push-gossip-tracker-elimination.md) | Push-based gossip. Tracker elimination | Accepted |
| [0019](0019-tls-acme-let-s-encrypt.md) | TLS through ACME and Let's Encrypt. Three-tier node model | Accepted |
| [0032](0032-retire-resolver-and-review-runtime.md) | Retire the resolver and the review runtime | Proposed |

## Superseded

These stay in this directory until ADR 0045 creates `archive/`. Read them for
research, not for a live rule.

| ADR | Scope | Superseded by |
|---|---|---|
| [0007](0007-cloudflare-tracker-bootstrap.md) | Cloudflare Workers as tracker and bootstrap layer | ADR 0008 |
| [0012](0012-podping-listener.md) | Podping listener for real-time music feed discovery | The gossip mode in `stophammer-crawler` |
| [0013](0013-bulk-importer.md) | PodcastIndex bulk importer | ADR 0030 |
| [0014](0014-artist-resolution-aliases.md) | Artist resolution, alias table, merge operation, admin endpoints | ADR 0032 and ADR 0034 |
| [0020](0020-sse-artist-follow-notifications.md) | SSE push notifications for artist follow | ADR 0037 and ADR 0036 |
| [0029](0029-primary-resolved-replication-authority.md) | Primary resolver authority for replicated read models | ADR 0032 |
| [0018](0018-proof-of-possession-mutations.md) | Proof-of-possession for feed and track mutations | ADR 0056 |
| [0035](0035-add-track-level-publisher-text.md) | Track-level publisher text | ADR 0049 for the Wavlake clause. Tests enforce the rest: `tests/api_canonical_query_tests.rs` and `tests/adr0049_text_field_tests.rs` |

## Status Needs A Check

A recorded status below disagrees with the code at commit `a220f44`. This index
records what each file states. Correcting a file is a separate change.

| ADR | File says | The code shows |
|---|---|---|
| [0032](0032-retire-resolver-and-review-runtime.md) | Proposed | No resolver module in `src/`, and no `stophammer-resolver` directory. ADR 0029 already names ADR 0032 as its successor |
| [0034](0034-adopt-rebuild-first-source-first-v1-music-schema.md) | Proposed | `migrations/0025` and `migrations/0032` are merged, and the seven `source_*` tables exist |
| [0033](0033-music-first-import-cursor-and-conditional-snapshot-refresh.md) | Proposed | Both halves are in `stophammer-crawler`. `music_first_lower_bound` in `src/modes/import.rs` gives the cursor, and `format_if_modified_since_value` gives the conditional refresh |

Four files also use a different status format. ADR 0035, ADR 0038, ADR 0039 and
ADR 0040 give `- Status:` in a list, and the rest use a `## Status` heading.

## ADR Workflow

This repo keeps architecture decision records in `docs/adr/`.

Use the ADR guidance and tooling curated at:

- https://adr.github.io/
- https://adr.github.io/adr-tooling/

For this repo, use the official Nygard-style
[`adr-tools`](https://github.com/npryce/adr-tools) command-line tool rather
than local custom scripts.

Install guidance from upstream:

- https://github.com/npryce/adr-tools/blob/master/INSTALL.md

Typical workflow:

```bash
adr help
adr list
adr new "Your decision title"
adr new -s 24 "Your replacement decision"
```

Notes for this repo:

- `.adr-dir` points `adr-tools` at `docs/adr/`
- ADRs live in `docs/adr/`, not the upstream default `doc/adr/`
- `docs/adr/templates/template.md` overrides the upstream default so new ADRs
  match this repo's current `# ADR NNNN: ...` plus `## Status` formatting
- the existing ADR set predates this standardization and is slightly mixed in
  formatting
- new ADRs should follow the `adr-tools` / Nygard shape unless we explicitly
  migrate to a different template family later
