//! Money handling for Stellar amounts.
//!
//! Stellar represents every balance as an integer number of *stroops*, where
//! `1 unit = 10_000_000 stroops` (7 decimal places). Doing arithmetic and
//! comparisons in stroops avoids the rounding pitfalls of binary floating point,
//! which is why we never compare payment amounts as `f64`.

/// Number of stroops in one whole unit of any Stellar asset.
pub const STROOPS_PER_UNIT: i64 = 10_000_000;

/// Maximum number of decimal places a Stellar amount may carry.
pub const MAX_DECIMALS: usize = 7;

/// Parse a positive decimal amount string into stroops.
///
/// Returns `None` if the value is empty, malformed, signed, non-positive,
/// carries more than [`MAX_DECIMALS`] decimal places, or overflows `i64`.
///
/// ```
/// use stellargate::money::parse_stroops;
/// assert_eq!(parse_stroops("1"), Some(10_000_000));
/// assert_eq!(parse_stroops("0.0000001"), Some(1));
/// assert_eq!(parse_stroops("10.50"), Some(105_000_000));
/// assert_eq!(parse_stroops("0"), None);
/// assert_eq!(parse_stroops("-1"), None);
/// assert_eq!(parse_stroops("1.00000001"), None);
/// ```
pub fn parse_stroops(input: &str) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };

    // Reject signs, whitespace, exponents — only plain digits in each segment.
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if frac_part.len() > MAX_DECIMALS {
        return None;
    }

    let int_val: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };

    // Right-pad the fractional part to exactly 7 digits so it reads as stroops.
    let mut frac = String::with_capacity(MAX_DECIMALS);
    frac.push_str(frac_part);
    while frac.len() < MAX_DECIMALS {
        frac.push('0');
    }
    let frac_val: i64 = frac.parse().ok()?;

    let stroops = int_val
        .checked_mul(STROOPS_PER_UNIT)?
        .checked_add(frac_val)?;

    if stroops <= 0 {
        return None;
    }
    Some(stroops)
}

/// Returns `true` if `input` is a valid, strictly-positive Stellar amount.
pub fn is_valid_amount(input: &str) -> bool {
    parse_stroops(input).is_some()
}

/// Format a stroop count as a minimal-decimal Stellar amount string.
///
/// Trailing fractional zeros are stripped so the result is compact:
/// `10_000_000` → `"1"`, `15_500_000` → `"1.55"`, `5_000_000` → `"0.5"`.
pub fn stroops_to_string(stroops: i64) -> String {
    let whole = stroops / STROOPS_PER_UNIT;
    let frac = stroops % STROOPS_PER_UNIT;
    if frac == 0 {
        format!("{whole}")
    } else {
        let padded = format!("{frac:07}");
        let trimmed = padded.trim_end_matches('0');
        format!("{whole}.{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whole_and_fractional() {
        assert_eq!(parse_stroops("1"), Some(10_000_000));
        assert_eq!(parse_stroops("10"), Some(100_000_000));
        assert_eq!(parse_stroops("10.00"), Some(100_000_000));
        assert_eq!(parse_stroops("10.50"), Some(105_000_000));
        assert_eq!(parse_stroops("0.0000001"), Some(1));
        assert_eq!(parse_stroops(".5"), Some(5_000_000));
        assert_eq!(parse_stroops("  2.5  "), Some(25_000_000));
    }

    #[test]
    fn rejects_invalid() {
        assert_eq!(parse_stroops(""), None);
        assert_eq!(parse_stroops("0"), None);
        assert_eq!(parse_stroops("0.0"), None);
        assert_eq!(parse_stroops("-1"), None);
        assert_eq!(parse_stroops("+1"), None);
        assert_eq!(parse_stroops("abc"), None);
        assert_eq!(parse_stroops("1.2.3"), None);
        assert_eq!(parse_stroops("1e3"), None);
        assert_eq!(parse_stroops("1.00000001"), None); // 8 decimals
        assert_eq!(parse_stroops("9999999999999999999"), None); // overflow
    }

    #[test]
    fn stroops_to_string_works() {
        assert_eq!(stroops_to_string(10_000_000), "1");
        assert_eq!(stroops_to_string(100_000_000), "10");
        assert_eq!(stroops_to_string(15_000_000), "1.5");
        assert_eq!(stroops_to_string(15_500_000), "1.55");
        assert_eq!(stroops_to_string(5_000_000), "0.5");
        assert_eq!(stroops_to_string(1), "0.0000001");
        assert_eq!(stroops_to_string(105_000_000), "10.5");
    }

    #[test]
    fn comparisons_are_exact() {
        // The classic float trap: 0.1 + 0.2 != 0.3 in f64, but exact in stroops.
        let a = parse_stroops("0.1").unwrap() + parse_stroops("0.2").unwrap();
        assert_eq!(a, parse_stroops("0.3").unwrap());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Strategy for generating valid stroops values (positive i64 that won't overflow)
    fn valid_stroops() -> impl Strategy<Value = i64> {
        1i64..=(i64::MAX / 2)
    }

    // Strategy for generating valid amount strings with 0-7 decimals
    fn valid_amount_string() -> impl Strategy<Value = String> {
        (0u64..1_000_000_000u64, 0usize..=MAX_DECIMALS).prop_map(|(whole, decimals)| {
            if decimals == 0 {
                format!("{}", whole)
            } else {
                let frac_max = 10u64.pow(decimals as u32);
                let frac = whole % frac_max;
                let frac_str = format!("{:0width$}", frac, width = decimals);
                format!("{}.{}", whole / frac_max, frac_str)
            }
        })
    }

    // Strategy for generating arbitrary strings (including adversarial)
    fn arbitrary_string() -> impl Strategy<Value = String> {
        prop::string::string_regex(".*").unwrap()
    }

    proptest! {
        /// Property: Round-trip
        /// For all valid stroops s, parse_stroops(&stroops_to_string(s)) == Some(s)
        #[test]
        fn prop_round_trip(stroops in valid_stroops()) {
            let formatted = stroops_to_string(stroops);
            let parsed = parse_stroops(&formatted);
            prop_assert_eq!(
                parsed,
                Some(stroops),
                "Round-trip failed: {} -> '{}' -> {:?}",
                stroops,
                formatted,
                parsed
            );
        }

        /// Property: Ordering preservation
        /// For all valid stroops a, b where a < b, their string representations
        /// should parse back such that parsed(a) < parsed(b)
        #[test]
        fn prop_ordering(a in valid_stroops(), b in valid_stroops()) {
            let a_str = stroops_to_string(a);
            let b_str = stroops_to_string(b);

            let a_parsed = parse_stroops(&a_str).unwrap();
            let b_parsed = parse_stroops(&b_str).unwrap();

            // Ordering must be preserved
            if a < b {
                prop_assert!(
                    a_parsed < b_parsed,
                    "Ordering not preserved: {} < {} but parsed {} >= {}",
                    a,
                    b,
                    a_parsed,
                    b_parsed
                );
            } else if a > b {
                prop_assert!(
                    a_parsed > b_parsed,
                    "Ordering not preserved: {} > {} but parsed {} <= {}",
                    a,
                    b,
                    a_parsed,
                    b_parsed
                );
            } else {
                prop_assert_eq!(
                    a_parsed,
                    b_parsed,
                    "Equal values should parse to equal stroops"
                );
            }
        }

        /// Property: Canonicalization is idempotent
        /// Formatting a parsed amount twice yields the same string
        #[test]
        fn prop_canonicalization_idempotent(s in valid_amount_string()) {
            if let Some(stroops) = parse_stroops(&s) {
                let canonical1 = stroops_to_string(stroops);
                let stroops2 = parse_stroops(&canonical1).unwrap();
                let canonical2 = stroops_to_string(stroops2);

                prop_assert_eq!(
                    canonical1,
                    canonical2,
                    "Canonicalization not idempotent: '{}' -> '{}' -> '{}'",
                    s,
                    canonical1,
                    canonical2
                );
            }
        }

        /// Property: Totality (never panics)
        /// parse_stroops returns None or Some(positive) for any input, never panics
        #[test]
        fn prop_totality_no_panic(s in arbitrary_string()) {
            let result = std::panic::catch_unwind(|| parse_stroops(&s));
            prop_assert!(result.is_ok(), "parse_stroops panicked on input: {:?}", s);

            if let Ok(Some(stroops)) = result {
                prop_assert!(
                    stroops > 0,
                    "parse_stroops returned non-positive value: {}",
                    stroops
                );
            }
        }

        /// Property: Totality (structured inputs)
        /// For structured valid inputs, parsing always succeeds and returns positive
        #[test]
        fn prop_totality_valid_inputs(s in valid_amount_string()) {
            match parse_stroops(&s) {
                Some(stroops) => prop_assert!(
                    stroops > 0,
                    "Valid input '{}' parsed to non-positive: {}",
                    s,
                    stroops
                ),
                None => prop_assert!(
                    false,
                    "Valid input '{}' unexpectedly failed to parse",
                    s
                ),
            }
        }

        /// Property: Parse then format yields minimal representation
        /// Formatting a parsed amount always yields the minimal string (no trailing zeros)
        #[test]
        fn prop_minimal_representation(stroops in valid_stroops()) {
            let formatted = stroops_to_string(stroops);

            // Should not end with ".0" or ".00" etc unless it's just "X" with no decimal
            if formatted.contains('.') {
                prop_assert!(
                    !formatted.ends_with('0') || formatted.ends_with(".0"),
                    "Formatted string has trailing zeros: '{}'",
                    formatted
                );
                // Actually, it should NEVER end with 0 because we trim them
                prop_assert!(
                    !formatted.ends_with('0'),
                    "Formatted string has trailing zeros: '{}'",
                    formatted
                );
            }
        }

        /// Property: Signed values always rejected
        #[test]
        fn prop_reject_signed(n in -1000000i64..0i64) {
            let s = format!("{}", n);
            prop_assert_eq!(
                parse_stroops(&s),
                None,
                "Negative value '{}' should be rejected",
                s
            );

            let s_plus = format!("+{}", n.abs());
            prop_assert_eq!(
                parse_stroops(&s_plus),
                None,
                "Explicitly signed positive value '{}' should be rejected",
                s_plus
            );
        }

        /// Property: Zero always rejected
        #[test]
        fn prop_reject_zero(
            zeros in prop::sample::select(vec!["0", "0.0", "0.00", "0.000", ".0", "00"])
        ) {
            prop_assert_eq!(
                parse_stroops(zeros),
                None,
                "Zero variant '{}' should be rejected",
                zeros
            );
        }

        /// Property: Too many decimals rejected
        #[test]
        fn prop_reject_excess_decimals(
            whole in 0u64..1000u64,
            decimals in 8usize..15usize
        ) {
            let frac_str = "1".repeat(decimals);
            let s = format!("{}.{}", whole, frac_str);
            prop_assert_eq!(
                parse_stroops(&s),
                None,
                "Input with {} decimals '{}' should be rejected",
                decimals,
                s
            );
        }

        /// Property: Overflow detection
        #[test]
        fn prop_overflow_detection(n in (i64::MAX / STROOPS_PER_UNIT + 1)..=i64::MAX) {
            let s = format!("{}", n);
            prop_assert_eq!(
                parse_stroops(&s),
                None,
                "Overflow value '{}' should be rejected",
                s
            );
        }

        /// Property: Whitespace handling
        #[test]
        fn prop_whitespace_trimmed(
            stroops in valid_stroops(),
            prefix in prop::string::string_regex("[ \\t]+").unwrap(),
            suffix in prop::string::string_regex("[ \\t]+").unwrap()
        ) {
            let formatted = stroops_to_string(stroops);
            let with_ws = format!("{}{}{}", prefix, formatted, suffix);

            prop_assert_eq!(
                parse_stroops(&with_ws),
                Some(stroops),
                "Whitespace-wrapped '{}' should parse same as '{}'",
                with_ws,
                formatted
            );
        }

        /// Property: Consistency with is_valid_amount
        #[test]
        fn prop_is_valid_amount_consistency(s in arbitrary_string()) {
            let is_valid = is_valid_amount(&s);
            let parsed = parse_stroops(&s);

            prop_assert_eq!(
                is_valid,
                parsed.is_some(),
                "is_valid_amount and parse_stroops disagree on '{}'",
                s
            );
        }

        /// Property: Fractional-only amounts
        #[test]
        fn prop_fractional_only(
            frac in 1u32..10_000_000u32,
            decimals in 1usize..=MAX_DECIMALS
        ) {
            let frac_str = format!("{:0width$}", frac, width = decimals);
            let s = format!(".{}", frac_str);

            let result = parse_stroops(&s);
            prop_assert!(
                result.is_some(),
                "Fractional-only amount '{}' should parse",
                s
            );

            if let Some(stroops) = result {
                prop_assert!(
                    stroops > 0 && stroops < STROOPS_PER_UNIT,
                    "Fractional-only '{}' should yield 0 < stroops < {}, got {}",
                    s,
                    STROOPS_PER_UNIT,
                    stroops
                );
            }
        }
    }
}

