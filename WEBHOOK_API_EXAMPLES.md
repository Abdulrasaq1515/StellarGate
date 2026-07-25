# Webhook Delivery API — Usage Examples

> This document provides practical examples. For complete webhook documentation including all event types, signature verification details, and configuration options, see [WEBHOOK_REFERENCE.md](WEBHOOK_REFERENCE.md) (**canonical source**).

## 1. List Webhook Deliveries

Retrieve all delivery attempts for a payment.

```bash
curl -X GET http://localhost:3000/payments/550e8400-e29b-41d4-a716-446655440000/webhooks
```

**Success Response (200 OK):**
```json
{
  "payment_id": "550e8400-e29b-41d4-a716-446655440000",
  "deliveries": [
    {
      "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
      "url": "https://merchant.example.com/webhook",
      "status": "delivered",
      "attempts": 1,
      "last_attempt": "2026-06-22T15:30:45",
      "created_at": "2026-06-22T15:30:00"
    },
    {
      "id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
      "url": "https://merchant.example.com/webhook",
      "status": "failed",
      "attempts": 3,
      "last_attempt": "2026-06-22T15:25:15",
      "created_at": "2026-06-22T15:20:00"
    }
  ]
}
```

**Error Response (404 Not Found) — Payment doesn't exist:**
```json
{ "error": "payment not found" }
```

---

## 2. Manually Redeliver a Webhook

Resend a specific delivery attempt.

```bash
curl -X POST http://localhost:3000/payments/550e8400-e29b-41d4-a716-446655440000/webhooks/f47ac10b-58cc-4372-a567-0e02b2c3d479/redeliver
```

**Success Response (200 OK):**
```
(empty body with HTTP 200)
```

**Error Response (502 Bad Gateway) — Webhook delivery failed:**
```json
{ "error": "webhook delivery failed" }
```

**Error Response (404 Not Found) — Delivery doesn't exist:**
```json
{ "error": "delivery not found" }
```

**Error Response (404 Not Found) — Payment doesn't exist:**
```json
{ "error": "payment not found" }
```

---

## 3. Complete Workflow Example

### Step 1: Create a Payment
```bash
curl -X POST http://localhost:3000/payments \
  -H "Content-Type: application/json" \
  -d '{
    "amount": "100.0",
    "asset": "XLM",
    "webhook_url": "https://merchant.example.com/webhook"
  }'
```

Response:
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "merchant_id": "anonymous",
  "status": "pending",
  "memo": "ABC12345",
  ...
}
```

### Step 2: Check Webhook Delivery Status
```bash
curl http://localhost:3000/payments/550e8400-e29b-41d4-a716-446655440000/webhooks
```

(Returns list of deliveries once payment is confirmed)

### Step 3: If Delivery Failed, Redeliver
```bash
curl -X POST http://localhost:3000/payments/550e8400-e29b-41d4-a716-446655440000/webhooks/[delivery-id]/redeliver
```

---

## Webhook Signature Verification

See [WEBHOOK_REFERENCE.md — Verifying Webhooks](WEBHOOK_REFERENCE.md#verifying-webhooks) for complete verification guidance with examples in Node.js and Python, including:
- Timestamp freshness validation
- Exact signature computation
- Constant-time comparison patterns

---

## Key Points

1. **Redeliver preserves the original payload** — The signature remains valid because we re-compute it from the exact original bytes
2. **Attempts counter increments** — Each redeliver attempt increases the `attempts` count and updates `last_attempt`
3. **Delivery isolation** — You can only see/redeliver deliveries for payments you own (once merchant auth lands)
4. **Standard errors** — All error responses follow `{ "error": "message" }` format with appropriate HTTP status codes

---

## Integration Checklist

- [ ] Fetch webhook history after payment confirmation
- [ ] Display delivery status and attempt count to merchant
- [ ] Provide manual redeliver button for failed deliveries
- [ ] Log redelivery attempts for audit trail
- [ ] Add filtering by status (pending/delivered/failed) when pagination lands
- [ ] Scope endpoints to merchant_id once auth is implemented
