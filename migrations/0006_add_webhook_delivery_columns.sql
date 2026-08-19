-- event_type: records which event the payload represents so a redelivery can
-- reproduce the original X-StellarGate-Event header (issue #160). Rows written
-- before this column existed stay NULL; readers fall back to the `event` field
-- inside the stored payload.
ALTER TABLE webhook_deliveries ADD COLUMN event_type TEXT;

-- acknowledged_at: records that an operator has seen and acted on a terminal
-- failure, so retention can distinguish "dealt with" from "nobody looked at
-- this yet" and refuse to delete the latter (issue #319).
ALTER TABLE webhook_deliveries ADD COLUMN acknowledged_at TEXT;

-- delivery lookup index — queried on every listing and by the redrive worker;
-- without this it is a full table scan (issue #112).
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_payment
    ON webhook_deliveries(payment_id);
