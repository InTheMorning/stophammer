# Artist Source Evidence

Stophammer's current public model is source-first. It stores feed and track
artist text plus preserved RSS evidence, but it does not expose a canonical
artist or contributor identity graph.

## Stored Artist Text

Feed rows store release-shaped artist text:

- `feeds.release_artist`
- `feeds.release_artist_sort`
- `feeds.image_url`

Track rows store item-shaped artist text:

- `tracks.track_artist`
- `tracks.track_artist_sort`
- `tracks.image_url`

`tracks.track_artist` is separate from `feeds.release_artist`, even when ingest
defaults it from the feed artist text. These are display strings from the source
layer, not stable artist identities.

## Stored Source Evidence

RSS contributor and identity evidence is preserved in source tables:

- `source_contributor_claims` stores `podcast:person` evidence:
  `name`, `role`, normalized role, group, `href`, `img`, source, extraction
  path, and observation time.
- `source_entity_ids` stores entity-level IDs such as
  `podcast:txt purpose="npub"` as `scheme = "nostr_npub"`.
- `source_entity_links` stores typed entity links such as websites and release
  pages.
- `source_release_claims` stores feed and item release-like facts, including
  feed artwork evidence.

These rows are attached to a feed or track entity. They are not resolved into a
global artist profile.

## Internal Compatibility Tables

The `artists`, `artist_credit`, and `artist_credit_name` tables still exist for
foreign keys, event compatibility, search, and transitional workflows. Ingest
creates feed-scoped source-text artist credits from published artist strings.

Do not treat those rows as canonical artist profiles. Ingest-created
source-text artists do not receive RSS `npub`, `podcast:person img`, or feed
artwork as `artists` table fields.

## Retrieval Without RSS Fetch

Clients can retrieve stored evidence without fetching and parsing RSS again:

- Feed artist text and image:
  `GET /v1/feeds/{feed_guid}`
- Feed npub, links, and contributor evidence:
  `GET /v1/feeds/{feed_guid}?include=source_ids,source_links,source_contributors`
- Track artist text and image:
  `GET /v1/feeds/{feed_guid}/tracks/{track_guid}`
- Track npub, links, and contributor evidence:
  `GET /v1/feeds/{feed_guid}/tracks/{track_guid}?include=source_ids,source_links,source_contributors`

Track contributor reads fall back to feed-level contributor claims when the
track has no track-level claims. The inherited rows keep their original
`entity_type` and `entity_id`, so clients can tell whether the evidence came
from the feed or the track.

## When RSS Fetch Is Still Required

Fetch and parse RSS if the client needs data Stophammer does not extract, or if
it needs a relationship that is not represented in the current schema.

In particular, Stophammer does not currently store:

- a canonical artist profile npub
- a canonical contributor profile npub
- a normalized mapping from one `podcast:person` row to one `npub`
- a global artist image distinct from feed, track, or contributor evidence

If an npub appears as `podcast:txt purpose="npub"`, Stophammer stores it as an
entity-level source ID for the feed or track. If a contributor image appears as
`podcast:person img`, Stophammer stores it on the contributor claim. Neither
one is promoted into a resolved artist or contributor identity.
