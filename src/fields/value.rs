//! Value kinds: how a field's raw text is validated and parsed.
//!
//! A locator can land on the wrong text — a label-anchored match especially,
//! since it has no fixed position to confirm it found the right thing. The
//! [`ValueKind`] is what catches that: a value that does not fit its
//! expected shape (wrong digit count, bad checksum, an impossible date)
//! rejects rather than silently returning garbage.

use super::FieldError;

/// How to interpret and validate a field's raw text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Deserialize), serde(rename_all = "camelCase"))]
pub enum ValueKind {
    /// No validation: the trimmed raw text as-is.
    Text,
    /// A 13-digit Thai tax ID (นิติบุคคล) or citizen ID, checksum-validated
    /// with the Revenue Department's mod-11 check digit.
    ThaiTaxId,
    /// A calendar date, in Thai (พ.ศ.) or Gregorian, returned as Gregorian.
    /// Accepts a Thai month name (`10 สิงหาคม 2569`) or a numeric `D/M/Y`
    /// date (`10/08/2569`).
    Date,
    /// A decimal amount, tolerant of thousands separators and a trailing
    /// currency word or symbol (`100,000.00 บาท`).
    Amount,
}

/// A field's parsed value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "json",
    derive(serde::Serialize),
    serde(tag = "kind", content = "value", rename_all = "camelCase")
)]
pub enum Value {
    /// [`ValueKind::Text`].
    Text(String),
    /// [`ValueKind::ThaiTaxId`], the 13 digits with separators stripped.
    ThaiTaxId(String),
    /// [`ValueKind::Date`], in Gregorian.
    Date {
        /// Gregorian year.
        year: i32,
        /// 1-12.
        month: u8,
        /// 1-31, within `month`'s length.
        day: u8,
    },
    /// [`ValueKind::Amount`].
    Amount(f64),
}

/// Parse and validate `raw` per `kind`. `raw` is expected non-empty and
/// already trimmed by the locator that found it.
pub(super) fn parse(kind: ValueKind, raw: &str) -> Result<Value, FieldError> {
    match kind {
        ValueKind::Text => Ok(Value::Text(raw.trim().to_string())),
        ValueKind::ThaiTaxId => parse_thai_tax_id(raw),
        ValueKind::Date => parse_date(raw),
        ValueKind::Amount => parse_amount(raw),
    }
}

// ── Thai tax ID ──────────────────────────────────────────────────────────

/// Validates the 13-digit check digit the Revenue Department (and the
/// national ID registry, which uses the same scheme) assigns: weight digit
/// `i` (1-based, `i` in 1..=12) by `14 - i`, sum, and the check digit is
/// `(11 - sum % 11) % 10`.
fn parse_thai_tax_id(raw: &str) -> Result<Value, FieldError> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 13 {
        return Err(FieldError::invalid(raw, "expected 13 digits"));
    }

    let values: Vec<u32> = digits.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let sum: u32 = values[..12].iter().enumerate().map(|(i, d)| d * (13 - i as u32)).sum();
    let check_digit = (11 - sum % 11) % 10;

    if check_digit != values[12] {
        return Err(FieldError::invalid(raw, "check digit does not match"));
    }
    Ok(Value::ThaiTaxId(digits))
}

// ── Date ─────────────────────────────────────────────────────────────────

const FULL_MONTHS: [&str; 12] = [
    "มกราคม",
    "กุมภาพันธ์",
    "มีนาคม",
    "เมษายน",
    "พฤษภาคม",
    "มิถุนายน",
    "กรกฎาคม",
    "สิงหาคม",
    "กันยายน",
    "ตุลาคม",
    "พฤศจิกายน",
    "ธันวาคม",
];

const ABBR_MONTHS: [&str; 12] = [
    "ม.ค.",
    "ก.พ.",
    "มี.ค.",
    "เม.ย.",
    "พ.ค.",
    "มิ.ย.",
    "ก.ค.",
    "ส.ค.",
    "ก.ย.",
    "ต.ค.",
    "พ.ย.",
    "ธ.ค.",
];

fn parse_date(raw: &str) -> Result<Value, FieldError> {
    let trimmed = raw.trim();
    let (day, month, year) = parse_thai_month_date(trimmed)
        .or_else(|| parse_numeric_date(trimmed))
        .ok_or_else(|| FieldError::invalid(raw, "not a recognizable date"))?;

    // Thai documents almost always give the year in the Buddhist Era
    // (พ.ศ. = ค.ศ. + 543); a modern Gregorian year never reaches this range,
    // so the threshold disambiguates without a per-document flag. A
    // two-digit year carries no such tell — `69` is พ.ศ. 2569 on a Thai form
    // and `26` is ค.ศ. 2026 on an English one, 43 years apart — so it falls
    // through to the range check below and is rejected rather than guessed.
    let year = if year > 2400 { year - 543 } else { year };
    if year < 1000 {
        return Err(FieldError::invalid(raw, "year must be four digits"));
    }

    if !(1..=12).contains(&month) {
        return Err(FieldError::invalid(raw, "month out of range"));
    }
    let last_day = days_in_month(year, month);
    if day < 1 || day > last_day {
        return Err(FieldError::invalid(raw, "day out of range"));
    }

    Ok(Value::Date { year, month: month as u8, day: day as u8 })
}

/// `<day> <Thai month name> <year>`, e.g. `10 สิงหาคม 2569`.
fn parse_thai_month_date(text: &str) -> Option<(u32, u32, i32)> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let [day, month, year] = tokens[..] else { return None };
    let day = day.trim_end_matches('.').parse().ok()?;
    let month = thai_month_number(month)?;
    let year = year.parse().ok()?;
    Some((day, month, year))
}

fn thai_month_number(text: &str) -> Option<u32> {
    FULL_MONTHS
        .iter()
        .position(|m| *m == text)
        .or_else(|| {
            ABBR_MONTHS.iter().position(|m| m.trim_end_matches('.') == text.trim_end_matches('.'))
        })
        .map(|i| i as u32 + 1)
}

/// `<day>/<month>/<year>`, accepting `/`, `-`, or `.` as the separator.
fn parse_numeric_date(text: &str) -> Option<(u32, u32, i32)> {
    let parts: Vec<&str> = text.split(['/', '-', '.']).collect();
    let [day, month, year] = parts[..] else { return None };
    Some((day.trim().parse().ok()?, month.trim().parse().ok()?, year.trim().parse().ok()?))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ── Amount ───────────────────────────────────────────────────────────────

/// Keeps digits, the decimal point, and a leading minus or accounting
/// parenthesis; drops thousands commas and any currency word or symbol
/// (`บาท`, `THB`, `฿`).
///
/// Exactly one number has to be present. A locator that picked up a
/// neighbouring column gives `2 50,000.00`, which concatenating every digit
/// would silently read as 250000 — a wrong number is worse than none.
fn parse_amount(raw: &str) -> Result<Value, FieldError> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let numbers: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.contains(|c: char| c.is_ascii_digit()))
        .map(|(index, _)| index)
        .collect();
    let [index] = numbers[..] else {
        let reason = if numbers.is_empty() { "no digits found" } else { "more than one number" };
        return Err(FieldError::invalid(raw, reason));
    };

    let token = tokens[index];
    let first_digit =
        token.find(|c: char| c.is_ascii_digit()).expect("token was selected for having one");
    let (prefix, number) = token.split_at(first_digit);
    // Only thousands commas, the decimal point, and a closing accounting
    // parenthesis belong among the digits. Anything else is a separator, and
    // a separator means this is not one number: `10/08/2569` and `10-08-2569`
    // are dates, which dropping the separator would report as 10,082,569.
    if number.contains(|c: char| !c.is_ascii_digit() && !matches!(c, ',' | '.' | ')')) {
        return Err(FieldError::invalid(raw, "not a decimal number"));
    }

    // The sign is its own text run whenever the font changes at the digits,
    // so it arrives as a separate token (`- 250.00`). Reading only the
    // number's own prefix drops it and turns a credit into a charge.
    let detached_sign = index.checked_sub(1).map_or("", |before| tokens[before]);
    let negative = prefix.contains(['-', '(']) || matches!(detached_sign, "-" | "(");

    let digits: String = number.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let value: f64 =
        digits.parse().map_err(|_| FieldError::invalid(raw, "not a decimal number"))?;
    Ok(Value::Amount(if negative { -value } else { value }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_trimmed_and_unvalidated() {
        assert_eq!(parse(ValueKind::Text, "  hello  ").unwrap(), Value::Text("hello".to_string()));
    }

    #[test]
    fn a_checksum_valid_tax_id_parses() {
        // First 12 digits give check digit 9 under the mod-11 scheme.
        assert_eq!(
            parse(ValueKind::ThaiTaxId, "0105558012349").unwrap(),
            Value::ThaiTaxId("0105558012349".to_string())
        );
    }

    #[test]
    fn a_tax_id_with_grouping_hyphens_still_parses() {
        assert_eq!(
            parse(ValueKind::ThaiTaxId, "0-1055-58012-34-9").unwrap(),
            Value::ThaiTaxId("0105558012349".to_string())
        );
    }

    #[test]
    fn a_checksum_mismatch_is_rejected() {
        let error = parse(ValueKind::ThaiTaxId, "0105558012345").unwrap_err();
        assert!(matches!(error, FieldError::Invalid { .. }), "{error:?}");
    }

    #[test]
    fn the_wrong_digit_count_is_rejected() {
        let error = parse(ValueKind::ThaiTaxId, "12345").unwrap_err();
        assert!(matches!(error, FieldError::Invalid { .. }), "{error:?}");
    }

    #[test]
    fn a_thai_month_name_date_converts_from_buddhist_era() {
        assert_eq!(
            parse(ValueKind::Date, "10 สิงหาคม 2569").unwrap(),
            Value::Date { year: 2026, month: 8, day: 10 }
        );
    }

    #[test]
    fn an_abbreviated_thai_month_parses_the_same_as_the_full_name() {
        assert_eq!(
            parse(ValueKind::Date, "10 ส.ค. 2569").unwrap(),
            Value::Date { year: 2026, month: 8, day: 10 }
        );
    }

    #[test]
    fn a_numeric_date_converts_from_buddhist_era() {
        assert_eq!(
            parse(ValueKind::Date, "10/08/2569").unwrap(),
            Value::Date { year: 2026, month: 8, day: 10 }
        );
    }

    #[test]
    fn a_year_already_in_the_gregorian_range_is_left_alone() {
        assert_eq!(
            parse(ValueKind::Date, "10/08/2026").unwrap(),
            Value::Date { year: 2026, month: 8, day: 10 }
        );
    }

    #[test]
    fn an_out_of_range_month_is_rejected() {
        let error = parse(ValueKind::Date, "10/13/2569").unwrap_err();
        assert!(matches!(error, FieldError::Invalid { .. }), "{error:?}");
    }

    #[test]
    fn february_29_is_accepted_only_in_a_leap_year() {
        // 2567 BE -> 2024 AD, a leap year.
        assert!(parse(ValueKind::Date, "29 กุมภาพันธ์ 2567").is_ok());
        // 2568 BE -> 2025 AD, not a leap year.
        assert!(parse(ValueKind::Date, "29 กุมภาพันธ์ 2568").is_err());
    }

    #[test]
    fn an_amount_drops_thousands_commas_and_the_currency_word() {
        assert_eq!(parse(ValueKind::Amount, "100,000.00 บาท").unwrap(), Value::Amount(100_000.0));
    }

    #[test]
    fn an_amount_with_no_digits_is_rejected() {
        let error = parse(ValueKind::Amount, "บาท").unwrap_err();
        assert!(matches!(error, FieldError::Invalid { .. }), "{error:?}");
    }

    #[test]
    fn a_negative_amount_keeps_its_sign() {
        assert_eq!(parse(ValueKind::Amount, "-250.00").unwrap(), Value::Amount(-250.0));
    }

    /// A font change at the digits puts the sign in its own text run, so the
    /// locator hands it over as a separate token.
    #[test]
    fn a_sign_in_its_own_run_still_negates() {
        assert_eq!(parse(ValueKind::Amount, "- 250.00").unwrap(), Value::Amount(-250.0));
    }

    #[test]
    fn an_accounting_parenthesis_reads_as_negative() {
        assert_eq!(parse(ValueKind::Amount, "(250.00)").unwrap(), Value::Amount(-250.0));
    }

    /// A locator that reached into the next column returns two numbers.
    /// Concatenating their digits would read `2 50,000.00` as 250000.
    #[test]
    fn two_numbers_in_one_value_are_rejected() {
        let error = parse(ValueKind::Amount, "2 50,000.00").unwrap_err();
        assert!(matches!(error, FieldError::Invalid { .. }), "{error:?}");
    }

    /// Every separator a date is written with, not just the hyphen: a slash
    /// date read as an amount reports 10,082,569 บาท, which parses, passes
    /// every downstream check, and is off by seven orders of magnitude.
    #[test]
    fn a_date_read_as_an_amount_is_rejected() {
        for raw in ["10-08-2569", "10/08/2569", "10.08.2569", "2569-08-10"] {
            let error = parse(ValueKind::Amount, raw).unwrap_err();
            assert!(matches!(error, FieldError::Invalid { .. }), "{raw}: {error:?}");
        }
    }

    /// A two-digit year names no era: `69` is 2026 read as พ.ศ. and `26` is
    /// 2026 read as ค.ศ., and the two readings are 43 years apart. Guessing
    /// one silently dates a document to the wrong decade.
    #[test]
    fn a_two_digit_year_is_rejected() {
        for raw in ["10/08/69", "10/08/26"] {
            let error = parse(ValueKind::Date, raw).unwrap_err();
            assert!(matches!(error, FieldError::Invalid { .. }), "{raw}: {error:?}");
        }
    }
}
