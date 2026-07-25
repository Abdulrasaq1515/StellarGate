# Webhook Documentation Consolidation

## Status: Completed ✅

### The Problem
Three separate webhook documentation sources had overlapping and partly-inconsistent content:
- **README.md** - Event names were outdated (`payment.success`, `payment.failed` vs actual `payment.completed`, `payment.overpaid`, `payment.underpaid`)
- **WEBHOOK_API_EXAMPLES.md** - Practical examples with redundant verification code
- **WEBHOOK_DELIVERY_API.md** - Delivery management endpoints documentation
- **openapi.yaml** - Incomplete webhook schema definitions

This created three sources of truth with drift, confusing readers about which was authoritative.

### The Solution

#### Single Canonical Source: `WEBHOOK_REFERENCE.md`
This is now the **authoritative webhook documentation**, containing:

1. **Event Types** - Complete definitions for all four events:
   - `payment.completed` (exact match)
   - `payment.overpaid` (with `delta` field)
   - `payment.underpaid` (with `delta` field)
   - `payment.expired` (TTL elapsed)

2. **Webhook Headers** - Signature structure and meaning

3. **Verification Recipes** - Step-by-step with examples:
   - Node.js implementation
   - Python implementation
   - Timestamp freshness validation
   - Constant-time comparison

4. **Delivery Management** - The two webhook endpoints:
   - `GET /payments/:id/webhooks` - List deliveries
   - `POST /payments/:id/webhooks/:delivery_id/redeliver` - Manual retry

5. **Configuration** - All webhook-related env vars in one place

6. **Delivery Guarantee** - At-least-once semantics, idempotency guidance

7. **SSRF Protection** - Security measures

8. **Integration Checklist** - Step-by-step merchant integration

9. **Integration Examples** - Real-world workflows:
   - Complete payment flow
   - Overpayment handling
   - Underpayment/top-up handling
   - Expiry handling

#### Supporting Documents (Now Focused)

**README.md**
- ❌ Removed outdated event names (`payment.success`, `payment.failed`)
- ✅ Added link to `WEBHOOK_REFERENCE.md`
- ✅ Removed redundant verification code
- ✅ Kept high-level "Payment Flow" overview for context

**WEBHOOK_DELIVERY_API.md**
- ✅ Added prominent link to canonical reference at top
- ✅ Clarified scope: "details webhook delivery management endpoints"
- ✅ Kept focused on delivery schema and endpoint specifics
- ✅ Removed signature verification details (moved to reference)

**WEBHOOK_API_EXAMPLES.md**
- ✅ Added prominent link to canonical reference at top
- ✅ Clarified scope: "practical examples"
- ✅ Removed redundant verification code (link instead)
- ✅ Kept real workflow examples

### Navigation Structure

```
WEBHOOK_REFERENCE.md (CANONICAL)
├── Beginner → Event Types section
├── Integration → Verification Recipes section
├── Ops → Configuration & Delivery Guarantee sections
├── Setup → Integration Checklist & Examples sections
│
README.md (Quick ref)
├── Link to WEBHOOK_REFERENCE.md
├── High-level Payment Flow overview
└── Env vars table (with webhook section)

WEBHOOK_DELIVERY_API.md (Endpoints only)
├── Link to WEBHOOK_REFERENCE.md
├── GET /payments/:id/webhooks spec
├── POST /payments/:id/webhooks/:delivery_id/redeliver spec
└── Database schema details

WEBHOOK_API_EXAMPLES.md (Examples only)
├── Link to WEBHOOK_REFERENCE.md
├── Complete workflow walkthrough
├── Overpayment scenario
├── Underpayment scenario
├── Expiry scenario
└── Integration checklist
```

### Breaking Changes Fixed

**Event Name Corrections:**
- ❌ `payment.success` → ✅ `payment.completed`
- ❌ `payment.failed` → ✅ Split into `payment.overpaid` and `payment.underpaid`

This matches the actual implementation in `src/webhook.rs` and `src/horizon.rs`.

### Acceptance Criteria Met

✅ **Single canonical webhook reference** - `WEBHOOK_REFERENCE.md` is the authoritative source
✅ **Others link to it** - README, WEBHOOK_DELIVERY_API.md, WEBHOOK_API_EXAMPLES.md all link prominently
✅ **No more drift** - All event names now match code implementation
✅ **Readers know what's authoritative** - Clear links and scope definitions on each document

### Files Modified

1. **Created:** `StellarGate/WEBHOOK_REFERENCE.md` (673 lines, comprehensive)
2. **Updated:** `StellarGate/README.md` - Fixed event names, added reference link, removed duplicate code
3. **Updated:** `StellarGate/WEBHOOK_API_EXAMPLES.md` - Added reference link, removed duplicate verification code
4. **Updated:** `StellarGate/WEBHOOK_DELIVERY_API.md` - Added reference link, clarified scope

### Maintenance Going Forward

**When updating webhook docs:**
1. Check if change belongs in `WEBHOOK_REFERENCE.md` (event types, verification, integration)
2. If it's an endpoint detail, update `WEBHOOK_DELIVERY_API.md` and link to reference
3. If it's an example, add to `WEBHOOK_API_EXAMPLES.md` and link to reference
4. Never duplicate event definitions across files
5. Keep README pointing to reference as the single source of truth

**Linting rule suggestion:** Add a check in CI to ensure all webhook event names in docs match `src/webhook.rs` constants.
