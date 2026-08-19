-- API keys, one row per credential rather than one per merchant, enabling
-- key rotation and instant revocation without touching the merchant record.
-- Only the SHA-256 hex digest is stored; the raw key is never persisted.
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    merchant_id TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    prefix TEXT NOT NULL,
    label TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    last_used_at TEXT,
    revoked_at TEXT
);

-- Authentication looks a key up by hash on every request — load-bearing index.
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_merchant ON api_keys(merchant_id);

-- Carry pre-existing single-key merchants across: their raw key is not
-- recoverable, but the hash is all authentication needs, so keys issued
-- before this table existed keep working. Prefix is unknown — mark as legacy.
INSERT OR IGNORE INTO api_keys (id, merchant_id, key_hash, prefix, label, created_at)
SELECT lower(hex(randomblob(16))), id, api_key_hash, 'legacy', 'migrated', created_at
FROM merchants
WHERE api_key_hash IS NOT NULL AND api_key_hash <> '';
