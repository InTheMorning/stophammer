# Data Model and API

## Canonical First

The public read surface is canonical-first.

That means discovery should usually start with:

- `artist`
- `release`
- `recording`

not with individual source feeds or source tracks.

## Why

The same music can appear on multiple source platforms:

- Wavlake
- RSS Blue
- Fountain
- direct RSS mirrors

If search returned raw source rows by default, clients would see duplicates.
Canonical-first search avoids that by returning one merged release or recording
and then letting the client drill into source/platform variants underneath.

## Main Read Endpoints

Use:

- `/v1/search`
- `/v1/recent`
- `/v1/artists/{id}/releases`
- `/v1/releases/{id}`
- `/v1/recordings/{id}`
- `/v1/releases/{id}/sources`
- `/v1/recordings/{id}/sources`

## Source Detail Endpoints

Use these when you need provenance or source claims:

- `/v1/feeds/{guid}`
- `/v1/tracks/{guid}`
- `/v1/artists/{id}/resolution`
- `/v1/releases/{id}/resolution`
- `/v1/recordings/{id}/resolution`

## Where the Evidence Lives

The staged source layer stores:

- contributor claims
- source IDs
- source links
- release claims
- platform claims
- item enclosures

The canonical layer stores:

- artists
- releases
- recordings
- mappings from source feed/item rows into canonical entities

For the exact table meanings, read
[schema-reference.md](/home/citizen/build/stophammer/docs/schema-reference.md).

For the exact route contracts, read
[API.md](/home/citizen/build/stophammer/docs/API.md).
