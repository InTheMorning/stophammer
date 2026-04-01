ALTER TABLE wallet_identity_review RENAME TO wallet_identity_review_legacy_0024;

CREATE TABLE wallet_identity_review (
    id                    INTEGER PRIMARY KEY,
    wallet_id             TEXT NOT NULL REFERENCES wallets(wallet_id),
    source                TEXT NOT NULL,
    evidence_key          TEXT NOT NULL,
    wallet_ids_json       TEXT NOT NULL DEFAULT '[]',
    endpoint_summary_json TEXT NOT NULL DEFAULT '[]',
    status                TEXT NOT NULL DEFAULT 'pending'
                          CHECK(status IN ('pending','merged','blocked','resolved')),
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL
) STRICT;

INSERT INTO wallet_identity_review (
    id,
    wallet_id,
    source,
    evidence_key,
    wallet_ids_json,
    endpoint_summary_json,
    status,
    created_at,
    updated_at
)
SELECT
    id,
    wallet_id,
    source,
    evidence_key,
    wallet_ids_json,
    endpoint_summary_json,
    status,
    created_at,
    updated_at
FROM wallet_identity_review_legacy_0024;

CREATE INDEX IF NOT EXISTS idx_wallet_identity_review_wallet_id
    ON wallet_identity_review(wallet_id);
CREATE INDEX IF NOT EXISTS idx_wallet_identity_review_status_created_at
    ON wallet_identity_review(status, created_at DESC);
