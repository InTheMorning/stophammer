# Data Model and API

## Source-First V1

The current public read surface is source-first.

That means discovery and detail reads should start with:

- `feed`
- `track`

not with canonical `artist`, `release`, or `recording` entities. Those public
canonical routes were retired during Phase 3.

## Main Read Endpoints

Use:

- `/v1/search`
- `/v1/feeds/recent`
- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`
- `/v1/wallets/{id}`
- `/v1/publishers`
- `/v1/publishers/{publisher}`

## Provenance and RSS-Truth Views

Feed detail can include preserved source evidence:

- `remote_items`
- `publisher`
- `source_links`
- `source_ids`
- `source_contributors`
- `source_platforms`
- `source_release_claims`

`remote_items` preserves feed-level `podcast:remoteItem` declarations exactly as
published.

`publisher` is a derived view over those declarations:

- it reports direction and reciprocal validation from RSS
- it does not create canonical artist truth
- non-Wavlake `publisher_text` is only promoted after a reciprocal
  publisher/music pair exists
- Wavlake is the narrow compatibility exception where a linked publisher feed
  may supply artist text while stored `publisher_text` remains `"Wavlake"`

## Where the Evidence Lives

The source-first layer stores:

- feed rows
- track rows
- feed and track payment routes
- value time splits
- feed remote items
- contributor claims
- source IDs
- source links
- source release claims
- source item enclosures
- source platform claims

Some compatibility tables still exist internally, such as `artists` and
`artist_credit`, but they are no longer the public API model.

Wallet reads and publisher reads are inspection facets over the source-first
data. They do not add a canonical artist/release/recording layer.

For exact table meanings, read
[schema-reference.md](../schema-reference.md).

For exact route contracts, read
[API.md](../API.md).
