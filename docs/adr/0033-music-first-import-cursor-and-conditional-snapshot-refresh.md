# ADR 0033: Music-First Import Cursor and Conditional Snapshot Refresh

## Status
Proposed

Date: 2026-04-08

Builds on [ADR 0030: PodcastIndex Importer Durable Attempt Memory](0030-podcastindex-importer-durable-attempt-memory.md)

## Context
Phase 2 of the v4v music metadata plan is limited to importer and crawler
behavior. The immediate importer problems are mechanical:

- a fresh or stale `all_feeds` cursor still starts near `0`, which forces the
  importer to replay millions of obviously irrelevant old PodcastIndex rows
  before it reaches the music-era window that matters to current Stophammer
  operation
- `--refresh-db` always redownloads and re-extracts the PodcastIndex snapshot
  archive even when the local snapshot is already current
- the importer still carries resolver-era pause coordination even though the
  resolver runtime has been retired

ADR 0030 already established the durable `import_state.db` attempt memory and
batch cursor model. Phase 2 should tighten startup behavior without reopening
schema questions.

## Decision
The crawler importer adds a hard music-first lower bound for the `all_feeds`
scope and changes snapshot refresh to a conditional network check.

### Music-first lower bound

The `all_feeds` snapshot import scope gets a hard lower bound at PodcastIndex
row `4_630_863`.

If the stored `all_feeds` cursor is below that bound and the operator did not
pass an explicit `--cursor`, the importer:

- logs the jump
- starts from `4_630_863`
- persists the jump in `import_progress`

The persisted audit keys record:

- reason
- previous cursor
- new cursor
- timestamp

This keeps the behavior operator-visible in state instead of making the jump an
invisible startup heuristic.

The explicit `--cursor` flag still wins over the lower-bound policy. The
`wavlake_only` scope does not apply this jump.

### Conditional snapshot refresh

`--refresh-db` no longer means “blindly redownload.” It now means “check the
remote snapshot and download only if it changed.”

When a local snapshot file already exists and `--refresh-db` is set, the
importer:

- reads the local file modification time
- sends a conditional request using `If-Modified-Since`
- keeps the local snapshot untouched when the remote responds `304 Not Modified`
- downloads and extracts the archive only when the remote snapshot changed

If the local snapshot file does not exist, the importer still downloads it
unconditionally.

### Resolver-era importer coordination

The importer no longer attempts to pause or resume a resolver runtime. Import
mode behavior is now scoped to:

- snapshot state
- durable import attempt memory
- fetch / parse / ingest execution

This aligns the importer with ADR 0032's resolver retirement.

## Consequences
- Fresh `all_feeds` imports skip directly to the approved music-first window
  instead of replaying the pre-music corpus.
- Operators can audit the jump in `import_state.db`.
- `--refresh-db` becomes cheaper and more restart-friendly when the local
  snapshot is already current.
- The importer remains source-first and observability-first; it does not add
  new canonical matching behavior.
- CLI help, README text, and importer tests must be updated to match the new
  cursor and snapshot semantics.
