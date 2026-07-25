# Webhook Delivery Management API

> This document details the webhook delivery management endpoints. For complete webhook documentation including event types, signature verification, and integration examples, see [WEBHOOK_REFERENCE.md](WEBHOOK_REFERENCE.md) (**canonical source**).

## Overview

Two endpoints expose webhook delivery history and enable manual redelivery of failed webhooks. These provide merchants with full visibility into webhook attempt history and recovery capabilities — standard for production payment gateways.

- `GET /payments/:id/webhooks` — List all webhook delivery attempts for a payment
- `POST /payments/:id/webhooks/:delivery_id/redeliver` — Manually re-attempt a failed delivery

## Database Schema

The existing `webhook_deliveries` table stores:
- `id`: Unique delivery identifier (UUID)
- `payment_id`: Reference to the payment
- `url`: Webhook URL for the merchant
- `payload`: Original signed JSON payload
- `status`: Current status (`pending`, `delivered`, `failed`)
- `attempts`: Count of delivery attempts
- `last_attempt`: Timestamp of most recent attempt
- `created_at`: Delivery creation timestamp

## API Endpoints

### GET /payments/:id/webhooks

List all webhook delivery attempts for a payment.

**Response:**
```json
{
  "payment_id": "550e8400-e29b-41d4-a716-446655440000",
  "deliveries": [
    {
      "id": "delivery-uuid",
      "url": "https://merchant.example.com/webhook",
      "status": "delivered",
      "attempts": 2,
      "last_attempt": "2026-06-22T15:30:45",
      "created_at": "2026-06-22T15:20:00"
    },
    {
      "id": "delivery-uuid-2",
      "url": "https://merchant.example.com/webhook",
      "status": "failed",
      "attempts": 3,
      "last_attempt": "2026-06-22T15:25:15",
      "created_at": "2026-06-22T15:15:00"
    }
  ]
}
```

**Error Responses:**
- `404 Not Found`: If payment does not exist
  ```json
  { "error": "payment not found" }
  ```

---

### POST /payments/:id/webhooks/:delivery_id/redeliver

Manually re-queue a webhook delivery attempt. Re-sends the original signed payload with a fresh attempt.

**Response:**
- `200 OK`: Delivery succeeded
- `502 Bad Gateway`: Delivery failed (recipient returned error or was unreachable)
  ```json
  { "error": "webhook delivery failed" }
  ```
- `404 Not Found`: Payment or delivery not found
  ```json
  { "error": "payment not found" }
  ```
  or
  ```json
  { "error": "delivery not found" }
  ```

**Behavior:**
- Re-sends the exact same signed payload (preserves authenticity)
- Records a new attempt, incrementing the attempt counter
- Honors merchant's webhook authentication (X-StellarGate-Signature header uses original payload)
- Sets status to `delivered` only if the merchant returns 2xx response
- Merchant-scoped access (once auth lands in #9)

## Implementation Details

### Code Changes

**src/db.rs**
- Added `WebhookDelivery` struct for serialization
- `list_webhook_deliveries(pool, payment_id)`: Fetch all deliveries for a payment (newest first)
- `get_webhook_delivery(pool, id)`: Fetch a specific delivery by id
- Query rows safely via `row_to_webhook_delivery()` helper

**src/api/mod.rs**
- Registered two new routes:
  - `GET /payments/:id/webhooks` → `payments::list_webhooks`
  - `POST /payments/:id/webhooks/:delivery_id/redeliver` → `payments::redeliver_webhook`

**src/api/payments.rs**
- `list_webhooks()`: Handler to retrieve delivery history with proper error handling
- `redeliver_webhook()`: Handler to re-send a delivery:
  - Validates payment exists
  - Validates delivery exists and belongs to payment
  - Recomputes signature using original payload
  - Sends via HTTP with original headers
  - Updates delivery status and attempt count
  - Returns appropriate status codes

### Test Coverage

**tests/api_tests.rs**
- `test_list_webhooks_not_found`: Returns 404 for nonexistent payment
- `test_list_webhooks_empty`: Returns empty deliveries list for payment with no webhooks
- `test_redeliver_webhook_not_found`: Returns 404 for nonexistent payment
- `test_redeliver_delivery_not_found`: Returns 404 for nonexistent delivery
- `test_webhook_delivery_isolation`: Verifies deliveries from one payment are not accessible from another

## Future Work

1. **Merchant Scoping (#9)**: Once authentication lands, scope list/redeliver endpoints to payment owner
2. **Delivery Status Dashboard**: Add filtering by status or date range
3. **Webhook Event Log**: Store request/response details for debugging
4. **Automatic Retry Policy**: Enhance retry strategy with exponential backoff
5. **Delivery Webhooks**: Notify merchants of delivery confirmation events

## Error Handling

All endpoints follow the standard error response format:
```json
{ "error": "descriptive message" }
```

Errors are:
- Logged internally (never leaked to client)
- Returned with appropriate HTTP status codes (404 for not found, 502 for delivery failures)
- Safe for untrusted input (no path traversal, properly parameterized queries)
