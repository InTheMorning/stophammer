-- Audit log for applied wallet merge batches so the operator UI can undo them.

CREATE TABLE IF NOT EXISTS wallet_merge_apply_batch (
    id              INTEGER PRIMARY KEY,
    source          TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    merges_applied  INTEGER NOT NULL DEFAULT 0,
    undone_at       INTEGER
) STRICT;

CREATE TABLE IF NOT EXISTS wallet_merge_apply_entry (
    id                          INTEGER PRIMARY KEY,
    batch_id                    INTEGER NOT NULL REFERENCES wallet_merge_apply_batch(id),
    seq                         INTEGER NOT NULL,
    reason                      TEXT NOT NULL,
    old_wallet_id               TEXT NOT NULL,
    new_wallet_id               TEXT NOT NULL,
    old_wallet_json             TEXT NOT NULL,
    old_endpoint_ids_json       TEXT NOT NULL,
    old_artist_links_json       TEXT NOT NULL,
    new_artist_ids_json         TEXT NOT NULL,
    moved_reviews_json          TEXT NOT NULL,
    redirect_rows_json          TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_wallet_merge_apply_entry_batch
    ON wallet_merge_apply_entry(batch_id, seq);
