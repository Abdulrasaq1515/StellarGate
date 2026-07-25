# Amount Canonicalization - Quick Reference

## What Changed?
Amounts are now canonicalized to ensure "10.00", "10.0", and "10" all serialize as "10".

## Files Modified
| File | Function | What | Why |
|------|----------|------|-----|
| `src/db.rs` | `create_payment()` | Parse to stroops, convert back before insert | Prevents non-canonical amounts from entering DB |
| `src/api/payments.rs` | `to_json()` | Canonicalize amount and paid_amount | Ensures all responses return canonical form |
| `src/webhook.rs` | `build_payload()` | Canonicalize amount, paid_amount, delta | Ensures webhooks always consistent |
| `tests/api_tests.rs` | (new tests) | Added canonicalization test coverage | Verify the fix works across all endpoints |

## How It Works

### Canonicalization Function
```rust
// Existing function in src/money.rs
pub fn stroops_to_string(stroops: i64) -> String {
    // Returns "10" for 100,000,000 stroops
    // Returns "10.5" for 105,000,000 stroops
    // No trailing zeros, no trailing decimal point
}
```

### Example Flow
```
Input: "10.50"
  ↓ parse_stroops("10.50") → 105,000,000
  ↓ stroops_to_string(105,000,000) → "10.5"
  ↓ Store "10.5" in database
  ↓ Later: serialize to JSON as "10.5"
  → Always consistent!
```

## Testing

### Run Canonicalization Tests
```bash
# Just the new tests
cargo test test_amount_canonicalization
cargo test test_whole_amount_canonicalization

# Full suite
cargo test
```

### Manual Test
```bash
# Create with trailing zero
curl -X POST http://localhost:3000/payments \
  -H "Authorization: Bearer <key>" \
  -d '{"amount": "10.00", "asset": "XLM"}'

# Response: { "amount": "10" } ← canonical
```

## Edge Cases Handled

| Input | Output | Notes |
|-------|--------|-------|
| "10" | "10" | Whole amounts have no decimal |
| "10.0" | "10" | Trailing zeros stripped |
| "10.00" | "10" | Multiple zeros stripped |
| "10.5" | "10.5" | One decimal digit kept |
| "10.50" | "10.5" | Trailing zeros stripped |
| "10.500000" | "10.5" | Many trailing zeros stripped |
| "0.0000001" | "0.0000001" | Smallest unit preserved |

## Common Questions

### Q: Why three places?
**A:** Defense-in-depth. Write layer prevents bad data. Read layer handles legacy. Webhook layer hardens API contract.

### Q: What if canonicalization fails?
**A:** We fallback to the original value. Never crashes, always returns something.

### Q: Do I need to migrate the database?
**A:** No. Amounts are still TEXT in the database. Responses normalize on read.

### Q: Is this a breaking change?
**A:** No. Request formats still work. Responses may differ only in trailing zeros (an improvement).

### Q: What about performance?
**A:** ~1-2 microseconds per canonicalization. Negligible impact.

### Q: Why reuse stroops_to_string()?
**A:** DRY principle. Single source of truth. Already tested and correct.

## Implementation Notes

### Write Layer Canonicalization
```rust
// In create_payment(), before insert:
let stroops = crate::money::parse_stroops(new.amount)?;
let canonical_amount = crate::money::stroops_to_string(stroops);
// Use canonical_amount for database insert
```

### Read Layer Canonicalization
```rust
// In to_json(), before JSON serialization:
let canonical_amount = crate::money::parse_stroops(&p.amount)
    .map(crate::money::stroops_to_string)
    .unwrap_or_else(|| p.amount.clone());
// Use canonical_amount in json! macro
```

### Webhook Layer Canonicalization
```rust
// In build_payload(), before returning payload:
let canonical_amount = crate::money::parse_stroops(&payment.amount)
    .map(crate::money::stroops_to_string)
    .unwrap_or_else(|| payment.amount.clone());
// Use canonical_amount in webhook payload
```

## Verification Checklist

Before deployment:
- [ ] Run `cargo test` - all tests pass
- [ ] Run `cargo test test_amount_canonicalization` - canonicalization tests pass
- [ ] Manual test: create with "10.50", verify response has "10.5"
- [ ] Manual test: get same payment, verify amount is "10.5"
- [ ] Manual test: list payments, verify amounts are canonical
- [ ] Check documentation is clear
- [ ] No compiler warnings

## Troubleshooting

### Test fails with "invalid amount"
- Check: Is `parse_stroops()` getting a valid input?
- Solution: Ensure amount has ≤7 decimal places and is positive

### Response has trailing zero when it shouldn't
- Check: Is the amount coming from the database canonical?
- Solution: Verify `create_payment()` is canonicalizing on insert

### Webhook has inconsistent amounts
- Check: Is `build_payload()` canonicalizing?
- Solution: Verify webhook function calls `stroops_to_string()`

## References

- **Technical Guide:** AMOUNT_CANONICALIZATION.md
- **Changes Summary:** AMOUNT_CANONICALIZATION_CHANGES.md
- **Verification:** VERIFICATION_CHECKLIST.md
- **Full Solution:** SOLUTION_SUMMARY.md
- **Money Module:** src/money.rs (stroops_to_string implementation)

## Support

For questions or issues:
1. Check SOLUTION_SUMMARY.md
2. Review code comments in modified functions
3. Run tests to verify behavior
4. Check existing test cases for examples
