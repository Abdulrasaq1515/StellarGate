# StellarGate

[![CI](https://github.com/StellarGateLabs/StellarGate/actions/workflows/ci.yml/badge.svg)](https://github.com/StellarGateLabs/StellarGate/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)

A payment gateway API built on [Stellar](https://stellar.org) for accepting, verifying, and settling payments in XLM, USDC, and any other Stellar asset you configure.

> Think Stripe — but settlement happens on the Stellar network instead of through banks.

---

## Table of Contents

- [Overview](#overview)
- [How It Works](#how-it-works)
- [Features](#features)
- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Payment Resolution Policy](#payment-resolution-policy)
- [Webhooks](#webhooks)
- [Security Model](#security-model)
- [Observability](#observability)
- [Database Migrations](#database-migrations)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

StellarGate turns Stellar payments into a conventional REST API. A merchant creates a **payment intent** and receives a destination address plus a unique memo. The payer sends funds from any Stellar wallet. StellarGate watches the chain, matches the incoming transaction to the intent, settles it, and delivers a signed webhook to the merchant's application.

The gateway is **non-custodial in the strictest sense**: it never holds a secret key, never signs, and never submits a Stellar transaction. It only *observes* the configured gateway account for incoming payments. Refunds and payouts remain the merchant's responsibility.

## How It Works

```
┌─────────────┐   1. POST /payments      ┌──────────────┐
│ Merchant    │ ───────────────────────► │ StellarGate  │
│ Application │ ◄─────────────────────── │              │
└─────────────┘   address + memo + id    └──────┬───────┘
       │                                        │
       │ 2. show payment details                │ 3. watch Horizon
       ▼                                        │    (SSE stream + poller)
┌─────────────┐   pays address w/ memo   ┌──────▼───────┐
│   Payer's   │ ───────────────────────► │ Stellar      │
│   Wallet    │                          │ Network      │
└─────────────┘                          └──────┬───────┘
                                                │
┌─────────────┐   5. signed webhook      ┌──────▼───────┐
│ Merchant    │ ◄─────────────────────── │ 4. verify    │
│ Application │    payment.completed     │    & settle  │
└─────────────┘                          └──────────────┘
```

A payment is matched on three independent attributes — **memo**, **destination**, and **asset** — and only then is the amount compared. Transactions that fail on-chain are ignored.

## Features

| Capability | Status | Notes |
|---|---|---|
| Payment intents | ✅ | Create, fetch, list with filtering |
| Multi-merchant | ✅ | API-key auth; every payment scoped to a `merchant_id` |
| Real-time settlement | ✅ | Horizon SSE stream, with an interval poller as reconciler |
| Payment verification | ✅ | Memo + destination + asset + exact stroop amount |
| Over/underpayment handling | ✅ | Distinct statuses and events; underpaid intents accept a top-up |
| Intent expiry | ✅ | Configurable TTL with a `payment.expired` event |
| Signed webhooks | ✅ | Timestamped HMAC-SHA256, replay-resistant |
| Webhook redrive | ✅ | Background worker recovers deliveries lost to a crash |
| Delivery inspection | ✅ | List attempts and manually redeliver |
| Idempotent creates | ✅ | Via the `Idempotency-Key` header |
| Cursor pagination | ✅ | Keyset pagination, stable at any depth |
| SSRF protection | ✅ | Webhook targets resolved and filtered, re-checked on every send |
| Rate limiting | ✅ | Per-IP, per-route-bucket |
| Prometheus metrics | ✅ | `GET /metrics` |
| Dashboard UI | ⬜ | Not started |

## Architecture

```
src/
├── main.rs        Entry point: boot, background task spawn, graceful shutdown
├── lib.rs         Shared AppState and task-health tracking
├── config.rs      Environment parsing and validation (fails fast on bad input)
├── db.rs          SQLite persistence via sqlx
├── money.rs       Stroop-exact amount parsing and canonical serialization
├── strkey.rs      Stellar address (strkey) validation
├── ssrf.rs        Webhook target resolution and private-range filtering
├── horizon.rs     Horizon SSE listener, interval poller, payment verification
├── expiry.rs      Background sweeper for overdue pending intents
├── metrics.rs     Prometheus counters and histograms
├── webhook.rs     Signed dispatch and the background redrive worker
└── api/
    ├── mod.rs     Router, auth, rate limiting, CORS, timeouts, 404 fallback
    └── payments.rs  Payment and webhook-delivery handlers

migrations/        Versioned SQL, applied automatically on startup
tests/             Integration tests (API, concurrency, rate limits, webhooks, trustlines)
```

**Amounts are handled in stroops** (1 XLM = 10,000,000 stroops) as integers throughout. Floating-point arithmetic is never used for money. Values are canonicalized on write and on serialization, so `"10.00"`, `"10.0"`, and `"10"` are stored and returned identically.

**Two independent listeners** run concurrently. The SSE stream gives near-real-time settlement; the interval poller re-scans from a persisted cursor and acts as a reconciler for anything missed during a reconnect. Both converge on the same idempotent settlement path, so a payment observed twice settles once.

### Tech Stack

| Layer | Choice |
|---|---|
| Language | Rust (2021 edition, 1.88+) |
| HTTP | [axum](https://github.com/tokio-rs/axum) + [tower-http](https://github.com/tower-rs/tower-http) |
| Database | SQLite via [sqlx](https://github.com/launchbadge/sqlx) (WAL mode) |
| Async runtime | [tokio](https://tokio.rs) |
| TLS | rustls (no OpenSSL dependency) |
| Chain access | [Stellar Horizon API](https://developers.stellar.org/api) |

---

## Getting Started

### Prerequisites

- **Rust 1.88 or newer** — [install via rustup](https://rustup.rs)
- A Stellar account public key to receive payments (testnet keys: [Stellar Laboratory](https://laboratory.stellar.org/#account-creator))

### Install and Run

```bash
git clone https://github.com/StellarGateLabs/StellarGate.git
cd StellarGate

cp .env.example .env
# Edit .env — at minimum set STELLAR_GATEWAY_PUBLIC, WEBHOOK_SECRET,
# and ADMIN_PROVISIONING_SECRET

cargo run
```

The API listens on `http://localhost:3000` by default.

### Docker

The fastest path if you'd rather not install Rust:

```bash
cp .env.example .env   # edit as above
docker compose up --build
```

The SQLite database lives in a named volume (`stellargate_data`) and survives container restarts. `docker compose down` stops the stack while preserving that volume.

### Verify the Installation

```bash
# 1. Liveness
curl http://localhost:3000/health
# {"status":"ok"}

# 2. Provision a merchant (requires ADMIN_PROVISIONING_SECRET)
curl -X POST http://localhost:3000/merchants \
  -H "X-Admin-Secret: $ADMIN_PROVISIONING_SECRET"
# {"merchant_id":"...","api_key":"..."}

# 3. Create a payment intent
curl -X POST http://localhost:3000/payments \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"amount":"10","asset":"XLM"}'
```

---

## Configuration

All configuration is via environment variables, read once at startup. **Invalid values abort boot** rather than silently falling back to a default — a typo in an asset issuer or listener mode is a startup failure, not a runtime surprise.

### Core

| Variable | Description | Default |
|---|---|---|
| `PORT` | HTTP listen port | `3000` |
| `DATABASE_URL` | sqlx connection string (not a file path) | `sqlite:stellargate.db` |
| `STELLAR_NETWORK` | `testnet` or `public` | `testnet` |
| `STELLAR_HORIZON_URL` | Horizon endpoint | testnet Horizon |
| `STELLAR_GATEWAY_PUBLIC` | Gateway wallet public key (`G…`), validated as a strkey at startup. The listener stays idle until this is set. | — |
| `ACCEPTED_ASSETS` | Comma-separated. `CODE` for native (`XLM`) or `CODE:ISSUER` (`USDC:GA…`). Adding an asset is config-only. Each issuer is strkey-validated at boot. | `XLM,USDC:<testnet issuer>` |
| `REQUEST_TIMEOUT_SECS` | Whole-request timeout; exceeding it returns `408` | `30` |

### Settlement

| Variable | Description | Default |
|---|---|---|
| `STELLAR_LISTENER_MODE` | `stream` (SSE + poller reconciler) or `poll` (interval only) | `stream` |
| `POLL_INTERVAL_SECS` | How often the poller reconciles | `10` |
| `PAYMENT_TTL_SECS` | How long an intent stays `pending` before expiring, from `created_at` | `3600` |

### Webhooks

| Variable | Description | Default |
|---|---|---|
| `WEBHOOK_SECRET` | HMAC-SHA256 signing secret. Must be **≥ 32 characters**; known placeholder values are rejected at boot. | — |
| `ALLOWED_WEBHOOK_SCHEMES` | Comma-separated URL schemes accepted for `webhook_url`. HTTPS is enforced on `public` regardless of this value. | `https` |
| `WEBHOOK_RETRY_ATTEMPTS` | Inline delivery attempts | `3` |
| `WEBHOOK_RETRY_DELAY_MS` | Delay between inline retries | `5000` |
| `WEBHOOK_TIMEOUT_SECS` | Per-attempt timeout; each retry is bounded independently | `10` |
| `WEBHOOK_ALLOW_PRIVATE_TARGETS` | Bypasses the SSRF private-range check. **Development and tests only.** | `false` |

### Webhook Redrive Worker

Recovers deliveries left `pending`/`failed` by a process that exited mid-send or a receiver that was down when inline retries ran out. Its first pass runs immediately at startup, so a restart redrives without waiting a full interval.

| Variable | Description | Default |
|---|---|---|
| `WEBHOOK_REDRIVE_INTERVAL_SECS` | Scan frequency | `30` |
| `WEBHOOK_REDRIVE_CONCURRENCY` | Max redrive requests in flight | `4` |
| `WEBHOOK_REDRIVE_MAX_ATTEMPTS` | Total attempts (inline + redrive) before a delivery is left permanently `failed` | `8` |
| `WEBHOOK_REDRIVE_GRACE_SECS` | Idle time required before the worker touches a row, so it never races an in-flight inline delivery. Also the floor under the backoff. | `60` |
| `WEBHOOK_REDRIVE_BACKOFF_INITIAL_SECS` | Exponential backoff base: `initial × 2^(attempts−1)`. A row never attempted is exempt and gated only by the grace window. `0` disables growth. | `30` |
| `WEBHOOK_REDRIVE_BACKOFF_MAX_SECS` | Backoff ceiling. Must be `≥` the initial value. | `900` |

### Security and Limits

| Variable | Description | Default |
|---|---|---|
| `ADMIN_PROVISIONING_SECRET` | Required via `X-Admin-Secret` to call `POST /merchants`. Unset disables provisioning entirely (always `401`). | _(unset — disabled)_ |
| `CORS_ALLOWED_ORIGINS` | Comma-separated origins. **Required** on `public`; omitting on testnet falls back to permissive with a warning. | _(unset)_ |
| `RATE_LIMIT_REQUESTS_PER_SEC` | Base per-IP limit. Write routes get this rate; read-only routes get 5×. | `10` |
| `DB_POOL_MAX_CONNECTIONS` | SQLite pool size. WAL allows one writer plus many readers. | `10` |
| `DB_BUSY_TIMEOUT_MS` | Lock-acquisition wait before erroring. Must be `> 0` under concurrent load. | `5000` |

---

## API Reference

### Authentication

| Scheme | Header | Used by |
|---|---|---|
| Merchant API key | `Authorization: Bearer <api_key>` | `POST /payments`, `GET /payments`, webhook delivery routes |
| Admin secret | `X-Admin-Secret: <secret>` | `POST /merchants` |

`GET /payments/:id` is intentionally public — anyone holding the payment ID can poll its status, which lets a checkout page poll without embedding a merchant key.

### Error Envelope

Every error response uses the same shape:

```json
{
  "error": "A human-readable explanation",
  "code": "stable_machine_readable_code"
}
```

The `code` field is stable across releases and is what you should branch on.

| Code | HTTP | Meaning |
|---|---|---|
| `unauthorized` | `401` | Missing/invalid API key or admin secret |
| `invalid_request` | `400` | Malformed JSON or a deserialization failure |
| `unsupported_media_type` | `415` | `Content-Type` is not `application/json` |
| `unsupported_asset` | `400` | Asset is not in `ACCEPTED_ASSETS` |
| `invalid_amount` | `400` | Not a positive decimal with ≤ 7 decimal places |
| `invalid_webhook_url` | `400` | Malformed, disallowed scheme, over 2048 chars, or SSRF-rejected |
| `invalid_status` | `400` | `status` filter is not a recognized value |
| `invalid_cursor` | `400` | `cursor` could not be decoded |
| `payment_not_found` | `404` | No such payment, or it belongs to another merchant |
| `delivery_not_found` | `404` | No such delivery for that payment |
| `webhook_target_blocked` | `400` | Redelivery target rejected by the SSRF guard |
| `webhook_delivery_failed` | `502` | Receiver returned a non-success response |
| `rate_limit_exceeded` | `429` | Per-IP bucket limit exceeded |
| `idempotency_conflict` | `500` | Concurrent creates raced on one idempotency key |
| `not_found` | `404` | No matching route |
| `internal_error` | `500` | Unexpected server-side failure |

---

### `POST /merchants`

Provision a merchant and return its API key. **Admin only** — requires `X-Admin-Secret`. There is no self-service signup; this is meant to be run by whoever operates the gateway.

```bash
curl -X POST http://localhost:3000/merchants \
  -H "X-Admin-Secret: $ADMIN_PROVISIONING_SECRET"
```

**`201 Created`**

```json
{
  "merchant_id": "a1b2c3d4-...",
  "api_key": "e5f6..."
}
```

> ⚠️ `api_key` is returned **once**, in plaintext, and is never recoverable. Only a hash is stored. Save it immediately.

---

### `POST /payments`

Create a payment intent. Requires a merchant API key; the merchant is taken from the key, not the request body.

**Request**

```json
{
  "amount": "10.00",
  "asset": "XLM",
  "webhook_url": "https://yourapp.com/webhooks/stellar"
}
```

| Field | Type | Required | Constraints |
|---|---|---|---|
| `amount` | string | ✅ | Positive decimal, ≤ 7 decimal places |
| `asset` | string | ❌ | Must be in `ACCEPTED_ASSETS`. Defaults to `XLM`. |
| `webhook_url` | string | ❌ | ≤ 2048 chars; scheme must be allowed; HTTPS required on `public`; SSRF-checked |

| Header | Required | Description |
|---|---|---|
| `Content-Type: application/json` | ✅ | Anything else returns `415` |
| `Authorization: Bearer <key>` | ✅ | Merchant API key |
| `Idempotency-Key` | ❌ | Client-chosen key, scoped per merchant. Reuse returns the original intent with `200 OK` instead of creating a duplicate. |

**`201 Created`** (or **`200 OK`** on an idempotency-key hit)

```json
{
  "id": "a1b2c3d4-...",
  "destination_address": "GBBD47IF6LWK7P7...",
  "memo": "A1B2C3D4",
  "amount": "10",
  "asset": "XLM",
  "status": "pending",
  "created_at": "2026-04-29T15:00:00Z",
  "expires_at": "2026-04-29T16:00:00Z"
}
```

> The payer must send **exactly** `amount` of `asset` to `destination_address` with `memo` set as a **text memo**. The intent expires at `expires_at` if unpaid.

---

### `GET /payments/:id`

Fetch a payment's current state. Public — no authentication required.

**`200 OK`**

```json
{
  "id": "a1b2c3d4-...",
  "merchant_id": "your-merchant-id",
  "destination_address": "GBBD47IF6LWK7P7...",
  "memo": "A1B2C3D4",
  "amount": "10",
  "asset": "XLM",
  "status": "pending",
  "tx_hash": null,
  "paid_amount": null,
  "created_at": "2026-04-29T15:00:00Z",
  "updated_at": "2026-04-29T15:00:00Z",
  "expires_at": "2026-04-29T16:00:00Z"
}
```

**Status values**

| Status | Meaning |
|---|---|
| `pending` | Awaiting payment |
| `completed` | Fully paid (includes overpayment) |
| `underpaid` | Partially paid; still watched for a top-up |
| `expired` | TTL elapsed before payment arrived; no longer watched |

---

### `GET /payments`

List the authenticated merchant's payments, newest first. Supports **cursor** (recommended) and **offset** (legacy) pagination.

| Param | Description | Default |
|---|---|---|
| `status` | Filter by `pending`, `completed`, `underpaid`, or `expired` | all |
| `limit` | Page size, 1–100 | `20` |
| `cursor` | Keyset cursor from a previous `next_cursor` | — |
| `offset` | Rows to skip (legacy; prefer `cursor`) | `0` |

**`200 OK`** — cursor mode

```json
{
  "payments": [ { "id": "...", "status": "pending" } ],
  "limit": 20,
  "next_cursor": "3230..."
}
```

`next_cursor` is `null` on the final page. Offset mode additionally returns `total` and `offset`.

> Cursor pagination is keyset-based and stays stable regardless of page depth or concurrent inserts. Offset mode is retained for backward compatibility and can skip or repeat rows if data changes mid-scan.

---

### `GET /payments/:id/webhooks`

List every delivery attempt for a payment. Requires the owning merchant's API key.

**`200 OK`**

```json
{
  "payment_id": "a1b2c3d4-...",
  "deliveries": [
    {
      "id": "d1e2f3...",
      "url": "https://yourapp.com/webhooks/stellar",
      "event": "payment.completed",
      "status": "delivered",
      "attempts": 1,
      "last_attempt": "2026-04-29T15:04:00Z",
      "created_at": "2026-04-29T15:03:59Z"
    }
  ]
}
```

### `POST /payments/:id/webhooks/:delivery_id/redeliver`

Manually re-send a delivery. The stored payload and event type are replayed verbatim with a **fresh** timestamp and signature. The SSRF guard re-runs against the target.

---

### `GET /health`

Liveness probe. Returns `200 OK` while the process is running.

```json
{ "status": "ok" }
```

### `GET /ready`

Readiness probe. Runs `SELECT 1` against the database.

```
200 OK          — { "status": "ok" }
503 Unavailable — { "status": "unavailable" }
```

### `GET /metrics`

Prometheus exposition format. See [Observability](#observability).

---

## Payment Resolution Policy

Every on-chain payment matched by memo, destination, and asset resolves as follows:

| Scenario | `status` | Event | `delta` |
|---|---|---|---|
| Paid exactly | `completed` | `payment.completed` | — |
| Paid **more** than requested | `completed` | `payment.overpaid` | excess to refund |
| Paid **less** than requested | `underpaid` | `payment.underpaid` | shortfall owed |
| Top-up reaching exactly the total | `completed` | `payment.completed` | — |
| Top-up exceeding the total | `completed` | `payment.overpaid` | cumulative excess |
| TTL elapsed, unpaid | `expired` | `payment.expired` | — |

**Overpayment** fulfils the intent. The `delta` field carries the excess; refunding it is the merchant's responsibility — the gateway cannot send funds.

**Underpayment** leaves the intent open and watched. When a follow-up payment to the same memo brings the cumulative total to or above the requested amount, the intent completes.

**Limitations to be aware of:**

- Only a **single** top-up is tracked per underpaid intent. If more is needed, the payer should send the full remaining `delta` in one transaction.
- Once an intent is `completed`, further payments to the same address and memo are **not** tracked and fire no webhooks.
- Failed on-chain transactions are ignored entirely.

---

## Webhooks

When a payment reaches a terminal state, StellarGate POSTs a signed JSON event to the intent's `webhook_url`.

### Events

| Event | Fired when |
|---|---|
| `payment.completed` | Cumulative received equals the requested amount |
| `payment.overpaid` | Cumulative received exceeds it (`delta` = excess) |
| `payment.underpaid` | Payment received but short (`delta` = shortfall) |
| `payment.expired` | TTL elapsed with no payment |

```json
{
  "event": "payment.overpaid",
  "payment_id": "a1b2c3d4-...",
  "merchant_id": "your-merchant-id",
  "tx_hash": "abc123...",
  "amount": "10",
  "paid_amount": "12.5",
  "asset": "XLM",
  "status": "completed",
  "delta": "2.5"
}
```

`delta` is present only on `payment.overpaid` and `payment.underpaid`.

### Verifying Signatures

| Header | Value |
|---|---|
| `X-StellarGate-Timestamp` | Unix seconds at signing time |
| `X-StellarGate-Signature` | Hex HMAC-SHA256 of `"{timestamp}.{raw_body}"`, keyed with `WEBHOOK_SECRET` |
| `X-StellarGate-Event` | Convenience copy of the event type — **not signed** |

Binding the signature to the timestamp (Stripe-style) is what prevents indefinite replay of a captured request.

1. Read the timestamp (`t`) and signature (`sig`).
2. Reject if `abs(now − t) > tolerance`. **5 minutes** is recommended.
3. Concatenate `"{t}.{raw_body}"` using the **exact received bytes** — verify before any JSON re-encoding, which would change them.
4. Compute `HMAC_SHA256(WEBHOOK_SECRET, "{t}.{raw_body}")`, hex-encoded.
5. Compare against `sig` in **constant time**.
6. Only after the signature passes, read `event` from the **body**.

> ⚠️ Never route security-sensitive logic on `X-StellarGate-Event`. It is outside the signed material and can be altered in transit without invalidating the signature. The `event` field inside the verified body is authoritative.

**Node.js**

```js
const crypto = require("crypto");

function verify(rawBody, headers, secret, toleranceSec = 300) {
  const t = Number(headers["x-stellargate-timestamp"]);
  const sig = headers["x-stellargate-signature"];
  if (!Number.isFinite(t) || Math.abs(Date.now() / 1000 - t) > toleranceSec) {
    return false; // stale or missing timestamp
  }
  const expected = crypto
    .createHmac("sha256", secret)
    .update(`${t}.${rawBody}`)
    .digest("hex");
  const a = Buffer.from(sig, "hex");
  const b = Buffer.from(expected, "hex");
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

function handleWebhook(rawBody, headers, secret) {
  if (!verify(rawBody, headers, secret)) throw new Error("invalid signature");
  const { event } = JSON.parse(rawBody); // ← authenticated; safe to route on
  switch (event) {
    case "payment.completed": /* fulfil the order */ break;
    case "payment.overpaid":  /* fulfil, then refund `delta` */ break;
    case "payment.underpaid": /* await top-up of `delta` */ break;
    case "payment.expired":   /* release the cart */ break;
  }
}
```

**Python**

```python
import hmac, hashlib, time

def verify(raw_body: bytes, headers, secret: str, tolerance: int = 300) -> bool:
    try:
        t = int(headers["X-StellarGate-Timestamp"])
    except (KeyError, ValueError):
        return False
    if abs(time.time() - t) > tolerance:
        return False
    expected = hmac.new(
        secret.encode(), f"{t}.".encode() + raw_body, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, headers.get("X-StellarGate-Signature", ""))
```

### Delivery Guarantees

Delivery is **at-least-once**. A receiver may see the same event more than once — after an inline retry, a redrive, or a manual redelivery — so handlers must be idempotent. Key on `payment_id` plus `event`.

Every attempt is recorded in `webhook_deliveries` and inspectable via `GET /payments/:id/webhooks`. A delivery that exhausts `WEBHOOK_REDRIVE_MAX_ATTEMPTS` is left `failed` and can still be redelivered manually.

For the full canonical reference, see **[WEBHOOK_REFERENCE.md](WEBHOOK_REFERENCE.md)**.

---

## Security Model

**No custody.** The gateway never holds a secret key, never signs, and never submits a transaction. Compromising it does not move funds — it only watches an address.

**API keys are hashed at rest.** The plaintext key is shown once at provisioning and never stored.

**SSRF protection on webhook targets.** A `webhook_url` has its host resolved and rejected if it lands on loopback, link-local (including the cloud metadata address `169.254.169.254`), private, or otherwise reserved ranges. The check runs again on every dispatch and redelivery **against the exact resolved address** rather than a fresh DNS lookup, closing the DNS-rebinding window.

**HTTPS enforced on mainnet.** On `STELLAR_NETWORK=public`, a `webhook_url` must be HTTPS regardless of `ALLOWED_WEBHOOK_SCHEMES` — a permissive scheme list cannot downgrade mainnet delivery to plaintext.

**Rate limiting.** Every route falls into a per-IP bucket. Write and sensitive routes get the base quota; read-only routes get 5×. The limiter cache is capacity-bounded with idle eviction, so key cardinality cannot exhaust memory.

**Bounded requests.** Bodies are capped at 256 KiB and every request is subject to `REQUEST_TIMEOUT_SECS`.

**Fail-fast configuration.** Invalid strkeys, unknown listener modes, and short webhook secrets abort startup instead of degrading silently.

To report a vulnerability, see [SECURITY.md](SECURITY.md).

---

## Observability

`GET /metrics` exposes Prometheus metrics:

| Metric | Type | Description |
|---|---|---|
| `stellargate_auth_attempts_total` | counter | Labelled by `outcome`, and `reason` on failure (`missing_key`, `invalid_key`) |
| `stellargate_webhook_deliveries_total` | counter | Delivery outcomes |
| `stellargate_webhook_retries_total` | counter | Retry attempts |
| `stellargate_webhook_delivery_latency_ms` | histogram | End-to-end delivery latency |

Structured logs (via `tracing`) carry an `x-request-id` on every request, propagated to responses. Settlement logs include `settlement_latency_secs`, and both listeners log `cursor_age_secs` so poller lag is visible before a merchant notices.

Control verbosity with `RUST_LOG`, e.g. `RUST_LOG=stellargate=debug,tower_http=debug`.

---

## Database Migrations

Schema is managed with [`sqlx::migrate!`](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html). Numbered SQL files in `migrations/` are applied automatically at startup, so a fresh database and an existing one converge on the same schema. sqlx records applied migrations in `_sqlx_migrations`, running each exactly once.

**Adding a migration**

1. Create `migrations/<next_number>_<description>.sql` (e.g. `0003_add_refunds.sql`).
2. Write the `CREATE TABLE` / `ALTER TABLE` statements.
3. Run `cargo test` — the suite boots against an in-memory database and applies every migration, so syntax errors surface immediately.

---

## Development

```bash
cargo build                 # compile
cargo test                  # full suite (unit + integration)
cargo fmt                   # format
cargo clippy --all-targets -- -D warnings
```

CI enforces all four on every pull request, plus a `cargo deny` supply-chain audit and a build on both the minimum supported Rust version and stable.

**Test layout**

| File | Covers |
|---|---|
| `tests/api_tests.rs` | Endpoints, validation, auth, pagination, idempotency |
| `tests/concurrency_tests.rs` | Double-settlement safety under concurrent reconciliation |
| `tests/rate_limit_tests.rs` | Per-bucket limiting |
| `tests/webhook_dispatch_tests.rs` | Signing, retries, redrive |
| `tests/trustline_tests.rs` | Asset trustline checks |

Integration tests run against an in-memory SQLite database and a [wiremock](https://github.com/LukeMathWalker/wiremock-rs) HTTP server — no network access or external services required.

---

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, coding standards, and the PR process; participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). Scoped, ready-to-pick-up issues are tracked in the [issue list](https://github.com/StellarGateLabs/StellarGate/issues).

1. Fork the repository
2. Branch: `git checkout -b feat/your-feature`
3. Make your changes **with tests**
4. Ensure `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` all pass
5. Open a pull request describing the change and its rationale

Found a security vulnerability? Please report it privately — see [SECURITY.md](SECURITY.md).

---

## License

Released under the [MIT License](LICENSE).
