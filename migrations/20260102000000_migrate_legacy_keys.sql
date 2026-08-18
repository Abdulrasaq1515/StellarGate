-- Migrate pre-existing single-key merchants to the api_keys table.
-- Idempotent via ON CONFLICT; safe to run multiple times.

INSERT OR IGNORE INTO api_keys (id, merchant_id, key_hash, prefix, label, created_at)
SELECT 
    lower(hex(randomblob(16))), 
    id, 
    api_key_hash, 
    'legacy', 
    'migrated', 
    created_at
FROM merchants
WHERE api_key_hash IS NOT NULL AND api_key_hash <> '';
