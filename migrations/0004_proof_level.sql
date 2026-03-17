-- Migration 0004: add proof_level to proof_tokens table
-- Issue-PROOF-LEVEL — 2026-03-14
--
-- Tracks which verification phases were completed when the token was issued.
-- Current implementation only performs Phase 1 (RSS proof), so all existing
-- and new tokens default to 'rss_only'. See ADR-0018 for phase definitions.
ALTER TABLE proof_tokens ADD COLUMN proof_level TEXT NOT NULL DEFAULT 'rss_only';
