# ADR 0014: Artist Resolution — Alias Table, Merge Operation, and Admin Endpoints

## Status
Accepted

## Context

The original `resolve_artist` implementation matched artists by a single case-insensitive `name_lower` column. This caused two classes of defect:

1. **False merges** — two unrelated artists with the same display name (e.g. "Arcade Fire" the band and "Arcade Fire" on a tribute album) would be silently merged into one artist record.
2. **Split identities** — "Taylor Swift" and "Taylor Swift (feat. Ed Sheeran)" produced separate artist rows even though the latter clearly refers to the same canonical artist.

The simple approach was intentional pending real collision data. With enough feed ingestion we now have evidence that both problems occur in production and manual corrections are needed.

## Decision

### Alias table

A new `artist_aliases` table stores `(alias_lower, artist_id)` pairs. The lookup path becomes:

1. Check `artist_aliases.alias_lower` — alias match wins.
2. Fall back to `artists.name_lower` — handles rows that predate the alias table; back-fills the alias row on hit.
3. If no match: insert new artist row and insert the canonical name as its first alias.

**Why decouple canonical name from lookup keys?**
The `artists.name` column records the display name as it appeared in the feed that first created the record. The alias table records every name string that should resolve to that artist. These two concerns are independent: a merge should not require rewriting the display name, and a display-name correction should not break lookup.

**Why auto-register the canonical name as an alias?**
Merge becomes simpler. Every artist has at least one alias row, so `merge_artists` can transfer all aliases with a single `UPDATE … WHERE artist_id = :source` without needing a special case for the primary name.

### `db::merge_artists` — human-triggered, not automatic

`merge_artists` repoints all feeds and tracks from the source artist to the target, transfers non-conflicting aliases, then deletes the source artist row. It operates inside a transaction so the merge is atomic.

**Why manual, not automatic?**
Deciding whether two artists with the same name are the same entity requires human judgment. Automatic merging on name collision would cause data loss (false merges). The admin endpoint exists to let a human apply the merge after verifying the artists are the same.

### `ArtistMerged` event

A new `ArtistMerged` event type is emitted after a successful merge. The payload records both artist IDs and the list of alias strings transferred so the operation is fully auditable in the event log and can be replayed on community nodes.

Community nodes apply `ArtistMerged` events by calling `db::merge_artists` during the sync loop, keeping their local state consistent with the primary.

### Admin endpoints — HTTP rather than CLI

Two new routes exist on the primary router only (`build_router`; absent from `build_readonly_router`):

- `POST /admin/artists/merge` — merges two artists.
- `POST /admin/artists/alias` — registers an additional alias for an artist.

Both require `X-Admin-Token` matching the `ADMIN_TOKEN` environment variable. If `ADMIN_TOKEN` is not set the endpoints always return 403.

**Why HTTP instead of a CLI tool?**
HTTP admin endpoints allow the operator to perform corrections remotely on a deployed node without SSH access or a shared binary. A CLI tool would require either deploying an extra binary or running a local database tool against the live SQLite file — both are more error-prone than a simple `curl` call.

**Why not expose admin routes on community nodes?**
Community nodes do not ingest data. Allowing merges on a community node would diverge its state from the primary and break sync invariants. The routes are absent from `build_readonly_router` by construction, not by runtime guard.

## Consequences

- **Schema migration**: the `artist_aliases` table is created with `CREATE TABLE IF NOT EXISTS`, so it is applied automatically on next restart of any node — no migration script is needed.
- **Back-fill on first touch**: the legacy `name_lower` fallback in `resolve_artist` back-fills the alias row on any artist hit, so the alias table self-populates without a separate migration pass.
- **Alias conflicts during merge**: if both the source and target have an alias for the same string (possible if the same name was resolved independently on both), the duplicate is dropped silently. The net result is correct — the target retains the alias; only the redundant source copy is removed.
- **`ingest_transaction` updated**: the internal artist-resolve block inside `ingest_transaction` also writes the alias row on insert, keeping the alias table consistent even for the high-throughput ingest path that bypasses `resolve_artist`.
