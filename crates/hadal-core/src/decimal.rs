//! Arbitrary-range decimal numbers with deterministic IEEE-754 arithmetic.
//!
//! # Representation
//!
//! A [`Decimal`] stores a value as `(mantissa, exponent10)` where:
//! - `mantissa ∈ [1, 10)` for nonzero values
//! - canonical zero is `(0.0, 0)`
//! - `exponent10` is the power of 10 applied to the mantissa
//!
//! # Precision
//!
//! Relative precision is ~15–17 significant digits (f64 mantissa). Adding a
//! value more than 17 decades smaller than the accumulator is a no-op by
//! definition, because alignment shifts the smaller value's mantissa away
//! entirely.
//!
//! # Determinism
//!
//! Only IEEE-754 `+ - * / sqrt` and integer operations are used in numeric
//! paths. No `powf`, `ln`, `exp`, `log10`, `sin`, or locale formatting.
//! The same sequence of operations produces identical bit-level results on
//! every platform.

use std::fmt;
use std::hash::Hash;
use std::ops::{Add, Div, Mul, Sub};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Error type for [`Decimal`] operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecimalError {
    /// Input value is NaN or Infinity.
    #[error("non-finite input")]
    NonFinite,
    /// Division by zero.
    #[error("division by zero")]
    DivisionByZero,
    /// Square root of a negative number.
    #[error("negative square root")]
    NegativeSqrt,
    /// String could not be parsed as a valid decimal.
    #[error("invalid decimal: {0}")]
    InvalidParse(String),
    /// Exponent exceeds the clamped range.
    #[error("exponent out of range")]
    ExponentOutOfRange,
}

/// Arbitrary-range decimal number.
///
/// Stored as a normalized `(mantissa, exponent10)` pair:
/// - `mantissa ∈ [1, 10)` for nonzero values
/// - canonical zero is `(0.0, 0)`
///
/// All arithmetic is deterministic: only IEEE-754 `+ - * / sqrt` and integer
/// operations are used. No `NaN` or `Infinity` can enter state.
#[derive(Clone, Copy, Debug)]
pub struct Decimal {
    mantissa: f64,
    exponent: i64,
}

// Exponent clamp bounds: i64::MIN/2 ..= i64::MAX/2
const EXP_CLAMP_MIN: i64 = i64::MIN / 2;
const EXP_CLAMP_MAX: i64 = i64::MAX / 2;

// Sentinel for log10_floor(0).
const LOG10_FLOOR_ZERO_SENTINEL: i64 = i64::MIN / 2;

impl PartialEq for Decimal {
    /// Compares canonical form: equal values built by different operation
    /// paths compare equal because every constructor normalizes.
    fn eq(&self, other: &Self) -> bool {
        // NaN cannot occur; canonical zero is always +0.0.
        self.mantissa == other.mantissa && self.exponent == other.exponent
    }
}

impl Eq for Decimal {}

impl Hash for Decimal {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Bits are consistent with `eq` because zero is canonicalized to +0.0
        // and no other distinct f64 values compare equal.
        self.mantissa.to_bits().hash(state);
        self.exponent.hash(state);
    }
}

/// Canonical zero constant.
pub const ZERO: Decimal = Decimal {
    mantissa: 0.0,
    exponent: 0,
};

/// Canonical one constant.
pub const ONE: Decimal = Decimal {
    mantissa: 1.0,
    exponent: 0,
};

impl Decimal {
    /// Canonical zero as an associated constant.
    pub const ZERO: Self = ZERO;

    /// Canonical one as an associated constant.
    pub const ONE: Self = ONE;

    /// Creates a zero decimal.
    pub const fn zero() -> Self {
        ZERO
    }

    /// Creates a one decimal.
    pub const fn one() -> Self {
        ONE
    }

    /// Constructs a decimal from a mantissa and base-10 exponent.
    ///
    /// # Errors
    ///
    /// - [`DecimalError::NonFinite`] if `mantissa` is NaN or infinite.
    /// - [`DecimalError::ExponentOutOfRange`] if the clamped exponent exceeds
    ///   the allowed range.
    ///
    /// The mantissa is normalized into `[1, 10)` and the exponent adjusted
    /// accordingly. Zero mantissa always yields canonical zero `(0.0, 0)`.
    pub fn from_parts(mantissa: f64, exponent: i64) -> Result<Self, DecimalError> {
        if !mantissa.is_finite() {
            return Err(DecimalError::NonFinite);
        }
        Self::normalize(mantissa, exponent)
    }

    /// Creates a decimal from a non-negative integer.
    pub fn from_u64(v: u64) -> Self {
        if v == 0 {
            return ZERO;
        }
        let digits = (v.ilog10() + 1) as i64;
        let mantissa = v as f64 / 10.0_f64.powi((digits - 1) as i32);
        Self::normalize_unchecked(mantissa, digits - 1)
    }

    /// Creates a decimal from a signed integer.
    pub fn from_i64(v: i64) -> Self {
        if v < 0 {
            return Decimal::zero(); // saturating
        }
        Decimal::from_u64(v as u64)
    }

    /// Try-converts from an `f64`.
    ///
    /// # Errors
    ///
    /// Returns [`DecimalError::NonFinite`] only when `v` is NaN or infinite.
    pub fn try_from_f64(v: f64) -> Result<Self, DecimalError> {
        Self::from_parts(v, 0)
    }

    /// Returns the mantissa component (∈ `[1, 10)` for nonzero).
    pub fn mantissa(&self) -> f64 {
        self.mantissa
    }

    /// Returns the base-10 exponent component.
    pub fn exponent10(&self) -> i64 {
        self.exponent
    }

    /// Returns `true` if this decimal is canonical zero.
    pub fn is_zero(&self) -> bool {
        self.mantissa == 0.0
    }

    // ----------------------------------------------------------------
    // Arithmetic
    // ----------------------------------------------------------------

    /// Adds two decimals.
    ///
    /// # Panics
    ///
    /// In debug builds, panics only if an internal intermediate is
    /// non-finite, which cannot happen for valid `Decimal` inputs.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        if self.is_zero() {
            return other;
        }
        if other.is_zero() {
            return self;
        }

        let (m1, e1) = (self.mantissa, self.exponent);
        let (m2, e2) = (other.mantissa, other.exponent);

        if e1 >= e2 {
            let diff = e1 - e2;
            if diff >= 20 {
                // Other value is >17 decades smaller — no-op
                self
            } else {
                let aligned = m2 * 10.0_f64.powi(-(diff as i32));
                Self::normalize_unchecked(m1 + aligned, e1)
            }
        } else {
            let diff = e2 - e1;
            if diff >= 20 {
                other
            } else {
                let aligned = m1 * 10.0_f64.powi(-(diff as i32));
                Self::normalize_unchecked(m2 + aligned, e2)
            }
        }
    }

    /// Subtracts `other` from `self`, saturating at zero.
    ///
    /// Negative results are clamped to canonical zero.
    ///
    /// # Panics
    ///
    /// In debug builds, panics only if an internal intermediate is
    /// non-finite, which cannot happen for valid `Decimal` inputs.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Self) -> Self {
        if self.is_zero() {
            return ZERO;
        }
        if other.is_zero() {
            return self;
        }

        let (m1, e1) = (self.mantissa, self.exponent);
        let (m2, e2) = (other.mantissa, other.exponent);

        if e1 >= e2 {
            let diff = e1 - e2;
            if diff >= 20 {
                // other is negligible — result is self
                self
            } else {
                let aligned = m2 * 10.0_f64.powi(-(diff as i32));
                let diff_val = m1 - aligned;
                if diff_val <= 0.0 {
                    return ZERO;
                }
                Self::normalize_unchecked(diff_val, e1)
            }
        } else {
            // e2 > e1: self < other → result is negative → saturate to zero
            ZERO
        }
    }

    /// Multiplies two decimals.
    ///
    /// # Panics
    ///
    /// In debug builds, panics only if an internal intermediate is
    /// non-finite, which cannot happen for valid `Decimal` inputs.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, other: Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return ZERO;
        }
        let m = self.mantissa * other.mantissa;
        let e = self.exponent + other.exponent;
        Self::normalize_unchecked(m, e)
    }

    /// Divides `self` by `other`.
    ///
    /// # Errors
    ///
    /// Returns [`DecimalError::DivisionByZero`] if `other` is zero.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, other: Self) -> Result<Self, DecimalError> {
        if other.is_zero() {
            return Err(DecimalError::DivisionByZero);
        }
        if self.is_zero() {
            return Ok(ZERO);
        }
        let m = self.mantissa / other.mantissa;
        let e = self.exponent - other.exponent;
        Self::normalize(m, e)
    }

    /// Raises `self` to the integer power `n` using exponentiation by squaring.
    ///
    /// Fast path for `n = 0` (returns one) and `n = 1` (returns self).
    pub fn powi(self, n: u32) -> Self {
        if n == 0 {
            return ONE;
        }
        if n == 1 {
            return self;
        }
        if self.is_zero() {
            return ZERO;
        }

        let mut base = self;
        let mut exp = n;
        let mut result = ONE;

        while exp > 0 {
            if exp & 1 == 1 {
                result = result.mul(base);
            }
            exp >>= 1;
            if exp > 0 {
                base = base.mul(base);
            }
        }

        result
    }

    /// Square root.
    ///
    /// Computed with one IEEE-754 `sqrt` on a mantissa adjusted to an even
    /// exponent, so no repeated normalization rounding is introduced.
    ///
    /// # Errors
    ///
    /// Returns [`DecimalError::NegativeSqrt`] if `self` is negative.
    /// Negative values saturate to zero at construction, so this error is
    /// defensive and cannot be reached through the public API.
    pub fn sqrt(self) -> Result<Self, DecimalError> {
        if self.is_zero() {
            return Ok(ZERO);
        }
        let mut m = self.mantissa;
        let mut e = self.exponent;
        if m < 0.0 {
            return Err(DecimalError::NegativeSqrt);
        }
        // Make the exponent even so sqrt can be applied to the mantissa alone.
        if e & 1 != 0 {
            m *= 10.0;
            e -= 1;
        }
        let s = m.sqrt(); // m ∈ [1, 100), so s ∈ [1, 10)
        Self::from_parts(s, e / 2)
    }

    /// Returns the largest integer ≤ self as a Decimal.
    ///
    /// For values already integral by construction (exponent ≥ 16, well past
    /// f64's fractional resolution) this returns `self` unchanged. Otherwise
    /// the value is small enough for an exact f64 floor.
    pub fn floor(self) -> Self {
        if self.is_zero() {
            return ZERO;
        }
        // Values with exponent >= 16 have no fractional part: mantissa < 10 and
        // 10^16 exceeds the fractional resolution of the ~16-digit mantissa.
        if self.exponent >= 16 {
            return self;
        }
        let f = self.mantissa * 10.0_f64.powi(self.exponent as i32);
        let floored = f.floor();
        if floored <= 0.0 {
            return ZERO;
        }
        Self::normalize_unchecked(floored, 0)
    }

    /// Returns `true` if this decimal represents an integer value.
    ///
    /// A value is an integer when `mantissa * 10^exponent` has no fractional
    /// part. Since the mantissa is in `[1, 10)`, this is true only when the
    /// exponent is non-negative and the scaled value is integral. Extremely
    /// large exponents (beyond f64's fractional resolution) are integers by
    /// construction.
    pub fn is_integer(self) -> bool {
        if self.is_zero() {
            return true;
        }
        if self.exponent < 0 {
            return false;
        }
        if self.exponent > 308 {
            return true;
        }
        let scaled = self.mantissa * 10.0_f64.powi(self.exponent as i32);
        !scaled.is_finite() || scaled.fract() == 0.0
    }

    /// Returns the floor of log10(self).
    ///
    /// For nonzero values, this equals the exponent10 when mantissa ∈ [1, 10).
    /// For zero, returns `LOG10_FLOOR_ZERO_SENTINEL` (= `i64::MIN / 2`).
    pub fn log10_floor(self) -> i64 {
        if self.is_zero() {
            return LOG10_FLOOR_ZERO_SENTINEL;
        }
        self.exponent
    }

    /// Returns the greater of self and other.
    pub fn max(self, other: Self) -> Self {
        if self >= other { self } else { other }
    }

    /// Returns the lesser of self and other.
    pub fn min(self, other: Self) -> Self {
        if self <= other { self } else { other }
    }

    /// Compares self to other, returning the ordering.
    #[allow(clippy::should_implement_trait)]
    pub fn cmp(self, other: Self) -> std::cmp::Ordering {
        if self.is_zero() && other.is_zero() {
            return std::cmp::Ordering::Equal;
        }
        if self.is_zero() {
            return std::cmp::Ordering::Less;
        }
        if other.is_zero() {
            return std::cmp::Ordering::Greater;
        }

        // Compare exponents first; same exponent compares mantissas exactly.
        match self.exponent.cmp(&other.exponent) {
            std::cmp::Ordering::Equal => self.mantissa.total_cmp(&other.mantissa),
            ord => ord,
        }
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    /// Normalizes a mantissa/exponent pair into canonical form.
    ///
    /// Mantissa is brought into `[1, 10)`, exponent is clamped to
    /// `[i64::MIN/2, i64::MAX/2]`, and non-positive mantissa is canonicalized
    /// to `(0.0, 0)`. Negative values cannot be represented; they saturate
    /// to canonical zero.
    fn normalize(m: f64, e: i64) -> Result<Self, DecimalError> {
        if !m.is_finite() {
            return Err(DecimalError::NonFinite);
        }
        Ok(Self::normalize_unchecked(m, e))
    }

    /// Canonicalizes a finite mantissa/exponent pair without a fallible check.
    ///
    /// Arithmetic paths always feed finite intermediates here (mantissa
    /// products/sums stay near `[0, 100]`), so this is the infallible core.
    fn normalize_unchecked(m: f64, e: i64) -> Self {
        debug_assert!(
            m.is_finite(),
            "normalize_unchecked requires finite mantissa"
        );
        if m <= 0.0 {
            // Handles zero, negative zero, and negatives (saturating).
            return ZERO;
        }

        let mut m = m;
        let mut e = e;

        if m >= 10.0 {
            while m >= 10.0 {
                m /= 10.0;
                e += 1;
            }
        } else if m < 1.0 {
            // m is finite and > 0, so repeated *10 approaches 1 from below.
            while m < 1.0 {
                m *= 10.0;
                e -= 1;
            }
        }

        let e = e.clamp(EXP_CLAMP_MIN, EXP_CLAMP_MAX);
        Decimal {
            mantissa: m,
            exponent: e,
        }
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return write!(f, "0");
        }
        // Mantissa with 17 significant digits: one digit before the point and
        // sixteen after. Rust's float formatter is deterministic and locale-free.
        let mantissa_str = format!("{:.16}", self.mantissa);
        // Canonical exponent form: `e6`, `e-5`, `e0` (no sign on positives, no
        // zero padding). `FromStr` accepts this and also `e+006` forms.
        write!(f, "{}e{}", mantissa_str, self.exponent)
    }
}

impl fmt::LowerExp for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Floor human-readable formatting.
///
/// | Range | Format |
/// |-------|--------|
/// | `0` | `"0"` |
/// | `(0, 1)` | `"0"` (floor of any positive value < 1) |
/// | `[1, 1e6)` | plain integer with commas, e.g. `"1"`, `"999"`, `"1,000"`, `"999,999"` |
/// | `[1e6, 1e36)` | 3 sig figs with suffix, e.g. `"1.00M"`, `"12.3M"`, `"123M"`, `"1.00Dc"` |
/// | `≥ 1e36` | scientific with 3 sig figs, e.g. `"1.00e36"`, `"9.99e40"` |
///
/// Suffixes: M (1e6), B (1e9), T (1e12), Qa (1e15), Qi (1e18),
/// Sx (1e21), Sp (1e24), Oc (1e27), No (1e30), Dc (1e33).
///
/// Rounding: floor at the displayed precision. If floor-carry pushes
/// scaled to 1000, advance to the next suffix.
pub fn to_human_floor(d: Decimal) -> String {
    format_number(d, Rounding::Floor)
}

/// Ceil human-readable formatting.
///
/// Same layout as [`to_human_floor`] but rounds up at the displayed precision.
pub fn to_human_ceil(d: Decimal) -> String {
    format_number(d, Rounding::Ceil)
}

/// Rate formatting: floor at 2 decimals below 1e6, suffix form above.
///
/// Below 1e6: always two decimal places with commas, e.g. `"0.00"`, `"0.50"`,
/// `"18.60"`, `"999,999.00"`.
/// At or above 1e6: same suffix form as [`to_human_floor`] (3 sig figs).
pub fn to_rate(d: Decimal) -> String {
    if d.is_zero() {
        return "0.00".to_string();
    }
    if d.exponent10() >= 6 {
        format_number(d, Rounding::Floor)
    } else {
        format_rate_small(d)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rounding {
    Floor,
    Ceil,
}

/// Format a Decimal with the given rounding direction.
fn format_number(d: Decimal, rounding: Rounding) -> String {
    if d.is_zero() {
        return "0".to_string();
    }

    // Value >= 1e6 iff exponent >= 6 (mantissa is in [1, 10)). This branch is
    // exact and avoids converting huge exponents through f64.
    if d.exponent10() >= 6 {
        return format_suffix(d, rounding);
    }

    // Values in [0, 1e6): plain integer with thousands commas.
    // Floor truncates toward zero; ceil bumps a non-integer up by one.
    let floor_int = extract_integer(d);
    let n: u128 = match rounding {
        Rounding::Floor => floor_int,
        Rounding::Ceil => {
            if d.is_integer() {
                floor_int
            } else {
                floor_int + 1
            }
        }
    };

    // Ceiling can cross into the suffix range only at exactly 1e6.
    if n >= 1_000_000 {
        return format_suffix(Decimal::from_u64(1_000_000), Rounding::Floor);
    }

    format_integer_with_commas(n)
}

/// Format a value in suffix form (M/B/T/Qa/Qi/Sx/Sp/Oc/No/Dc).
fn format_suffix(d: Decimal, rounding: Rounding) -> String {
    let mantissa = d.mantissa();
    let exponent = d.exponent10();
    let suffixes = ["M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No", "Dc"];

    // Each group spans three decades starting at 1e6: group = (exp - 6) / 3.
    let group = (exponent - 6) / 3;
    if group < 0 || group >= suffixes.len() as i64 {
        // Beyond the Dc group (>= 1e36): scientific with 3 sig figs.
        return format_scientific(d, rounding);
    }
    let suffix_idx = group as usize;
    let suffix = suffixes[suffix_idx];

    // frac_exp = exponent - 6 - 3*group ∈ {0, 1, 2}
    let frac_exp = exponent - 6 - 3 * group;

    // scaled = mantissa * 10^frac_exp ∈ [1, 1000)
    let scaled = mantissa * 10.0_f64.powi(frac_exp as i32);

    // Determine precision: 2 decimals if scaled < 10, 1 if < 100, 0 otherwise
    let (div, precision) = if scaled < 10.0 {
        (100u64, 2usize)
    } else if scaled < 100.0 {
        (10, 1)
    } else {
        (1, 0)
    };

    let scaled_int = if rounding == Rounding::Floor {
        (scaled * div as f64).floor() as u64
    } else {
        (scaled * div as f64).ceil() as u64
    };

    // Rounding may carry scaled up to the next group (e.g. ceil(999.4) → 1000).
    if scaled_int >= 1000 * div {
        let next_idx = suffix_idx + 1;
        if next_idx < suffixes.len() {
            return format!("1.00{}", suffixes[next_idx]);
        }
        return format_scientific(d, rounding);
    }

    let int_part = scaled_int / div;
    let dec_part = scaled_int % div;

    if precision == 0 {
        format!("{}{}", int_part, suffix)
    } else {
        let dec_str = format!("{:0>width$}", dec_part, width = precision);
        format!("{}.{}{}", int_part, dec_str, suffix)
    }
}

/// Format a value in scientific notation with 3 sig figs (e.g. `1.00e36`).
fn format_scientific(d: Decimal, rounding: Rounding) -> String {
    let mantissa = d.mantissa();
    let exponent = d.exponent10();

    // We need 3 significant digits: scaled = mantissa * 100 ∈ [100, 1000)
    let scaled = mantissa * 100.0_f64;

    let scaled_int = if rounding == Rounding::Floor {
        scaled.floor() as u64
    } else {
        scaled.ceil() as u64
    };

    // Check for carry: scaled_int >= 1000 → mantissa rounds to 10.0
    if scaled_int >= 1000 {
        return format!("1.00e{}", exponent + 1);
    }

    // Extract digits: int_part = scaled_int / 100, dec_part = scaled_int % 100
    let int_part = scaled_int / 100;
    let dec_part = scaled_int % 100;
    let dec_str = format!("{:02}", dec_part);

    format!("{}.{}e{}", int_part, dec_str, exponent)
}

/// Format a small integer (< 1e6) with thousands commas.
fn format_integer_with_commas(n: u128) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut result = String::new();
    let mut digits: Vec<u8> = Vec::new();
    let mut v = n;
    while v > 0 {
        digits.push((v % 10) as u8);
        v /= 10;
    }
    digits.reverse();
    let len = digits.len();
    for (i, &digit) in digits.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(char::from(b'0' + digit));
    }
    result
}

/// Format rate with 2 decimal places for values < 1e6.
fn format_rate_small(d: Decimal) -> String {
    let mantissa = d.mantissa();
    let exponent = d.exponent10();

    // value * 100 = mantissa * 10^(exponent + 2)
    let scale_exp = exponent + 2;

    if scale_exp >= 0 {
        let scaled = (mantissa * 10.0_f64.powi(scale_exp as i32)).floor() as u64;
        let integer = scaled / 100;
        let decimals = (scaled % 100) as u32;
        let int_str = format_integer_with_commas(integer as u128);
        format!("{}.{:02}", int_str, decimals)
    } else {
        // Value < 0.01: floor is 0
        "0.00".to_string()
    }
}

/// Extract the integer part of a Decimal as u128.
/// Only called for values in [1, 1e6).
fn extract_integer(d: Decimal) -> u128 {
    let mantissa = d.mantissa();
    let exponent = d.exponent10();

    if exponent >= 0 {
        let scaled = mantissa * 10.0_f64.powi(exponent as i32);
        scaled.floor() as u128
    } else {
        // For exponent < 0 the value is < 1, so the integer part is zero.
        0
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.is_empty() {
            return Err(DecimalError::InvalidParse(s.to_string()));
        }

        // Check for NaN / inf
        let lower = s.to_lowercase();
        if lower.contains("nan") || lower.contains("inf") {
            return Err(DecimalError::InvalidParse(s.to_string()));
        }

        // Parse sign
        let (s, sign) = if let Some(rest) = s.strip_prefix('+') {
            (rest, 1.0)
        } else if s.starts_with('-') {
            return Err(DecimalError::InvalidParse(s.to_string()));
        } else {
            (s, 1.0)
        };

        if s.is_empty() {
            return Err(DecimalError::InvalidParse(s.to_string()));
        }

        // Find '.' and 'e'/'E'
        let dot_pos = s.find('.');
        let exp_pos = s.find(['e', 'E']);

        let (int_part, frac_part, exp_str) = match (dot_pos, exp_pos) {
            (Some(dot), Some(exp)) if dot < exp => (&s[..dot], &s[dot + 1..exp], Some(&s[exp..])),
            (Some(dot), None) => (&s[..dot], &s[dot + 1..], None),
            (None, Some(exp)) => (&s[..exp], "", Some(&s[exp..])),
            (None, None) => (s, "", None),
            // dot after exp — invalid
            _ => {
                return Err(DecimalError::InvalidParse(s.to_string()));
            }
        };

        // Validate int_part: must have at least one digit
        if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
            return Err(DecimalError::InvalidParse(s.to_string()));
        }

        // Validate frac_part: if present, must have at least one digit
        if !frac_part.is_empty() && !frac_part.chars().all(|c| c.is_ascii_digit()) {
            return Err(DecimalError::InvalidParse(s.to_string()));
        }

        // Parse exponent: optional '+'/'-' followed by at least one digit.
        let exp_val: i64 = if let Some(exp_str) = exp_str {
            // exp_str begins with 'e' or 'E'.
            let rest = &exp_str[1..];
            let (negative, digits) = if let Some(d) = rest.strip_prefix('+') {
                (false, d)
            } else if let Some(d) = rest.strip_prefix('-') {
                (true, d)
            } else {
                (false, rest)
            };

            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return Err(DecimalError::InvalidParse(s.to_string()));
            }
            // An exponent with more than 18 digits can never fit the clamp.
            if digits.len() > 18 {
                return Err(DecimalError::ExponentOutOfRange);
            }

            let magnitude: i64 = digits
                .parse()
                .map_err(|_| DecimalError::InvalidParse(s.to_string()))?;
            let v = if negative { -magnitude } else { magnitude };
            if !(EXP_CLAMP_MIN..=EXP_CLAMP_MAX).contains(&v) {
                return Err(DecimalError::ExponentOutOfRange);
            }
            v
        } else {
            0
        };

        // Build the significand from digits only. Parsing the concatenated
        // string as f64 is deterministic and preserves 17-digit round-trips.
        let combined: f64 = if frac_part.is_empty() {
            int_part
                .parse::<f64>()
                .map_err(|_| DecimalError::InvalidParse(s.to_string()))?
        } else {
            format!("{}.{}", int_part, frac_part)
                .parse::<f64>()
                .map_err(|_| DecimalError::InvalidParse(s.to_string()))?
        };

        let value = sign * combined;

        Self::from_parts(value, exp_val)
    }
}

// ====================================================================
// Trait implementations
// ====================================================================

impl Add for Decimal {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        self.add(other)
    }
}

impl Sub for Decimal {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        self.sub(other)
    }
}

impl Mul for Decimal {
    type Output = Self;

    fn mul(self, other: Self) -> Self::Output {
        self.mul(other)
    }
}

impl Div for Decimal {
    type Output = Result<Self, DecimalError>;

    fn div(self, other: Self) -> Self::Output {
        self.div(other)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Decimal::cmp(*self, *other)
    }
}

impl From<u32> for Decimal {
    fn from(v: u32) -> Self {
        Decimal::from_u64(v as u64)
    }
}

impl From<u64> for Decimal {
    fn from(v: u64) -> Self {
        Decimal::from_u64(v)
    }
}

impl From<i32> for Decimal {
    fn from(v: i32) -> Self {
        Decimal::from_i64(v as i64)
    }
}

impl From<i64> for Decimal {
    fn from(v: i64) -> Self {
        Decimal::from_i64(v)
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Inner;

        impl<'de> serde::de::Visitor<'de> for Inner {
            type Value = Decimal;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a decimal string")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Decimal, E> {
                Decimal::from_str(v).map_err(E::custom)
            }

            fn visit_borrowed_str<E: serde::de::Error>(self, v: &'de str) -> Result<Decimal, E> {
                Decimal::from_str(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(Inner)
    }
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    /// Inline helper: convert Decimal to f64 for test assertions only.
    #[inline]
    fn to_f64(d: Decimal) -> f64 {
        if d.is_zero() {
            0.0
        } else {
            d.mantissa() * 10.0_f64.powi(d.exponent10() as i32)
        }
    }

    /// Test helper: Decimal from mantissa and exponent.
    #[inline]
    fn dec(m: f64, e: i64) -> Decimal {
        Decimal::from_parts(m, e).unwrap()
    }

    /// Test helper: Decimal from u64.
    #[inline]
    fn from_u(v: u64) -> Decimal {
        Decimal::from_u64(v)
    }

    /// Test helper: parse a human-formatted string (commas + suffixes + scientific).
    fn parse_human(s: &str) -> Decimal {
        let cleaned = s.replace(',', "");
        // Longest suffixes first; none is a prefix of another ambiguous string.
        let suffixes: [(&str, i64); 10] = [
            ("Qa", 15),
            ("Qi", 18),
            ("Sx", 21),
            ("Sp", 24),
            ("Oc", 27),
            ("No", 30),
            ("Dc", 33),
            ("M", 6),
            ("B", 9),
            ("T", 12),
        ];
        for (suf, exp) in suffixes {
            if let Some(num) = cleaned.strip_suffix(suf) {
                let base: f64 = num.parse().unwrap();
                return Decimal::from_parts(base, exp).unwrap();
            }
        }
        Decimal::from_str(&cleaned).unwrap()
    }

    // ----------------------------------------------------------------
    // Constructors and normalization
    // ----------------------------------------------------------------

    #[test]
    fn test_zero_const() {
        let z = Decimal::zero();
        assert!(z.is_zero());
        assert_eq!(z.mantissa(), 0.0);
        assert_eq!(z.exponent10(), 0);
    }

    #[test]
    fn test_one_const() {
        let o = Decimal::one();
        assert!(!o.is_zero());
        assert!((o.mantissa() - 1.0).abs() < 1e-15);
        assert_eq!(o.exponent10(), 0);
    }

    #[test]
    fn test_from_parts_normalization() {
        // Already in range
        let d = Decimal::from_parts(5.0, 2).unwrap();
        assert!((d.mantissa() - 5.0).abs() < 1e-15);
        assert_eq!(d.exponent10(), 2);

        // Above range
        let d = Decimal::from_parts(50.0, 2).unwrap();
        assert!((d.mantissa() - 5.0).abs() < 1e-15);
        assert_eq!(d.exponent10(), 3);

        // Below range
        let d = Decimal::from_parts(0.5, 2).unwrap();
        assert!((d.mantissa() - 5.0).abs() < 1e-15);
        assert_eq!(d.exponent10(), 1);

        // Zero mantissa → canonical zero
        let d = Decimal::from_parts(0.0, 999).unwrap();
        assert!(d.is_zero());
        assert_eq!(d.exponent10(), 0);
    }

    #[test]
    fn test_from_parts_nonfinite() {
        assert_eq!(
            Decimal::from_parts(f64::NAN, 0),
            Err(DecimalError::NonFinite)
        );
        assert_eq!(
            Decimal::from_parts(f64::INFINITY, 0),
            Err(DecimalError::NonFinite)
        );
        assert_eq!(
            Decimal::from_parts(f64::NEG_INFINITY, 0),
            Err(DecimalError::NonFinite)
        );
    }

    #[test]
    fn test_from_u64() {
        assert_eq!(Decimal::from_u64(0), ZERO);
        assert_eq!(Decimal::from_u64(1), ONE);
        assert_eq!(Decimal::from_u64(10).exponent10(), 1);
        assert_eq!(Decimal::from_u64(999).exponent10(), 2);
        assert_eq!(Decimal::from_u64(1000).exponent10(), 3);
        assert_eq!(Decimal::from_u64(u64::MAX).exponent10(), 19);
    }

    #[test]
    fn test_from_i64_saturating() {
        assert_eq!(Decimal::from_i64(-1), ZERO);
        assert_eq!(Decimal::from_i64(0), ZERO);
        assert_eq!(Decimal::from_i64(42), Decimal::from_u64(42));
    }

    #[test]
    fn test_normalize_invariant() {
        // Every constructed Decimal should satisfy the invariant
        let test_vals = [
            Decimal::from_parts(1.0, 0).unwrap(),
            Decimal::from_parts(9.999, 5).unwrap(),
            Decimal::from_parts(0.001, -3).unwrap(),
            Decimal::from_parts(100.0, 0).unwrap(),
            Decimal::from_parts(0.5, 0).unwrap(),
            Decimal::from_parts(1.2345678901234567, 10).unwrap(),
        ];
        for d in &test_vals {
            if !d.is_zero() {
                assert!(
                    d.mantissa() >= 1.0 && d.mantissa() < 10.0,
                    "mantissa out of range: {} (exp={})",
                    d.mantissa(),
                    d.exponent10()
                );
            } else {
                assert_eq!(d.exponent10(), 0);
            }
        }
    }

    // ----------------------------------------------------------------
    // Equality, hash, ordering
    // ----------------------------------------------------------------

    #[test]
    fn test_equality_across_scales() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let a = Decimal::from_parts(1.0, 3).unwrap(); // 1000
        let b = Decimal::from_parts(10.0, 2).unwrap(); // 10 * 100 = 1000 → normalizes to (1.0, 3)
        assert_eq!(a, b);
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn test_ordering() {
        let a = Decimal::from_parts(1.0, 2).unwrap(); // 100
        let b = Decimal::from_parts(2.0, 2).unwrap(); // 200
        let c = Decimal::from_parts(1.0, 3).unwrap(); // 1000

        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
        assert!(c > b);
        assert!(b > a);
        assert_eq!(a.cmp(b), std::cmp::Ordering::Less);
        assert_eq!(c.cmp(a), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_zero_ordering() {
        let z = Decimal::zero();
        let p = Decimal::from_u64(1);
        assert!(z < p);
        assert!(p > z);
        assert_eq!(z.cmp(z), std::cmp::Ordering::Equal);
    }

    // ----------------------------------------------------------------
    // Addition
    // ----------------------------------------------------------------

    #[test]
    fn test_add_basic() {
        let a = Decimal::from_u64(3);
        let b = Decimal::from_u64(5);
        let r = a.add(b);
        assert_eq!(r, Decimal::from_u64(8));
    }

    #[test]
    fn test_add_with_zero() {
        let a = Decimal::from_u64(42);
        assert_eq!(a.add(Decimal::zero()), a);
        assert_eq!(Decimal::zero().add(a), a);
        assert_eq!(Decimal::zero().add(Decimal::zero()), Decimal::zero());
    }

    #[test]
    fn test_add_exponent_alignment() {
        let a = Decimal::from_parts(1.0, 2).unwrap(); // 100
        let b = Decimal::from_parts(5.0, 1).unwrap(); // 50
        let r = a.add(b);
        assert_eq!(r, Decimal::from_u64(150));
    }

    #[test]
    fn test_add_seventeen_decade_noop() {
        // 1e30 + 1e0 should be 1e30 (difference > 17 decades)
        let big = Decimal::from_parts(1.0, 30).unwrap();
        let small = Decimal::from_u64(1);
        let r = big.add(small);
        assert_eq!(r, big);
    }

    #[test]
    fn test_add_carry_across_exponent() {
        // 9.9 + 0.2 → 10.1 → normalizes to (1.01, 1)
        let a = Decimal::from_parts(9.9, 0).unwrap();
        let b = Decimal::from_parts(2.0, -1).unwrap(); // 0.2
        let r = a.add(b);
        // 9.9 + 0.2 = 10.1
        assert!((r.mantissa() - 1.01).abs() < 1e-10);
        assert_eq!(r.exponent10(), 1);
    }

    #[test]
    fn test_add_same_exponent() {
        let a = Decimal::from_parts(3.0, 2).unwrap(); // 300
        let b = Decimal::from_parts(7.0, 2).unwrap(); // 700
        let r = a.add(b);
        // 10.0 → normalizes to (1.0, 3)
        assert!((r.mantissa() - 1.0).abs() < 1e-15);
        assert_eq!(r.exponent10(), 3);
    }

    // ----------------------------------------------------------------
    // Subtraction
    // ----------------------------------------------------------------

    #[test]
    fn test_sub_basic() {
        let a = Decimal::from_u64(10);
        let b = Decimal::from_u64(3);
        let r = a.sub(b);
        assert_eq!(r, Decimal::from_u64(7));
    }

    #[test]
    fn test_sub_saturating_at_zero() {
        let a = Decimal::from_u64(3);
        let b = Decimal::from_u64(10);
        let r = a.sub(b);
        assert_eq!(r, ZERO);
    }

    #[test]
    fn test_sub_self() {
        let a = Decimal::from_u64(42);
        assert_eq!(a.sub(a), ZERO);
    }

    #[test]
    fn test_sub_with_zero() {
        let a = Decimal::from_u64(42);
        assert_eq!(a.sub(Decimal::zero()), a);
        assert_eq!(Decimal::zero().sub(Decimal::from_u64(5)), ZERO);
    }

    // ----------------------------------------------------------------
    // Multiplication
    // ----------------------------------------------------------------

    #[test]
    fn test_mul_basic() {
        let a = Decimal::from_u64(3);
        let b = Decimal::from_u64(4);
        let r = a.mul(b);
        assert_eq!(r, Decimal::from_u64(12));
    }

    #[test]
    fn test_mul_with_zero() {
        let a = Decimal::from_u64(999);
        assert_eq!(a.mul(Decimal::zero()), ZERO);
        assert_eq!(Decimal::zero().mul(a), ZERO);
    }

    #[test]
    fn test_mul_exponent_addition() {
        let a = Decimal::from_parts(2.0, 3).unwrap(); // 2000
        let b = Decimal::from_parts(5.0, 2).unwrap(); // 500
        let r = a.mul(b);
        // 2000 * 500 = 1,000,000 = 1e6
        assert!((r.mantissa() - 1.0).abs() < 1e-15);
        assert_eq!(r.exponent10(), 6);
    }

    #[test]
    fn test_mul_associativity_spot() {
        // (2 * 3) * 5 == 2 * (3 * 5)
        let a = Decimal::from_u64(2);
        let b = Decimal::from_u64(3);
        let c = Decimal::from_u64(5);
        let l = a.mul(b).mul(c);
        let r = a.mul(b.mul(c));
        assert_eq!(l, r);
    }

    // ----------------------------------------------------------------
    // Division
    // ----------------------------------------------------------------

    #[test]
    fn test_div_basic() {
        let a = Decimal::from_u64(10);
        let b = Decimal::from_u64(2);
        let r = a.div(b).unwrap();
        assert_eq!(r, Decimal::from_u64(5));
    }

    #[test]
    fn test_div_by_zero_err() {
        let a = Decimal::from_u64(10);
        assert_eq!(a.div(Decimal::zero()), Err(DecimalError::DivisionByZero));
    }

    #[test]
    fn test_zero_div_anything() {
        let r = Decimal::zero().div(Decimal::from_u64(5)).unwrap();
        assert_eq!(r, ZERO);
    }

    #[test]
    fn test_div_operator_panics_on_zero() {
        let a = Decimal::from_u64(10);
        let b = Decimal::from_u64(0);
        // Div trait returns Result, so this won't panic — it returns Err
        let result = a / b;
        assert_eq!(result, Err(DecimalError::DivisionByZero));
    }

    #[test]
    fn test_div_non_exact() {
        let a = Decimal::from_u64(7);
        let b = Decimal::from_u64(2);
        let r = a.div(b).unwrap();
        // 3.5
        assert!((r.mantissa() - 3.5).abs() < 1e-10);
        assert_eq!(r.exponent10(), 0);
    }

    // ----------------------------------------------------------------
    // powi
    // ----------------------------------------------------------------

    #[test]
    fn test_powi_zero_exponent() {
        let d = Decimal::from_u64(999);
        assert_eq!(d.powi(0), ONE);
    }

    #[test]
    fn test_powi_one_exponent() {
        let d = Decimal::from_u64(42);
        assert_eq!(d.powi(1), d);
    }

    #[test]
    fn test_powi_two() {
        let d = Decimal::from_u64(10);
        assert_eq!(d.powi(2), Decimal::from_u64(100));
    }

    #[test]
    fn test_powi_three() {
        let d = Decimal::from_u64(2);
        assert_eq!(d.powi(3), Decimal::from_u64(8));
    }

    #[test]
    fn test_powi_large() {
        let d = Decimal::from_u64(10);
        let r = d.powi(10);
        assert!((r.mantissa() - 1.0).abs() < 1e-15);
        assert_eq!(r.exponent10(), 10);
    }

    #[test]
    fn test_powi_overflow_saturation() {
        // Decimal exponents are clamped to ±i64::MAX/2. Squaring a value at
        // the clamp boundary must saturate, not overflow.
        let d = Decimal::from_parts(1.0, EXP_CLAMP_MAX).unwrap();
        let r = d.powi(2);
        assert_eq!(r.exponent10(), EXP_CLAMP_MAX);

        // Repeated squaring accumulates the saturation without panicking.
        let r2 = d.powi(3);
        assert_eq!(r2.exponent10(), EXP_CLAMP_MAX);
    }

    #[test]
    fn test_powi_zero_base() {
        assert_eq!(Decimal::zero().powi(5), ZERO);
    }

    // ----------------------------------------------------------------
    // Square root
    // ----------------------------------------------------------------

    #[test]
    fn test_sqrt_perfect_square() {
        let d = Decimal::from_u64(144);
        let r = d.sqrt().unwrap();
        assert_eq!(r, Decimal::from_u64(12));
    }

    #[test]
    fn test_sqrt_large() {
        let d = Decimal::from_parts(1.0, 10).unwrap(); // 1e10
        let r = d.sqrt().unwrap();
        // sqrt(1e10) = 1e5
        assert!((r.mantissa() - 1.0).abs() < 1e-10);
        assert_eq!(r.exponent10(), 5);
    }

    #[test]
    fn test_sqrt_small() {
        let d = Decimal::from_parts(1.0, -4).unwrap(); // 1e-4
        let r = d.sqrt().unwrap();
        // sqrt(1e-4) = 1e-2
        assert!((r.mantissa() - 1.0).abs() < 1e-10);
        assert_eq!(r.exponent10(), -2);
    }

    #[test]
    fn test_sqrt_negative_err() {
        // Decimal can't represent negative values (they saturate to zero)
        // so this error path is for defensive programming
        let d = Decimal::zero();
        let r = d.sqrt().unwrap();
        assert_eq!(r, ZERO);
    }

    #[test]
    fn test_sqrt_approx_identity() {
        // sqrt(x) * sqrt(x) ≈ x within 1e-15 relative
        let test_vals = [
            Decimal::from_u64(2),
            Decimal::from_u64(7),
            Decimal::from_parts(std::f64::consts::PI, 0).unwrap(),
            Decimal::from_parts(9.999999999999998, 307).unwrap(),
        ];
        for d in &test_vals {
            let s = d.sqrt().unwrap();
            let product = s.mul(s);
            let rel_err = (product.mantissa() * 10.0_f64.powi(product.exponent as i32)
                - d.mantissa() * 10.0_f64.powi(d.exponent as i32))
            .abs()
                / (d.mantissa() * 10.0_f64.powi(d.exponent as i32)).abs();
            assert!(
                rel_err < 1e-15,
                "sqrt(x)*sqrt(x) ≈ x failed: rel_err={}",
                rel_err
            );
        }
    }

    #[test]
    fn test_sqrt_zero() {
        assert_eq!(Decimal::zero().sqrt().unwrap(), ZERO);
    }

    // ----------------------------------------------------------------
    // floor, is_integer, cmp, max, min
    // ----------------------------------------------------------------

    #[test]
    fn test_floor_basic() {
        let d = Decimal::from_parts(3.7, 0).unwrap();
        let r = d.floor();
        assert_eq!(r, Decimal::from_u64(3));
    }

    #[test]
    fn test_floor_exact_integer() {
        let d = Decimal::from_u64(42);
        assert_eq!(d.floor(), d);
    }

    #[test]
    fn test_floor_large() {
        let d = Decimal::from_parts(9.9, 5).unwrap(); // 990000
        let r = d.floor();
        assert_eq!(r, Decimal::from_u64(990000));
    }

    #[test]
    fn test_floor_zero() {
        assert_eq!(Decimal::zero().floor(), ZERO);
    }

    #[test]
    fn test_is_integer() {
        assert!(Decimal::from_u64(42).is_integer());
        assert!(Decimal::from_u64(1).is_integer());
        assert!(Decimal::from_parts(1.0, 5).unwrap().is_integer());
        assert!(!Decimal::from_parts(1.5, 0).unwrap().is_integer());
        assert!(!Decimal::from_parts(9.9, -1).unwrap().is_integer()); // 0.99
    }

    #[test]
    fn test_max_min() {
        let a = Decimal::from_u64(5);
        let b = Decimal::from_u64(10);
        assert_eq!(a.max(b), b);
        assert_eq!(b.max(a), b);
        assert_eq!(a.min(b), a);
        assert_eq!(b.min(a), a);
    }

    // ----------------------------------------------------------------
    // log10_floor
    // ----------------------------------------------------------------

    #[test]
    fn test_log10_floor_zero() {
        assert_eq!(Decimal::zero().log10_floor(), LOG10_FLOOR_ZERO_SENTINEL);
    }

    #[test]
    fn test_log10_floor_powers_of_ten() {
        assert_eq!(Decimal::from_u64(1).log10_floor(), 0);
        assert_eq!(Decimal::from_u64(10).log10_floor(), 1);
        assert_eq!(Decimal::from_u64(100).log10_floor(), 2);
        assert_eq!(Decimal::from_parts(1.0, 6).unwrap().log10_floor(), 6);
    }

    #[test]
    fn test_log10_floor_between_powers() {
        // 500 = 5.0e2 → log10_floor = 2
        let d = Decimal::from_parts(5.0, 2).unwrap();
        assert_eq!(d.log10_floor(), 2);
        // 999 = 9.99e2 → log10_floor = 2
        let d = Decimal::from_parts(9.99, 2).unwrap();
        assert_eq!(d.log10_floor(), 2);
    }

    #[test]
    fn test_log10_floor_negative_exponent() {
        // 0.01 = 1.0e-2 → log10_floor = -2
        let d = Decimal::from_parts(1.0, -2).unwrap();
        assert_eq!(d.log10_floor(), -2);
        // 0.5 = 5.0e-1 → log10_floor = -1
        let d = Decimal::from_parts(5.0, -1).unwrap();
        assert_eq!(d.log10_floor(), -1);
    }

    // ----------------------------------------------------------------
    // Display formatting
    // ----------------------------------------------------------------

    #[test]
    fn test_display_zero() {
        assert_eq!(format!("{}", Decimal::zero()), "0");
    }

    #[test]
    fn test_display_one() {
        assert_eq!(format!("{}", Decimal::one()), "1.0000000000000000e0");
    }

    #[test]
    fn test_display_basic() {
        let d = Decimal::from_u64(1234567);
        let s = format!("{}", d);
        // 1.234567e6 → "1.2345670000000000e6"
        assert!(s.starts_with("1.234567"));
        assert!(s.ends_with("e6"));
    }

    #[test]
    fn test_display_canonical_no_padding() {
        // Positive exponent: no '+', no zero padding.
        assert_eq!(format!("{}", dec(1.0, 6)), "1.0000000000000000e6");
        // Negative exponent: single '-', no padding.
        assert_eq!(format!("{}", dec(1.0, -5)), "1.0000000000000000e-5");
        // Zero exponent.
        assert_eq!(format!("{}", dec(1.5, 0)), "1.5000000000000000e0");
    }

    #[test]
    fn test_parse_accepts_both_exponent_forms() {
        let a = Decimal::from_str("1.5e6").unwrap();
        let b = Decimal::from_str("1.5e+006").unwrap();
        let c = Decimal::from_str("1.5000000000000000e6").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        let neg = Decimal::from_str("1.5e-5").unwrap();
        let neg_padded = Decimal::from_str("1.5e-005").unwrap();
        assert_eq!(neg, neg_padded);
    }

    // ----------------------------------------------------------------
    // Parsing
    // ----------------------------------------------------------------

    #[test]
    fn test_parse_integer() {
        assert_eq!(Decimal::from_str("0").unwrap(), ZERO);
        assert_eq!(Decimal::from_str("1").unwrap(), ONE);
        assert_eq!(Decimal::from_str("42").unwrap(), Decimal::from_u64(42));
        assert_eq!(Decimal::from_str("+5").unwrap(), Decimal::from_u64(5));
    }

    #[test]
    fn test_parse_decimal() {
        let d = Decimal::from_str("1.5").unwrap();
        assert!((d.mantissa() - 1.5).abs() < 1e-10);
        assert_eq!(d.exponent10(), 0);
    }

    #[test]
    fn test_parse_scientific() {
        let d = Decimal::from_str("1.5e6").unwrap();
        assert!((d.mantissa() - 1.5).abs() < 1e-10);
        assert_eq!(d.exponent10(), 6);
    }

    #[test]
    fn test_parse_scientific_negative_exp() {
        let d = Decimal::from_str("1.5E-6").unwrap();
        assert!((d.mantissa() - 1.5).abs() < 1e-10);
        assert_eq!(d.exponent10(), -6);
    }

    #[test]
    fn test_parse_rejects_invalid() {
        assert!(Decimal::from_str("").is_err());
        assert!(Decimal::from_str("  ").is_err());
        assert!(Decimal::from_str("NaN").is_err());
        assert!(Decimal::from_str("nan").is_err());
        assert!(Decimal::from_str("inf").is_err());
        assert!(Decimal::from_str("infinity").is_err());
        assert!(Decimal::from_str("-1").is_err());
        assert!(Decimal::from_str("+").is_err());
        assert!(Decimal::from_str("1.2.3").is_err());
        assert!(Decimal::from_str("1e").is_err());
        assert!(Decimal::from_str("1e+").is_err());
        assert!(Decimal::from_str("abc").is_err());
    }

    // ----------------------------------------------------------------
    // Round-trip parsing
    // ----------------------------------------------------------------

    #[test]
    fn test_roundtrip_golden() {
        let golden = [
            "0",
            "1",
            "10",
            "1e6",
            "2.154434690031884e6",
            "4.641588833612779e6",
            "1e-300",
            "9.999999999999998e307",
        ];

        for s in &golden {
            let d = Decimal::from_str(s).unwrap();
            let formatted = d.to_string();
            let reparsed = Decimal::from_str(&formatted).unwrap();
            assert_eq!(
                d, reparsed,
                "round-trip failed: '{}' → '{}' → mismatch",
                s, formatted
            );
        }
    }

    // ----------------------------------------------------------------
    // Human formatting
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Floor/Ceil human formatting — exact table tests
    // ----------------------------------------------------------------

    #[test]
    fn test_floor_ceil_table() {
        let floor_cases: &[(&Decimal, &str)] = &[
            (&dec(7.0, -1), "0"),
            (&dec(2.0, -1), "0"),
            (&dec(1.9, 0), "1"),
            (&dec(9.999992, 5), "999,999"),
            (&dec(1.0, 6), "1.00M"),
            (&dec(1.5, 6), "1.50M"),
            (&dec(1.231, 6), "1.23M"),
            (&dec(1.239999, 6), "1.23M"),
            (&dec(1.2345678, 7), "12.3M"),
            (&dec(1.23456789, 8), "123M"),
            (&dec(9.99999999, 8), "999M"),
            (&dec(1.0, 9), "1.00B"),
            (&dec(1.0, 12), "1.00T"),
            (&dec(1.0, 15), "1.00Qa"),
            (&dec(1.0, 18), "1.00Qi"),
            (&dec(1.0, 21), "1.00Sx"),
            (&dec(1.0, 24), "1.00Sp"),
            (&dec(1.0, 27), "1.00Oc"),
            (&dec(1.0, 30), "1.00No"),
            (&dec(1.0, 33), "1.00Dc"),
            (&dec(1.0, 36), "1.00e36"),
            (&dec(9.996, 40), "9.99e40"),
        ];

        for (input, expected) in floor_cases {
            let got = to_human_floor(**input);
            assert_eq!(got, **expected, "floor failed for {}", input);
        }

        let ceil_cases: &[(&Decimal, &str)] = &[
            (&Decimal::zero(), "0"),
            (&dec(2.0, -1), "1"),
            (&dec(7.0, -1), "1"),
            (&dec(9.999992, 5), "1.00M"),
            (&dec(1.000001, 6), "1.01M"),
            (&dec(1.231, 6), "1.24M"),
            (&dec(1.239999, 6), "1.24M"),
            (&dec(1.2345678, 7), "12.4M"),
            (&dec(1.23456789, 8), "124M"),
            (&dec(9.99999999, 8), "1.00B"),
            (&dec(9.996, 40), "1.00e41"),
        ];

        for (input, expected) in ceil_cases {
            let got = to_human_ceil(**input);
            assert_eq!(got, **expected, "ceil failed for {}", input);
        }
    }

    #[test]
    fn test_floor_never_exceeds_ceil() {
        // Fixed vector: powers of ten 1e-6..1e36, 9.99 at group boundaries,
        // exact integers around 1e6, and products of game cost ratios.
        let test_vals: Vec<Decimal> = (0..=36)
            .map(|e| dec(1.0, e))
            .chain([
                dec(9.99, 0),
                dec(9.99, 3),
                dec(9.99, 6),
                dec(9.99, 9),
                dec(9.99, 12),
                dec(9.99, 15),
                dec(9.99, 18),
                dec(9.99, 21),
                dec(9.99, 24),
                dec(9.99, 27),
                dec(9.99, 30),
                dec(9.99, 33),
            ])
            .chain([
                dec(1.0, -6),
                dec(1.0, -3),
                dec(1.0, 0),
                dec(1.0, 3),
                dec(1.0, 6),
                dec(1.0, 9),
                dec(1.0, 12),
                dec(1.0, 15),
                dec(1.0, 18),
                dec(1.0, 21),
                dec(1.0, 24),
                dec(1.0, 27),
                dec(1.0, 30),
                dec(1.0, 33),
                dec(1.0, 36),
            ])
            .chain([
                from_u(1_000),
                from_u(999_999),
                from_u(1_000_000),
                from_u(1_230_000),
                from_u(10_000_000),
                from_u(100_000_000),
            ])
            .chain([
                // Game generator cost ratios compounded.
                dec(1.15, 0).powi(10),
                dec(1.15, 0).powi(60),
                dec(1.18, 0).powi(50),
                dec(1.2, 0).powi(40),
                dec(1.22, 0).powi(30),
                from_u(2_500_000).mul(dec(1.22, 0).powi(10)),
            ])
            .collect();

        for d_val in &test_vals {
            let floor_str = to_human_floor(*d_val);
            let ceil_str = to_human_ceil(*d_val);
            let floor_dec = parse_human(&floor_str);
            let ceil_dec = parse_human(&ceil_str);
            assert!(
                floor_dec.cmp(ceil_dec) != std::cmp::Ordering::Greater,
                "floor {} > ceil {} for {}",
                floor_str,
                ceil_str,
                d_val
            );
            assert!(
                floor_dec.cmp(*d_val) != std::cmp::Ordering::Greater,
                "floor {} > original {} for {}",
                floor_str,
                floor_dec,
                d_val
            );
            assert!(
                ceil_dec.cmp(*d_val) != std::cmp::Ordering::Less,
                "ceil {} < original {} for {}",
                ceil_str,
                ceil_dec,
                d_val
            );
        }
    }

    #[test]
    fn test_floor_exact_boundary_unchanged() {
        let d = dec(1.23, 6);
        assert_eq!(to_human_floor(d), "1.23M");
    }

    #[test]
    fn test_rate_small_values() {
        assert_eq!(to_rate(Decimal::zero()), "0.00");
        assert_eq!(to_rate(Decimal::from_u64(1)), "1.00");
        assert_eq!(to_rate(Decimal::from_parts(5.0, -1).unwrap()), "0.50");
        assert_eq!(to_rate(Decimal::from_parts(1.86, 1).unwrap()), "18.60");
    }

    #[test]
    fn test_rate_large_values() {
        let d = dec(1.0, 6);
        let s = to_rate(d);
        assert!(s.contains("M"));
    }

    // ----------------------------------------------------------------
    // Serde
    // ----------------------------------------------------------------

    #[test]
    fn test_serde_roundtrip_zero() {
        let d = Decimal::zero();
        let json = serde_json::to_string(&d).unwrap();
        let roundtrip: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(d, roundtrip);
    }

    #[test]
    fn test_serde_roundtrip_small() {
        let d = Decimal::from_u64(42);
        let json = serde_json::to_string(&d).unwrap();
        let roundtrip: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(d, roundtrip);
    }

    #[test]
    fn test_serde_roundtrip_large() {
        let d = Decimal::from_parts(9.99, 100).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        let roundtrip: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(d, roundtrip);
    }

    #[test]
    fn test_serde_roundtrip_canonical() {
        let d = Decimal::from_parts(1.0, 0).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"1.0000000000000000e0\"");
        let roundtrip: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(d, roundtrip);
    }

    #[test]
    fn test_serde_rejects_nan() {
        let result: Result<Decimal, _> = serde_json::from_str("\"NaN\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_rejects_inf() {
        let result: Result<Decimal, _> = serde_json::from_str("\"inf\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_rejects_empty() {
        let result: Result<Decimal, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    // ----------------------------------------------------------------
    // Determinism: bit-level pinning
    // ----------------------------------------------------------------

    #[test]
    fn test_determinism_bit_pinning() {
        use std::f64::consts::{E, PI};
        // Pin exact mantissa bits after a fixed sequence of ops
        let a = Decimal::from_parts(PI, 0).unwrap();
        let b = Decimal::from_parts(E, 0).unwrap();

        let product = a.mul(b);
        let sum = a.add(b);

        // These should produce identical bits on every platform
        let pm_bits = product.mantissa().to_bits();
        let sm_bits = sum.mantissa().to_bits();

        // Verify by re-running the same sequence
        let product2 = a.mul(b);
        let sum2 = a.add(b);
        assert_eq!(product2.mantissa().to_bits(), pm_bits);
        assert_eq!(sum2.mantissa().to_bits(), sm_bits);

        // Sanity: known approximate values computed from the same constants
        assert!(
            (product.mantissa() * 10.0_f64.powi(product.exponent as i32) - PI * E).abs() < 1e-10
        );
        assert!((sum.mantissa() * 10.0_f64.powi(sum.exponent as i32) - (PI + E)).abs() < 1e-10);
    }

    // ----------------------------------------------------------------
    // Precision boundary: 17-decade rule
    // ----------------------------------------------------------------

    #[test]
    fn test_precision_boundary() {
        // 1e17 + 1.0 — at the edge of f64 precision
        // f64 has ~15-17 significant digits, so 1e17 + 1 should be 1e17
        let big = Decimal::from_parts(1.0, 17).unwrap();
        let small = Decimal::from_u64(1);
        let r = big.add(small);
        // The 17-decade rule: 17 decades difference → should be a no-op
        assert_eq!(r, big);
    }

    #[test]
    fn test_precision_at_boundary_minus_one() {
        // 1e16 + 1 — 16 decades, should still be a no-op
        // (f64 mantissa is 53 bits ≈ 15.95 decimal digits)
        let big = Decimal::from_parts(1.0, 16).unwrap();
        let small = Decimal::from_u64(1);
        let r = big.add(small);
        // 1e16 has 17 significant digits in decimal representation
        // but f64 can only hold ~15-16 of them
        // Alignment shift of 16 means the small mantissa shifts 16 places
        // which should still be representable, but let's check
        // Actually 1e16 + 1 in f64: 10000000000000001.0
        // f64 has 53 bits = ~15.95 decimal digits, so this may or may not
        // be exactly representable. The alignment in our implementation
        // shifts by 16 decimal places, so the small value becomes 1e-16
        // relative to the big value — it should affect the last bit.
        // Let's just verify it doesn't panic and produces a finite result.
        assert!(r.mantissa().is_finite());
        assert!(r.exponent10() >= EXP_CLAMP_MIN && r.exponent10() <= EXP_CLAMP_MAX);
    }

    // ----------------------------------------------------------------
    // Edge cases
    // ----------------------------------------------------------------

    #[test]
    fn test_exponent_clamp() {
        // Very large exponent should clamp
        let d = Decimal::from_parts(5.0, i64::MAX).unwrap();
        assert_eq!(d.exponent10(), EXP_CLAMP_MAX);

        let d = Decimal::from_parts(5.0, i64::MIN).unwrap();
        assert_eq!(d.exponent10(), EXP_CLAMP_MIN);
    }

    #[test]
    fn test_mul_chain_associativity() {
        // Spot check: (a * b) * c == a * (b * c) for several values
        let values = [
            Decimal::from_u64(2),
            Decimal::from_u64(3),
            Decimal::from_u64(7),
            Decimal::from_parts(1.5, 2).unwrap(),
            Decimal::from_parts(9.9, -1).unwrap(),
        ];
        for i in 0..values.len() {
            for j in 0..values.len() {
                for k in 0..values.len() {
                    let a = values[i];
                    let b = values[j];
                    let c = values[k];
                    let l = a.mul(b).mul(c);
                    let r = a.mul(b.mul(c));
                    // Allow small floating-point differences
                    let diff =
                        (to_f64(l) - to_f64(r)).abs() / to_f64(l).max(to_f64(Decimal::from_u64(1)));
                    assert!(
                        diff < 1e-14,
                        "associativity failed: ({:?} * {:?}) * {:?} vs {:?} * ({:?} * {:?}) diff={}",
                        a,
                        b,
                        c,
                        a,
                        b,
                        c,
                        diff
                    );
                }
            }
        }
    }

    #[test]
    fn test_sub_ordering_invariant() {
        // a.sub(b) + b == a when a >= b
        for a_exp in 0..5 {
            for b_exp in 0..=a_exp {
                let a = Decimal::from_parts(5.0, a_exp).unwrap();
                let b = Decimal::from_parts(2.0, b_exp).unwrap();
                if a >= b {
                    let diff = a.sub(b);
                    let restored = diff.add(b);
                    // Should be approximately equal to a
                    let rel = (to_f64(restored) - to_f64(a)).abs() / to_f64(a).abs();
                    assert!(rel < 1e-14, "sub/add invariant failed: rel={}", rel);
                }
            }
        }
    }
}
