-- Normalize legacy datetime('now') timestamps to RFC 3339 with Z suffix.
-- Converts "YYYY-MM-DD HH:MM:SS" (space, no Z) to "YYYY-MM-DDTHH:MM:SSZ".
-- Idempotent; WHERE clause skips already-normalized rows.

UPDATE payments
SET created_at = replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';

UPDATE payments
SET updated_at = Replace(updated_at, ' ', 'T') || 'Z'
WHERE updated_at NOT LIKE '%T%';

UPDATE payments
SET expires_at = Replace(expires_at, ' ', 'T') || 'Z'
WHERE expires_at NOT LIKE '%T%';

UPDATE webhook_deliveries
SET created_at = Replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';

UPDATE webhook_deliveries
SET last_attempt = Replace(last_attempt, ' ', 'T') || 'Z'
WHERE last_attempt IS NOT NULL AND last_attempt NOT LIKE '%T%';

UPDATE kv_state
SET updated_at = Replace(updated_at, ' ', 'T') || 'Z'
WHERE updated_at NOT LIKE '%T%';

UPDATE merchants
SET created_at = Replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';

UPDATE api_keys
SET created_at = Replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';

UPDATE api_keys
SET last_used_at = Replace(last_used_at, ' ', 'T') || 'Z'
WHERE last_used_at IS NOT NULL AND last_used_at NOT LIKE '%T%';

UPDATE api_keys
SET revoked_at = Replace(revoked_at, ' ', 'T') || 'Z'
WHERE revoked_at IS NOT NULL AND revoked_at NOT LIKE '%T%';

UPDATE idempotency_keys
SET created_at = Replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';

UPDATE processed_transactions
SET created_at = Replace(created_at, ' ', 'T') || 'Z'
WHERE created_at NOT LIKE '%T%';
