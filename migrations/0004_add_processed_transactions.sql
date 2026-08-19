-- Every on-chain transaction credited to an intent, one row per
-- (payment_id, tx_hash). The cumulative received amount is SUM(amount_stroops)
-- so re-seeing a transaction on a later poll cycle is an idempotent no-op
-- instead of a double-credit (issue #119).
CREATE TABLE IF NOT EXISTS processed_transactions (
    payment_id TEXT NOT NULL,
    tx_hash TEXT NOT NULL,
    amount_stroops INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    PRIMARY KEY (payment_id, tx_hash)
);
