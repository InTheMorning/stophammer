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
| [0045](0045-governance-model-and-contract-ownership.md) | Adopts v4vmm ADR 0061. Names the MusicIndex API owner. Covers the three repositories | Accepted |
| [0046](0046-migration-versions-are-array-positions.md) | A migration version is its array position. A skipped migration needs a repair, and the runner reports the condition | Accepted |

## Foundation And Storage

| ADR | Scope | Status |
|---|---|---|
| [0002](0002-rust-static-binary-core.md) | Rust for the core node binary | Accepted |
| [0003](0003-sqlite-wal-primary-store.md) | SQLite with WAL mode as the primary store | Accepted |
| [0023](0023-schema-migrations.md) | Versioned schema migrations | Accepted |
| [0024](0024-sqlite-wal-connection-pool.md) | SQLite WAL connection pool | Accepted |
| [0025](0025-source-claims-and-canonical-music-layers.md) | Source claims and canonical music layers. The canonical layers are superseded by ADR 0034 | Accepted in part |
| [0034](0034-adopt-rebuild-first-source-first-v1-music-schema.md) | Rebuild-first source-first v1 music schema. Search and quality stay. The compatibility artist credit leaves in two releases, §11 | Accepted |
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
| [0064](0064-a-live-item-is-an-rss-fact.md) | A live item is an RSS fact. The relay owns the real-time path, and the index gives the relay link. No poll, no recording. A client reads live items per feed and across feeds. Supersedes ADR 0021 | Accepted. §6 amended on 2026-10-01: `in_now_view` and `in_upcoming_view` on each row |
| [0030](0030-podcastindex-importer-durable-attempt-memory.md) | PodcastIndex importer durable attempt memory. A parse error, or a node `413` for a feed with no medium, waits 7 days in the shared skip list | Accepted |
| [0031](0031-archive-backed-gossip-with-feed-memory.md) | Archive-backed gossip with durable feed memory | Accepted |
| [0033](0033-music-first-import-cursor-and-conditional-snapshot-refresh.md) | Music-first import cursor and conditional snapshot refresh. A refresh keeps its disk | Accepted |
| [0047](0047-a-corrective-pass-reads-the-index.md) | A corrective pass takes its corpus from the node's feed list | Accepted |
| [0038](0038-item-level-publisher-remote-items.md) | Item-level `remoteItem` extraction and non-music filter. `/v1/feeds/recent` keeps `medium`, music by default. Wavlake caveat superseded by ADR 0049 | Accepted |
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
| [0061](0061-a-publisher-read-counts-its-listed-artists.md) | A publisher read gives its confirmed and unconfirmed artists. An album shows only the publishers it names. The index derives no artist or label kind | Accepted |
| [0068](0068-a-publisher-row-gives-its-link-facts.md) | A publisher row of `GET /v1/feeds/recent` gives `two_way_link_count`, `stated_rels` and `confirmed_release_artists`. Opt-in with `include=link_facts` | Accepted |

## Identity, Signing And Security

| ADR | Scope | Status |
|---|---|---|
| [0004](0004-signed-event-log.md) | Nostr-style ed25519 signed event log. Sequence-number signing superseded by ADR 0036 | Accepted in part |
| [0026](0026-signed-peer-registration.md) | Signed peer registration | Accepted |
| [0027](0027-authenticate-sync-read-endpoints.md) | Authenticate sync read endpoints | Accepted |
| [0028](0028-require-dedicated-sync-token.md) | A dedicated sync token is required | Accepted |
| [0036](0036-sign-event-sequence-numbers.md) | Sign event sequence numbers | Accepted |
| [0051](0051-feed-content-comes-from-its-source-url.md) | Only content from the stored source URL changes a feed record. Authentication is first and cannot be removed | Accepted |
| [0052](0052-a-source-moves-its-own-feed.md) | A feed moves by a permanent redirect, `itunes:new-feed-url` or its self link at its source URL, or by the operator. A GUID change at the source URL is public, automatic only for the UUIDv5 of the source URL, and otherwise needs the operator | Accepted |
| [0053](0053-a-correction-stays-applied.md) | A block is a signed, replicated fact. An older copy does not replace a newer copy. Payment-recipient changes are visible | Accepted |
| [0054](0054-a-fetch-reaches-only-public-feed-hosts.md) | Each fetch of a URL from RSS or a podping reaches only public addresses, with body, redirect and follow limits. A non-web URL field is kept raw and not served | Accepted |
| [0059](0059-an-entry-that-names-a-feed-gives-its-summary.md) | Each `publisher` and `remote_items` entry gives the title, the image and the artist of the feed it names, as `remote_*` fields | Accepted. §5 amended on 2026-10-01: an entry that names a track gives its summary |
| [0060](0060-a-list-feed-keeps-its-items.md) | A `musicL` feed keeps `itemGuid`, `title` and its value block. Each entry gives its indexed track. The crawler follows a list, at most 1,000 URLs. Amends ADR 0049 §2 and ADR 0054 §3 | Accepted |
| [0062](0062-a-podping-is-never-dropped.md) | The gossip mode merges a podping inside the window of its URL, and never drops it. The window is 30 seconds, and doubles to at most 1 hour for a URL whose crawl changes nothing. `live` and `liveEnd` are not delayed | Accepted |
| [0058](0058-a-copy-of-a-feed-is-public.md) | A mirror body with different tracks or payment routes is a public copy. The operator keeps the source or relocates. A relocation clears `last_build_date` and `declared_self_url`. At most 20 rows for each GUID. A copy row gives its item titles and image (§1c) | Accepted |
| [0057](0057-a-feed-can-block-this-index.md) | A `podcast:block` at the source URL retires the feed, with no durable block. The slug of this index is `musicindex` | Accepted |
| [0067](0067-a-gone-source-retires-its-feed.md) | Two gone answers from the source URL, 24 hours apart, retire the feed. A `404` counts only from a host in `SOURCE_GONE_HOSTS`, a `410` from any host | Accepted |
| [0056](0056-the-public-proof-flow-is-offline.md) | The public proof flow is removed. Each write route needs the admin token. Supersedes ADR 0018 | Accepted |
| [0055](0055-the-primary-fetches-what-it-signs.md) | Crawlers nominate URLs. A primary-controlled fetch worker is the only source of ingest content. Waits for a crawler outside the primary host, or a suspected token leak | Proposed |

## Nodes, Replication And Deployment

| ADR | Scope | Status |
|---|---|---|
| [0008](0008-cloudflare-tracker-implementation.md) | Cloudflare Workers tracker implementation. Supersedes ADR 0007 | Accepted |
| [0009](0009-community-node-mode.md) | Community node mode. Sequence-signing consequence superseded by ADR 0036 | Accepted in part |
| [0016](0016-push-gossip-tracker-elimination.md) | Push-based gossip. Tracker elimination | Accepted |
| [0066](0066-a-version-number-tells-what-to-upgrade.md) | Semantic versions. PATCH for fixes, MINOR for additions and, before 1.0, for breaks. The notes say when community nodes upgrade first | Accepted |
| [0065](0065-a-release-promotes-its-candidate.md) | Only a candidate builds, and only on a commit that passed CI. A release tag publishes the files and images of its candidate on the same commits | Accepted |
| [0063](0063-a-release-publishes-role-packages.md) | A release is one tag in each repository. It publishes role tarballs, Arch packages and images. Supersedes ADR 0010 | Accepted |
| [0019](0019-tls-acme-let-s-encrypt.md) | TLS through ACME and Let's Encrypt. Three-tier node model | Accepted |
| [0032](0032-retire-resolver-and-review-runtime.md) | Retire the resolver and the review runtime | Accepted |

## Superseded

These files are in `docs/adr/archive/`. Read them for research, not for a live
rule.

| ADR | Scope | Superseded by |
|---|---|---|
| [0007](archive/0007-cloudflare-tracker-bootstrap.md) | Cloudflare Workers as tracker and bootstrap layer | ADR 0008 |
| [0012](archive/0012-podping-listener.md) | Podping listener for real-time music feed discovery | The gossip mode in `stophammer-crawler` |
| [0013](archive/0013-bulk-importer.md) | PodcastIndex bulk importer | ADR 0030 |
| [0014](archive/0014-artist-resolution-aliases.md) | Artist resolution, alias table, merge operation, admin endpoints | ADR 0032 and ADR 0034 |
| [0020](archive/0020-sse-artist-follow-notifications.md) | SSE push notifications for artist follow | ADR 0037 and ADR 0036 |
| [0029](archive/0029-primary-resolved-replication-authority.md) | Primary resolver authority for replicated read models | ADR 0032 |
| [0018](archive/0018-proof-of-possession-mutations.md) | Proof-of-possession for feed and track mutations | ADR 0056 |
| [0010](archive/0010-distribution-and-deployment.md) | Distribution and deployment | ADR 0063 |
| [0021](archive/0021-live-events.md) | Live event support | ADR 0064 |
| [0035](archive/0035-add-track-level-publisher-text.md) | Track-level publisher text | ADR 0049 for the Wavlake clause. Tests enforce the rest: `tests/api_canonical_query_tests.rs` and `tests/adr0049_text_field_tests.rs` |

## Status Needs A Check

On 2026-09-27 the operator decided ADR 0032, ADR 0033 and ADR 0034. No
recorded status disagrees with the code now.

Three files also use a different status format. ADR 0035, ADR 0039 and ADR 0040
give `- Status:` in a list, and the rest use a `## Status` heading.

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
