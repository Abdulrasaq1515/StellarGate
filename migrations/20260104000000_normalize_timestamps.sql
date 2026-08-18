-- Normalize legacy datetime('now') timestamps to RFC 3339 with Z suffix.
-- Converts "YYYY-MM-DD HH:MM:SS" (space, no Z) to "YYYY-MM-DDTHH:MM:SSZ".
-- Idempotent; WHERE clause skips already-normalized rows.

UPDATE payments 
SET created_at = replace(created_at, ' ', 'T') || 'Z' 
WHERE created_at NOT LIKE '%T%';

UPDATE payments 
SET updated_at = replace(updated_at, ' ', 'T') || 'Z' 
WHERE updated_at NOT LIKE '%T%';

UPDATE webhook_deliveries 
SET created_at = replace(created_at, ' ', 'T') || 'Z' 
WHERE created_at NOT LIKE '%T%';
