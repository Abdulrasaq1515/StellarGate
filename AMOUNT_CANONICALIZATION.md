# Amount Canonicalization Implementation

## Problem Statement

Amounts were stored and returned exactly as the client sent them, causing spurious string-based differences:
- `"10.00"`, `"10.0"`, and `"10"` are numerically identical (100,000,000 stroops)
- Clients comparing amounts as strings across create/get/webhook responses saw different values for the same amount
- This violates the principle that a given value should always serialize identically

**Acceptance Criteria:** A given value always serializes identically.

## Solution

Implemented canonical amount serialization at three layers:

### 1. **Write-Time Canonicalization** (`src/db.rs` - `create_payment()`)
When a new payment intent is created, the amount is:
1. Parsed to stroops (integer representation): `parse_stroops("10.00")` → `100,000,000`
2. Converted back to canonical form: `stroops_to_string(100,000,000)` → `"10"`

This ensures every amount stored in the database is in minimal form (no trailing zeros).

**Key benefit:** Eliminates spurious differences at the source; all stored amounts are guaranteed canonical.

### 2. **Response-Time Canonicalization** (`src/api/payments.rs` - `to_json()`)
When a payment is serialized for HTTP responses (create, get, list endpoints):
- `amount` field is canonicalized via the same stroops round-trip
- `paid_amount` field is canonicalized the same way (defensive; should already be canonical from `horizon.rs`)

**Key benefit:** Defensive layer ensures consistency even if legacy data exists.

### 3. **Webhook Payload Canonicalization** (`src/webhook.rs` - `build_payload()`)
Webhook event payloads canonicalize:
- `amount` field (requested amount)
- `paid_amount` field (cumulative received amount)
- `delta` field (difference for overpaid/underpaid events)

**Key benefit:** Ensures webhook recipients always see canonical amounts, regardless of internal representation or legacy data.

## Implementation Details

### Why Three Layers?

This is a belt-and-suspenders approach following senior development principles:

1. **Write-layer canonicalization** (primary): Prevents non-canonical amounts from ever entering the database. Fast, simple, solves the problem at the source.

2. **Response-layer canonicalization** (defensive): Protects against:
   - Legacy amounts in the database (from before this fix)
   - Any other code path that might bypass the write layer
   - Accidental future regressions

3. **Webhook-layer canonicalization** (hardening): Ensures external consumers (merchants) never see inconsistent serialization, even if internal representation changes.

### Amount Formatting Function

The `stroops_to_string()` function in `src/money.rs` already implements canonical formatting:
- Strips trailing zeros: `10.5000000` → `"10.5"`
- Omits decimal point for whole amounts: `10.0` → `"10"`
- Handles fractional stroops correctly: `1` stroop → `"0.0000001"`

Example canonicalizations:
```
"10" → "10"
"10.0" → "10"
"10.00" → "10"
"10.5" → "10.5"
"10.50" → "10.5"
"10.500" → "10.5"
"10.0000001" → "10.0000001"
```

### Error Handling

In `create_payment()`, if the amount is invalid:
- `parse_stroops()` returns `None` (invalid format, too many decimals, overflow, etc.)
- We return `Err(anyhow::anyhow!("Invalid amount"))`
- The error bubbles up as a 500 (internal server error)

Note: This should never happen in practice because the request handler validates amounts via `money::is_valid_amount()` before reaching `create_payment()`. This is a safety backstop.

## Test Coverage

Added two comprehensive tests in `tests/api_tests.rs`:

### `test_amount_canonicalization_on_create_get_list()`
Tests that various representations of "10.5" all serialize to "10.5":
- `POST /payments` returns canonical form in 201 response
- `GET /payments/:id` returns canonical form
- `GET /payments?limit=100` returns canonical form in list

Test cases: `10.5`, `10.50`, `10.500`, `10.5000`, `10.50000`, `10.500000`, `10.5000000`

### `test_whole_amount_canonicalization()`
Tests that whole amounts serialize without decimal point:
- `POST /payments` with `"1"`, `"1.0"`, `"1.00"`, `"100"`, `"100.0000000"` all return `"1"`, `"1"`, `"1"`, `"100"`, `"100"` respectively

## Files Changed

### `src/db.rs` - `create_payment()`
- Added stroop canonicalization before database insert
- Amount is now stored in canonical form

### `src/api/payments.rs` - `to_json()`
- Defensively canonicalizes `amount` and `paid_amount` on serialization
- Ensures consistency across all HTTP response paths

### `src/webhook.rs` - `build_payload()`
- Canonicalizes `amount`, `paid_amount`, and `delta` fields
- Ensures webhook recipients always see consistent representations

### `tests/api_tests.rs`
- Added `test_amount_canonicalization_on_create_get_list()`
- Added `test_whole_amount_canonicalization()`

## Verification

The implementation satisfies the acceptance criterion:

> **A given value always serializes identically.**

- ✅ Create response: canonical form
- ✅ Get response: canonical form
- ✅ List response: canonical form
- ✅ Webhook payload: canonical form
- ✅ All representations of the same value serialize identically

## Performance Impact

Minimal. The canonical serialization adds:
- **Write time**: One stroop parse + format (microseconds per write)
- **Read time**: One stroop parse + format per serialization (microseconds per read)

Since Stellar amounts are restricted to 7 decimal places, parsing/formatting is O(1) with bounded string operations. No loops, no network calls, no database operations.

## Backward Compatibility

- ✅ Incoming amount formats: Still accept "10", "10.0", "10.00", etc.
- ✅ Existing stored amounts: Still retrieved correctly (the schema stores amounts as TEXT)
- ✅ Responses: May differ from pre-fix behavior only in trailing zeros (customers should normalize client-side, but this is an improvement for consistency)
- ✅ Webhooks: May differ slightly but always canonical (customers receiving webhooks should treat amounts as equivalent if they normalize both values to stroops for comparison)

## Future Considerations

1. **Database migration**: Could add a post-deployment task to canonicalize legacy amounts, but not necessary since responses normalize on read
2. **API versioning**: No breaking change—this is a bug fix to ensure consistent semantics
3. **Amount type**: Could consider storing amounts as i64 (stroops) internally instead of TEXT, but that's beyond this scope
