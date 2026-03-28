-- Issue-ROUTE-CUSTOM-NORMALIZATION — 2026-03-28
-- Normalize legacy route rows so custom_key/custom_value use the same empty-
-- string convention as wallet_endpoints. Public reads continue to expose
-- empty strings as NULL/None via NULLIF() in query code.

UPDATE payment_routes
SET custom_key = ''
WHERE custom_key IS NULL;

UPDATE payment_routes
SET custom_value = ''
WHERE custom_value IS NULL;

UPDATE feed_payment_routes
SET custom_key = ''
WHERE custom_key IS NULL;

UPDATE feed_payment_routes
SET custom_value = ''
WHERE custom_value IS NULL;
