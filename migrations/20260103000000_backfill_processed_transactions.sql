-- Backfill processed_transactions from legacy payments.tx_hash + paid_amount.
-- This preserves the received-amount ledger for in-flight intents during upgrade.
-- Idempotent via ON CONFLICT; skips rows already present.

-- Note: This migration requires custom Rust code to parse stroops from paid_amount.
-- The migration runner will execute this as raw SQL, but the actual backfill
-- logic is handled in the transition period by db::migrate() calling the 
-- backfill code. Once all deployments are upgraded, this can become a no-op.

-- For now, this is a marker migration that sqlx will track as applied.
-- The actual data migration happens in db::migrate() during the transition.
SELECT 'Backfill handled by db::migrate() during transition' AS status;
