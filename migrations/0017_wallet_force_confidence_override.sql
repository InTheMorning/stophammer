-- Allow operator confidence overrides in wallet_identity_override.

CREATE TABLE wallet_identity_override_new (
    id              INTEGER PRIMARY KEY,
    override_type   TEXT NOT NULL
                    CHECK(override_type IN ('merge','do_not_merge','force_class',
                                            'force_confidence',
                                            'force_artist_link','block_artist_link')),
    wallet_id       TEXT NOT NULL,
    target_id       TEXT,
    value           TEXT,
    created_at      INTEGER NOT NULL
) STRICT;

INSERT INTO wallet_identity_override_new (id, override_type, wallet_id, target_id, value, created_at)
SELECT id, override_type, wallet_id, target_id, value, created_at
FROM wallet_identity_override;

DROP TABLE wallet_identity_override;
ALTER TABLE wallet_identity_override_new RENAME TO wallet_identity_override;

CREATE INDEX IF NOT EXISTS idx_wallet_override_wallet
    ON wallet_identity_override(wallet_id);
