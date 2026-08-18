use anyhow::Result;
use sqlx::{Pool, Row, Sqlite};

pub type Db = Pool<Sqlite>;

/// Normalize a raw SQLite timestamp to strict RFC 3339 UTC with a Z suffix.
///
/// Handles both legacy rows (`"2026-04-29 15:00:00"` / `"2026-04-29T15:00:00"`)
/// and already-correct rows (`"2026-04-29T15:00:00Z"`). Any value that doesn't
/// look like a 19-character datetime is returned unchanged so we never silently
/// corrupt unexpected data.
fn normalize_ts(raw: &str) -> String {
    let s = raw.trim();
    // Already has an explicit offset/Z — nothing to do.
    if s.ends_with('Z') || s.contains('+') {
        return s.to_string();
    }
    // Replace the space separator with T if present, then append Z.
    if s.len() == 19 {
        let with_t = s.replacen(' ', "T", 1);
        return format!("{with_t}Z");
    }
    s.to_string()
}

/// `LIKE` pattern every stored timestamp must match: strict RFC 3339 UTC with
/// a `Z` suffix and no fractional seconds, e.g. `2026-04-29T15:00:00Z`. `_`
/// matches exactly one character, so this pins the length and the position of
/// every separator without needing per-digit character classes SQLite's
/// dialect of `LIKE` cannot express.
///
/// Backing every timestamp `CHECK` constraint below (issue #314): every write
/// path already produces exactly this format via `strftime('%Y-%m-%dT%H:%M:%SZ',
/// ...)`, so this makes that a guarantee SQLite enforces rather than a
/// convention a future write path could silently break — which is exactly how
/// `expires_at` ended up compared as a lexical string against rows in the
/// legacy `"YYYY-MM-DD HH:MM:SS"` form (no `T`, no `Z`), which sorts *before*
/// every compliant timestamp and so reads as permanently expired.
///
/// Applies only to newly created tables: `CREATE TABLE IF NOT EXISTS` does not
/// retroactively add a constraint to a table that already exists, so an
/// upgrade of a running deployment does not gain this guarantee for rows
/// already on disk — the startup normalisation below is what repairs those.
const TS_PATTERN: &str = "____-__-__T__:__:__Z";

pub async fn migrate(pool: &Db) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;

    // Transition period: Handle columns added after baseline in old system.
    // These probes can be removed once all deployments have migrated.
    
    // asset_issuer column (issue #222)
    let has_asset_issuer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('payments') WHERE name = 'asset_issuer'",
    )
    .fetch_one(pool)
    .await?;
    if has_asset_issuer == 0 {
        sqlx::query("ALTER TABLE payments ADD COLUMN asset_issuer TEXT")
            .execute(pool)
            .await?;
    }

    // acknowledged_at column (issue #319)
    let has_acknowledged_at: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('webhook_deliveries') WHERE name = 'acknowledged_at'",
    )
    .fetch_one(pool)
    .await?;
    if has_acknowledged_at == 0 {
        sqlx::query("ALTER TABLE webhook_deliveries ADD COLUMN acknowledged_at TEXT")
            .execute(pool)
            .await?;
    }

    // Backfill processed_transactions for existing deployments.
    backfill_processed_transactions(pool).await?;

    Ok(())
}

/// Backfill processed_transactions from legacy payments.tx_hash + paid_amount.
/// Idempotent via ON CONFLICT; safe to run multiple times during transition.
async fn backfill_processed_transactions(pool: &Db) -> Result<()> {
    let legacy = sqlx::query(
        "SELECT id, tx_hash, paid_amount FROM payments
         WHERE tx_hash IS NOT NULL AND tx_hash <> '' AND paid_amount IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    for row in &legacy {
        let id: String = row.get("id");
        let tx_hash: String = row.get("tx_hash");
        let paid_amount: String = row.get("paid_amount");
        if let Some(stroops) = crate::money::parse_stroops(&paid_amount) {
            sqlx::query(
                "INSERT INTO processed_transactions (payment_id, tx_hash, amount_stroops)
                 VALUES (?, ?, ?)
                 ON CONFLICT(payment_id, tx_hash) DO NOTHING",
            )
            .bind(&id)
            .bind(&tx_hash)
            .bind(stroops)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

/// Fill `asset_issuer` on rows that only stored a code, using the current
/// allow-list. Duplicate codes are rejected at boot, so each code maps to at
/// most one issuer. Native assets stay NULL.
pub async fn backfill_asset_issuers(
    pool: &Db,
    accepted: &[crate::config::AcceptedAsset],
) -> Result<()> {
    for asset in accepted {
        let Some(issuer) = asset.issuer.as_deref() else {
            continue;
        };
        sqlx::query(
            "UPDATE payments
                SET asset_issuer = ?
              WHERE upper(asset) = upper(?)
                AND (asset_issuer IS NULL OR asset_issuer = '')",
        )
        .bind(issuer)
        .bind(&asset.code)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Payment {
    pub id: String,
    pub merchant_id: String,
    pub destination_address: String,
    pub memo: String,
    pub amount: String,
    pub asset: String,
    pub status: String,
    pub webhook_url: Option<String>,
    pub tx_hash: Option<String>,
    pub paid_amount: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// When this intent stops being `pending` and is swept to `expired`.
    pub expires_at: String,
    /// Issuer account for a credit asset; `None` for native XLM. Settlement
    /// matches this issuer, not any allow-list entry that shares the code
    /// (issue #222).
    pub asset_issuer: Option<String>,
}

fn row_to_payment(row: &sqlx::sqlite::SqliteRow) -> Payment {
    Payment {
        id: row.get("id"),
        merchant_id: row.get("merchant_id"),
        destination_address: row.get("destination_address"),
        memo: row.get("memo"),
        amount: row.get("amount"),
        asset: row.get("asset"),
        status: row.get("status"),
        webhook_url: row.get("webhook_url"),
        tx_hash: row.get("tx_hash"),
        paid_amount: row.get("paid_amount"),
        created_at: normalize_ts(&row.get::<String, _>("created_at")),
        updated_at: normalize_ts(&row.get::<String, _>("updated_at")),
        expires_at: normalize_ts(&row.get::<String, _>("expires_at")),
        asset_issuer: row.get("asset_issuer"),
    }
}

/// Fields needed to insert a new payment intent.
pub struct NewPayment<'a> {
    pub id: &'a str,
    pub merchant_id: &'a str,
    pub destination_address: &'a str,
    pub memo: &'a str,
    pub amount: &'a str,
    pub asset: &'a str,
    /// Issuer for `asset`; `None` for native XLM.
    pub asset_issuer: Option<&'a str>,
    pub webhook_url: Option<&'a str>,
    /// Seconds from now until the intent expires. The expiry timestamp is
    /// computed by SQLite at insert time as `now + ttl_secs`.
    pub ttl_secs: i64,
}

pub async fn create_payment(pool: &Db, new: NewPayment<'_>) -> Result<Payment> {
    /* Canonicalize the amount: parse to stroops, then convert back to the
    canonical string representation. This ensures "10.00", "10.0", and "10"
    all serialize identically, eliminating spurious string-based comparisons
    across create/get/webhook responses. */
    let stroops =
        crate::money::parse_stroops(new.amount).ok_or_else(|| anyhow::anyhow!("Invalid amount"))?;
    let canonical_amount = crate::money::stroops_to_string(stroops);

    /* Compute the expiry as `now + ttl_secs` in SQLite so it shares the exact
    clock and RFC 3339 format as created_at. */
    let ttl_modifier = format!("{:+} seconds", new.ttl_secs);
    sqlx::query(
        "INSERT INTO payments (id, merchant_id, destination_address, memo, amount, asset, asset_issuer, webhook_url, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now',?))",
    )
    .bind(new.id)
    .bind(new.merchant_id)
    .bind(new.destination_address)
    .bind(new.memo)
    .bind(&canonical_amount)
    .bind(new.asset)
    .bind(new.asset_issuer)
    .bind(new.webhook_url)
    .bind(&ttl_modifier)
    .execute(pool)
    .await?;

    get_payment(pool, new.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Payment not found after insert"))
}

/// Look up the payment id previously minted for `(merchant_id, key)`, if any.
pub async fn find_payment_id_by_idempotency_key(
    pool: &Db,
    merchant_id: &str,
    key: &str,
) -> Result<Option<String>> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT payment_id FROM idempotency_keys WHERE merchant_id = ? AND idempotency_key = ?",
    )
    .bind(merchant_id)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

/// Record the payment id minted for `(merchant_id, key)`. If the key already
/// exists (e.g. a concurrent request won the race), the existing mapping is left
/// untouched and the winning payment id is returned; otherwise `payment_id` is
/// stored and returned.
pub async fn save_idempotency_key(
    pool: &Db,
    merchant_id: &str,
    key: &str,
    payment_id: &str,
) -> Result<String> {
    sqlx::query(
        "INSERT INTO idempotency_keys (merchant_id, idempotency_key, payment_id)
         VALUES (?, ?, ?)
         ON CONFLICT(merchant_id, idempotency_key) DO NOTHING",
    )
    .bind(merchant_id)
    .bind(key)
    .bind(payment_id)
    .execute(pool)
    .await?;

    // Re-read so a concurrent insert that won the race returns the canonical id.
    let stored = find_payment_id_by_idempotency_key(pool, merchant_id, key)
        .await?
        .unwrap_or_else(|| payment_id.to_string());
    Ok(stored)
}

pub async fn get_payment(pool: &Db, id: &str) -> Result<Option<Payment>> {
    let row = sqlx::query(
        "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
         FROM payments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(row_to_payment))
}

/// Offset variant of `list_payments_keyset`. Rows are ordered by
/// `(created_at DESC, id DESC)` — exactly the keyset ordering — so a
/// `next_cursor` minted from this page resumes in cursor mode without
/// skipping or repeating rows. `created_at` is whole-second, so ties are
/// common; leaving their order to SQLite lets offset pages repeat or skip
/// rows and would make the migration cursor diverge from the keyset scan.
/// Offset-paginated page of a merchant's payments. Does **not** compute a row
/// count — see [`count_payments`] (issue #320). SQLite has no cached row
/// count, so a `COUNT(*)` here would scan every matching row on every list
/// request (including the first page) purely to fill a `total` field most
/// callers never read; keeping it a separate, opt-in query means the default
/// list path never pays for it.
pub async fn list_payments(
    pool: &Db,
    merchant_id: &str,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Payment>> {
    let rows = if let Some(s) = status {
        sqlx::query(
            "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments WHERE merchant_id = ? AND status = ? ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(merchant_id)
        .bind(s)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments WHERE merchant_id = ? ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(merchant_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.iter().map(row_to_payment).collect())
}

/// Count a merchant's payments matching an optional status filter. Split out
/// from [`list_payments`] so the default `GET /payments` path never pays for
/// a full-table `COUNT(*)` — this only runs when a caller explicitly asks for
/// `total` via `?include_total=true` (issue #320).
pub async fn count_payments(pool: &Db, merchant_id: &str, status: Option<&str>) -> Result<i64> {
    let total = if let Some(s) = status {
        sqlx::query_scalar("SELECT COUNT(*) FROM payments WHERE merchant_id = ? AND status = ?")
            .bind(merchant_id)
            .bind(s)
            .fetch_one(pool)
            .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM payments WHERE merchant_id = ?")
            .bind(merchant_id)
            .fetch_one(pool)
            .await?
    };
    Ok(total)
}

pub async fn list_payments_keyset(
    pool: &Db,
    merchant_id: &str,
    status: Option<&str>,
    limit: i64,
    cursor: Option<(&str, &str)>,
) -> Result<Vec<Payment>> {
    let rows = match (status, cursor) {
        (None, None) => {
            sqlx::query(
                "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments WHERE merchant_id = ? ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(merchant_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }

        (None, Some((ts, cid))) => {
            sqlx::query(
                "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments
             WHERE merchant_id = ? AND (created_at < ? OR (created_at = ? AND id < ?))
             ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(merchant_id)
            .bind(ts)
            .bind(ts)
            .bind(cid)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }

        (Some(s), None) => {
            sqlx::query(
                "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments WHERE merchant_id = ? AND status = ? ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(merchant_id)
            .bind(s)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }

        (Some(s), Some((ts, cid))) => {
            sqlx::query(
                "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                    webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
             FROM payments
             WHERE merchant_id = ? AND status = ? AND (created_at < ? OR (created_at = ? AND id < ?))
             ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(merchant_id)
            .bind(s)
            .bind(ts)
            .bind(ts)
            .bind(cid)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows.iter().map(row_to_payment).collect())
}

/// All payments still awaiting confirmation or top-up, oldest first. Rows whose
/// TTL has elapsed are excluded even if the sweeper hasn't transitioned them
/// yet, so an overdue intent is never polled.
pub async fn list_pending(pool: &Db) -> Result<Vec<Payment>> {
    let rows = sqlx::query(
        "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
         FROM payments
         WHERE status IN ('pending', 'underpaid')
           AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ','now')
         ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_payment).collect())
}

/// Transition up to `batch` watchable payments whose TTL has elapsed to
/// `expired`, returning the rows that were swept so the caller can fire
/// `payment.expired` webhooks.
///
/// The whole batch is transitioned in a single `UPDATE … RETURNING` — one
/// round-trip instead of one guarded `UPDATE` per intent (issue #323). The
/// `WHERE … status IN ('pending','underpaid')` guard remains what makes a
/// concurrent settlement win the race: the subquery and update run under one
/// write lock, so a payment that settles in between is never selected here
/// (if the settlement committed first) and a payment this statement sweeps is
/// rejected by the settlement's own guard (issue #155) — never double-reported.
/// `RETURNING` yields exactly the rows this statement actually transitioned.
///
/// `batch` bounds each statement, so a large backlog drains over several
/// sweeps instead of one long write lock.
pub async fn expire_overdue(pool: &Db, batch: i64) -> Result<Vec<Payment>> {
    let rows = sqlx::query(
        "UPDATE payments
            SET status = 'expired',
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id IN (
              SELECT id FROM payments
               WHERE status IN ('pending', 'underpaid')
                 AND expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ','now')
               ORDER BY created_at ASC
               LIMIT ?
          )
          RETURNING id, merchant_id, destination_address, memo, amount, asset,
                    asset_issuer, status, webhook_url, tx_hash, paid_amount,
                    created_at, updated_at, expires_at",
    )
    .bind(batch)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_payment).collect())
}

pub async fn find_pending_by_memo(pool: &Db, memo: &str) -> Result<Option<Payment>> {
    let row = sqlx::query(
        "SELECT id, merchant_id, destination_address, memo, amount, asset, asset_issuer, status,
                webhook_url, tx_hash, paid_amount, created_at, updated_at, expires_at
         FROM payments
         WHERE memo = ?
           AND status IN ('pending', 'underpaid')
           AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ','now')",
    )
    .bind(memo)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(row_to_payment))
}

/// Transition a payment to a new status, returning `true` when the row was
/// actually updated.
///
/// The `WHERE … AND status IN ('pending', 'underpaid')` guard is the key to
/// single-settlement under concurrent reconciliation (issue #155): SQLite's
/// serialized write path ensures that only one reconciliation loop can ever
/// win the race to mark a payment settled. Every other attempt finds zero
/// matching rows.
pub async fn update_payment_status(
    pool: &Db,
    id: &str,
    new_status: &str,
    tx_hash: Option<&str>,
    paid_amount: Option<&str>,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE payments
            SET status = ?,
                tx_hash = COALESCE(?, tx_hash),
                paid_amount = COALESCE(?, paid_amount),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id = ?
            AND status IN ('pending', 'underpaid')",
    )
    .bind(new_status)
    .bind(tx_hash)
    .bind(paid_amount)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Record that we've credited this `(payment_id, tx_hash)` so re-seeing the
/// same transaction on a later poll cycle is an idempotent no-op. Returns the
/// cumulative stroops received across all transactions we've credited to this
/// payment — the SUM of `amount_stroops` for the intent's rows, including the
/// one just inserted.
pub async fn record_processed_transaction(
    pool: &Db,
    payment_id: &str,
    tx_hash: &str,
    amount_stroops: i64,
) -> Result<i64> {
    sqlx::query(
        "INSERT INTO processed_transactions (payment_id, tx_hash, amount_stroops)
         VALUES (?, ?, ?)
         ON CONFLICT(payment_id, tx_hash) DO NOTHING",
    )
    .bind(payment_id)
    .bind(tx_hash)
    .bind(amount_stroops)
    .execute(pool)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_stroops), 0) FROM processed_transactions WHERE payment_id = ?",
    )
    .bind(payment_id)
    .fetch_one(pool)
    .await?;

    Ok(total)
}

pub async fn get_kv(pool: &Db, key: &str) -> Result<Option<String>> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM kv_state WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(value)
}

pub async fn set_kv(pool: &Db, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO kv_state (key, value)
         VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Merchant {
    pub id: String,
    pub api_key_hash: String,
    pub created_at: String,
}

pub async fn create_merchant(
    pool: &Db,
    id: &str,
    api_key_hash: &str,
) -> Result<Merchant> {
    sqlx::query(
        "INSERT INTO merchants (id, api_key_hash) VALUES (?, ?)",
    )
    .bind(id)
    .bind(api_key_hash)
    .execute(pool)
    .await?;

    get_merchant(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Merchant not found after insert"))
}

pub async fn get_merchant(pool: &Db, id: &str) -> Result<Option<Merchant>> {
    let row = sqlx::query("SELECT id, api_key_hash, created_at FROM merchants WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| Merchant {
        id: r.get("id"),
        api_key_hash: r.get("api_key_hash"),
        created_at: normalize_ts(&r.get::<String, _>("created_at")),
    }))
}

pub async fn find_merchant_by_key_hash(pool: &Db, hash: &str) -> Result<Option<Merchant>> {
    let row = sqlx::query("SELECT id, api_key_hash, created_at FROM merchants WHERE api_key_hash = ?")
        .bind(hash)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| Merchant {
        id: r.get("id"),
        api_key_hash: r.get("api_key_hash"),
        created_at: normalize_ts(&r.get::<String, _>("created_at")),
    }))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiKey {
    pub id: String,
    pub merchant_id: String,
    pub key_hash: String,
    pub prefix: String,
    pub label: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

pub async fn create_api_key(
    pool: &Db,
    id: &str,
    merchant_id: &str,
    key_hash: &str,
    prefix: &str,
    label: Option<&str>,
) -> Result<ApiKey> {
    sqlx::query(
        "INSERT INTO api_keys (id, merchant_id, key_hash, prefix, label) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(merchant_id)
    .bind(key_hash)
    .bind(prefix)
    .bind(label)
    .execute(pool)
    .await?;

    get_api_key(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("API key not found after insert"))
}

pub async fn get_api_key(pool: &Db, id: &str) -> Result<Option<ApiKey>> {
    let row = sqlx::query(
        "SELECT id, merchant_id, key_hash, prefix, label, created_at, last_used_at, revoked_at
         FROM api_keys WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ApiKey {
        id: r.get("id"),
        merchant_id: r.get("merchant_id"),
        key_hash: r.get("key_hash"),
        prefix: r.get("prefix"),
        label: r.get("label"),
        created_at: normalize_ts(&r.get::<String, _>("created_at")),
        last_used_at: r.get::<Option<String>, _>("last_used_at").map(|s| normalize_ts(&s)),
        revoked_at: r.get::<Option<String>, _>("revoked_at").map(|s| normalize_ts(&s)),
    }))
}

pub async fn find_api_key_by_hash(pool: &Db, hash: &str) -> Result<Option<ApiKey>> {
    let row = sqlx::query(
        "SELECT id, merchant_id, key_hash, prefix, label, created_at, last_used_at, revoked_at
         FROM api_keys WHERE key_hash = ?",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ApiKey {
        id: r.get("id"),
        merchant_id: r.get("merchant_id"),
        key_hash: r.get("key_hash"),
        prefix: r.get("prefix"),
        label: r.get("label"),
        created_at: normalize_ts(&r.get::<String, _>("created_at")),
        last_used_at: r.get::<Option<String>, _>("last_used_at").map(|s| normalize_ts(&s)),
        revoked_at: r.get::<Option<String>, _>("revoked_at").map(|s| normalize_ts(&s)),
    }))
}

pub async fn list_api_keys_for_merchant(pool: &Db, merchant_id: &str) -> Result<Vec<ApiKey>> {
    let rows = sqlx::query(
        "SELECT id, merchant_id, key_hash, prefix, label, created_at, last_used_at, revoked_at
         FROM api_keys WHERE merchant_id = ? ORDER BY created_at DESC",
    )
    .bind(merchant_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| ApiKey {
            id: r.get("id"),
            merchant_id: r.get("merchant_id"),
            key_hash: r.get("key_hash"),
            prefix: r.get("prefix"),
            label: r.get("label"),
            created_at: normalize_ts(&r.get::<String, _>("created_at")),
            last_used_at: r.get::<Option<String>, _>("last_used_at").map(|s| normalize_ts(&s)),
            revoked_at: r.get::<Option<String>, _>("revoked_at").map(|s| normalize_ts(&s)),
        })
        .collect())
}

pub async fn touch_api_key(pool: &Db, key_hash: &str) -> Result<()> {
    sqlx::query(
        "UPDATE api_keys SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE key_hash = ?",
    )
    .bind(key_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn revoke_api_key(pool: &Db, id: &str, merchant_id: &str) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE api_keys
            SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id = ? AND merchant_id = ? AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(merchant_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn count_active_keys_for_merchant(pool: &Db, merchant_id: &str) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_keys WHERE merchant_id = ? AND revoked_at IS NULL",
    )
    .bind(merchant_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookDelivery {
    pub id: String,
    pub payment_id: String,
    pub url: String,
    pub payload: String,
    pub event_type: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub last_attempt: Option<String>,
    pub acknowledged_at: Option<String>,
    pub created_at: String,
}

pub async fn create_webhook_delivery(
    pool: &Db,
    id: &str,
    payment_id: &str,
    url: &str,
    payload: &str,
    event_type: Option<&str>,
) -> Result<WebhookDelivery> {
    sqlx::query(
        "INSERT INTO webhook_deliveries (id, payment_id, url, payload, event_type) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(payment_id)
    .bind(url)
    .bind(payload)
    .bind(event_type)
    .execute(pool)
    .await?;

    get_webhook_delivery(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Webhook delivery not found after insert"))
}

pub async fn get_webhook_delivery(pool: &Db, id: &str) -> Result<Option<WebhookDelivery>> {
    let row = sqlx::query(
        "SELECT id, payment_id, url, payload, event_type, status, attempts, last_attempt, acknowledged_at, created_at
         FROM webhook_deliveries WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| WebhookDelivery {
        id: r.get("id"),
        payment_id: r.get("payment_id"),
        url: r.get("url"),
        payload: r.get("payload"),
        event_type: r.get("event_type"),
        status: r.get("status"),
        attempts: r.get("attempts"),
        last_attempt: r.get::<Option<String>, _>("last_attempt").map(|s| normalize_ts(&s)),
        acknowledged_at: r.get::<Option<String>, _>("acknowledged_at").map(|s| normalize_ts(&s)),
        created_at: normalize_ts(&r.get::<String, _>("created_at")),
    }))
}

pub async fn list_webhook_deliveries_for_payment(
    pool: &Db,
    payment_id: &str,
) -> Result<Vec<WebhookDelivery>> {
    let rows = sqlx::query(
        "SELECT id, payment_id, url, payload, event_type, status, attempts, last_attempt, acknowledged_at, created_at
         FROM webhook_deliveries
         WHERE payment_id = ?
         ORDER BY created_at DESC",
    )
    .bind(payment_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| WebhookDelivery {
            id: r.get("id"),
            payment_id: r.get("payment_id"),
            url: r.get("url"),
            payload: r.get("payload"),
            event_type: r.get("event_type"),
            status: r.get("status"),
            attempts: r.get("attempts"),
            last_attempt: r.get::<Option<String>, _>("last_attempt").map(|s| normalize_ts(&s)),
            acknowledged_at: r.get::<Option<String>, _>("acknowledged_at").map(|s| normalize_ts(&s)),
            created_at: normalize_ts(&r.get::<String, _>("created_at")),
        })
        .collect())
}

pub async fn update_webhook_delivery_status(
    pool: &Db,
    id: &str,
    status: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE webhook_deliveries
            SET status = ?,
                attempts = attempts + 1,
                last_attempt = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id = ?",
    )
    .bind(status)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Find webhook deliveries eligible for redrive: `pending` or `failed`, not
/// attempted recently, and below the max attempt limit. `grace_secs` ensures
/// a row never races an in-flight inline dispatch; `backoff_initial` and
/// `backoff_max` spread retries; `jitter` decorrelates co-failing batches.
pub async fn find_eligible_for_redrive(
    pool: &Db,
    max_attempts: i64,
    grace_secs: i64,
    backoff_initial: i64,
    backoff_max: i64,
    jitter_secs: i64,
    limit: i64,
) -> Result<Vec<WebhookDelivery>> {
    let rows = sqlx::query(&format!(
        "SELECT id, payment_id, url, payload, event_type, status, attempts, last_attempt, acknowledged_at, created_at
         FROM webhook_deliveries
         WHERE status IN ('pending', 'failed')
           AND attempts < ?
           AND (last_attempt IS NULL OR
                unixepoch('now') - unixepoch(last_attempt) >=
                  MAX(?, MIN(? * (1 << (attempts - 1)), ?)) + (ABS(RANDOM()) % (? + 1)))
         ORDER BY last_attempt ASC NULLS FIRST
         LIMIT ?",
    ))
    .bind(max_attempts)
    .bind(grace_secs)
    .bind(backoff_initial)
    .bind(backoff_max)
    .bind(jitter_secs)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| WebhookDelivery {
            id: r.get("id"),
            payment_id: r.get("payment_id"),
            url: r.get("url"),
            payload: r.get("payload"),
            event_type: r.get("event_type"),
            status: r.get("status"),
            attempts: r.get("attempts"),
            last_attempt: r.get::<Option<String>, _>("last_attempt").map(|s| normalize_ts(&s)),
            acknowledged_at: r.get::<Option<String>, _>("acknowledged_at").map(|s| normalize_ts(&s)),
            created_at: normalize_ts(&r.get::<String, _>("created_at")),
        })
        .collect())
}

/// Prune terminal (`delivered`/`failed`) delivery rows older than `retention_days`.
/// Returns the number of rows deleted. `pending` rows are exempt regardless of
/// age — the redrive worker still owns them. Rows are deleted in batches of
/// `batch_size` with a per-call cap so a large backlog drains over several cycles
/// instead of one long write lock. Does NOT prune `failed` rows that have been
/// `acknowledged_at` — they've been explicitly kept for audit.
pub async fn prune_webhook_deliveries(
    pool: &Db,
    retention_days: i64,
    batch_size: i64,
) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM webhook_deliveries
          WHERE id IN (
              SELECT id FROM webhook_deliveries
               WHERE status IN ('delivered', 'failed')
                 AND unixepoch('now') - unixepoch(created_at) > ? * 86400
                 AND (status = 'delivered' OR acknowledged_at IS NOT NULL)
               LIMIT ?
          )",
    )
    .bind(retention_days)
    .bind(batch_size)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Prune idempotency keys older than `retention_days`. Returns the number of
/// rows deleted. Keys are pruned in batches of `batch_size` with a per-call cap.
pub async fn prune_idempotency_keys(
    pool: &Db,
    retention_days: i64,
    batch_size: i64,
) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM idempotency_keys
          WHERE merchant_id || idempotency_key IN (
              SELECT merchant_id || idempotency_key FROM idempotency_keys
               WHERE unixepoch('now') - unixepoch(created_at) > ? * 86400
               LIMIT ?
          )",
    )
    .bind(retention_days)
    .bind(batch_size)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Mark deliveries as acknowledged so retention won't delete them (issue #319).
pub async fn acknowledge_deliveries(pool: &Db, ids: &[String]) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE webhook_deliveries
            SET acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id IN ({}) AND acknowledged_at IS NULL",
        placeholders
    );
    let mut query = sqlx::query(&sql);
    for id in ids {
        query = query.bind(id);
    }
    let result = query.execute(pool).await?;
    Ok(result.rows_affected())
}
