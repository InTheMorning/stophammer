ALTER TABLE source_contributor_claims
    ADD COLUMN role_norm TEXT;

UPDATE source_contributor_claims
SET role_norm = lower(trim(role))
WHERE role IS NOT NULL AND trim(role) <> '';

CREATE INDEX IF NOT EXISTS idx_source_contrib_role_norm
    ON source_contributor_claims(role_norm);
