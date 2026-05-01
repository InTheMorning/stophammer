# ADR 0041: Preserve Contributor Npub Source Evidence

## Status
Accepted

## Date
2026-05-01

## Context
Stophammer's v1 public model is source-first. It stores feed and track artist
display text plus preserved RSS evidence, but it does not resolve canonical
artist, contributor, or wallet identities.

Some music RSS feeds publish contributor-specific Nostr public keys directly on
`podcast:person`, for example:

```xml
<podcast:person
  img="https://files.heycitizen.xyz/Songs/HeyCitizen.jpg"
  npub="npub12um9zqae9uaydfszralpn0e0r90d559gd4qsrzar0j2yvut7t2zqwff5ck"
  group="music"
  role="musician">HeyCitizen</podcast:person>
```

The official Podcast Namespace `podcast:person` fields already include
contributor `href` and `img`, which Stophammer preserves in
`source_contributor_claims`. Without preserving the `npub` attribute on the same
claim row, applications cannot show the Nostr identity asserted for that exact
contributor occurrence without fetching and parsing RSS themselves.

## Decision
Accept `npub` as a valid source-evidence attribute on `podcast:person`.

Store it on the existing `source_contributor_claims` row as nullable source
evidence:

- feed-level `podcast:person npub` is attached to the feed contributor claim
- track-level `podcast:person npub` is attached to the track contributor claim
- live-item `podcast:person npub` is attached to the live-item contributor claim

Expose the field through existing `source_contributors` API includes.

This is row-scoped evidence only. Stophammer must not infer that the same npub,
name, image, wallet, or website across feeds/tracks is the same person or
artist.

## Alternatives Considered

### New contributor ID table

A separate `source_contributor_ids` table would allow multiple contributor IDs
per person row. It is more general, but it adds a second evidence collection and
more complex API joins before there is a second contributor-specific ID syntax
to preserve.

### Treat `podcast:person href` as enough

This does not preserve the feed's explicit Nostr key assertion when the feed
publishes `npub` separately from `href`.

### Promote npub into `artists`

Rejected. Internal `artists` rows are compatibility records, not canonical
artist profiles, and this project explicitly does not link contributors,
wallets, and artists.

## Consequences

- Apps can display contributor faces, websites, and npubs from stored API data.
- Community nodes receive contributor npubs through the existing signed
  source-contributor event payload.
- Existing data remains valid because the new column is nullable.
- Future contributor-specific IDs can still be added later with a generalized
  evidence table if another source syntax requires it.

## Invariants

- Contributor npub evidence is scoped to one source contributor claim row.
- Stophammer does not merge contributor claims by name, npub, image, website, or
  payment route.
- Stophammer does not infer artist identity from contributor npub evidence.
