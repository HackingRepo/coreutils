// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! GNU date format modifier support
//!
//! This module implements GNU-compatible format modifiers for date formatting.
//! These modifiers extend standard strftime format specifiers with optional
//! width and flag modifiers.
//!
//! ## Syntax
//!
//! Format: `%[flags][width]specifier`
//!
//! ### Flags
//! - `-`: Left-align (pad with spaces on the right)
//! - `_`: Pad with spaces instead of zeros
//! - `0`: Pad with zeros (default for numeric fields)
//! - `^`: Convert to uppercase
//! - `#`: Swap case (not widely used)
//! - `+`: Force display of sign (+ for positive, - for negative)
//!
//! ### Width
//! - One or more digits specifying minimum field width
//! - Field will be padded to this width using the padding character
//!
//! ### Examples
//! - `%10Y`: Year padded to 10 digits with zeros (0000001999)
//! - `%_10m`: Month padded to 10 digits with spaces (        06)
//! - `%-10Y`: Year left-aligned in 10 character field (1999      )
//! - `%^B`: Month name in uppercase (JUNE)
//! - `%+4C`: Century with sign, padded to 4 characters (+019)

use jiff::Zoned;
use jiff::fmt::strtime::{BrokenDownTime, Config, PosixCustom};
use regex::Regex;
use std::fmt;
use std::sync::OnceLock;

/// Error type for format modifier operations
#[derive(Debug)]
pub enum FormatError {
    /// Error from the underlying jiff library
    JiffError(jiff::Error),
    /// Custom error message (reserved for future use)
    #[allow(dead_code)]
    Custom(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JiffError(e) => write!(f, "{e}"),
            Self::Custom(s) => write!(f, "{s}"),
        }
    }
}

impl From<jiff::Error> for FormatError {
    fn from(e: jiff::Error) -> Self {
        Self::JiffError(e)
    }
}

/// Regex to match format specifiers with optional modifiers
/// Pattern: % [flags] [width] specifier
/// Flags: -, _, 0, ^, #, +
/// Width: one or more digits
/// Specifier: any letter or special sequence like :z, ::z, :::z
fn format_spec_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"%([_0^#+-]*)(\d*)(:*[a-zA-Z])").unwrap())
}

/// Check if format string contains any GNU modifiers and format if present.
///
/// This function combines modifier detection and formatting in a single pass
/// for better performance. If no modifiers are found, returns None and the
/// caller should use standard formatting. If modifiers are found, returns
/// the formatted string.
pub fn format_with_modifiers_if_present(
    date: &Zoned,
    format_string: &str,
    config: &Config<PosixCustom>,
) -> Option<Result<String, FormatError>> {
    let re = format_spec_regex();

    // Quick check: does the string contain any modifiers?
    let has_modifiers = re.captures_iter(format_string).any(|cap| {
        let flags = cap.get(1).map_or("", |m| m.as_str());
        let width_str = cap.get(2).map_or("", |m| m.as_str());
        !flags.is_empty() || !width_str.is_empty()
    });

    if !has_modifiers {
        return None;
    }

    // If we have modifiers, format the string
    Some(format_with_modifiers(date, format_string, config))
}

/// Process a format string with GNU modifiers.
///
/// # Arguments
/// * `date` - The date to format
/// * `format_string` - Format string with GNU modifiers
/// * `config` - Strftime configuration
///
/// # Returns
/// Formatted string with modifiers applied
///
/// # Errors
/// Returns `FormatError` if formatting fails
fn format_with_modifiers(
    date: &Zoned,
    format_string: &str,
    config: &Config<PosixCustom>,
) -> Result<String, FormatError> {
    // First, replace %% with a placeholder to avoid matching it
    let placeholder = "\x00PERCENT\x00";
    let temp_format = format_string.replace("%%", placeholder);

    let re = format_spec_regex();
    let mut result = String::new();
    let mut last_end = 0;

    let broken_down = BrokenDownTime::from(date);

    for cap in re.captures_iter(&temp_format) {
        let whole_match = cap.get(0).unwrap();
        let flags = cap.get(1).map_or("", |m| m.as_str());
        let width_str = cap.get(2).map_or("", |m| m.as_str());
        let spec = cap.get(3).unwrap().as_str();

        // Add text before this match
        result.push_str(&temp_format[last_end..whole_match.start()]);

        // Format the base specifier first
        let base_format = format!("%{spec}");
        let formatted = broken_down.to_string_with_config(config, &base_format)?;

        // Check if this specifier has modifiers
        if !flags.is_empty() || !width_str.is_empty() {
            // Apply modifiers to the formatted value
            let width: usize = width_str.parse().unwrap_or(0);
            let modified = apply_modifiers(&formatted, flags, width, spec);
            result.push_str(&modified);
        } else {
            // No modifiers, use formatted value as-is
            result.push_str(&formatted);
        }

        last_end = whole_match.end();
    }

    // Add remaining text
    result.push_str(&temp_format[last_end..]);

    // Restore %% by converting placeholder to %
    let result = result.replace(placeholder, "%");

    Ok(result)
}

/// Apply width and flag modifiers to a formatted value
fn apply_modifiers(value: &str, flags: &str, width: usize, _spec: &str) -> String {
    let mut result = value.to_string();

    // Apply uppercase flag first (before any sign handling)
    if flags.contains('^') {
        result = result.to_uppercase();
    }

    if width == 0 && flags.is_empty() {
        return result;
    }

    let pad_char = if flags.contains('0') {
        '0'
    } else if flags.contains('_') {
        ' '
    } else {
        '0' // default
    };

    let left_align = flags.contains('-');
    let force_sign = flags.contains('+');

    // For underscore/space padding on numeric fields, strip leading zeros first
    if pad_char == ' ' && result.len() <= 2 && result.starts_with('0') {
        // For day/month/hour fields that are zero-padded, strip the leading zero
        if let Some(stripped) = result.strip_prefix('0') {
            if !stripped.is_empty() && stripped.chars().all(|c| c.is_numeric()) {
                result = stripped.to_string();
            }
        }
    }

    // For left-align, strip leading zeros from default-padded values
    if left_align && result.starts_with('0') && result.len() >= 2 {
        if let Some(stripped) = result.strip_prefix('0') {
            if !stripped.is_empty() && stripped.chars().all(|c| c.is_numeric()) {
                result = stripped.to_string();
            }
        }
    }

    // Apply force sign for numeric values
    // Note: When + flag is used, we add the sign BEFORE applying width padding
    // so that for space padding, the space goes after the sign (e.g., "+ 1970")
    if force_sign && !result.starts_with('+') && !result.starts_with('-') {
        if result.chars().next().is_some_and(|c| c.is_numeric()) {
            result.insert(0, '+');
        }
    }

    // Apply width padding
    if width > result.len() {
        let padding = width - result.len();
        if left_align {
            result.push_str(&" ".repeat(padding));
        } else {
            // Determine where to place padding based on pad_char and sign
            let has_sign = result.starts_with('+') || result.starts_with('-');

            if pad_char == '0' && has_sign {
                // Zero padding: sign first, then zeros (e.g., "-0022")
                let sign = result.chars().next().unwrap();
                let rest = &result[1..];
                result = format!("{sign}{}{rest}", "0".repeat(padding));
            } else if pad_char == ' ' && force_sign && has_sign {
                // Space padding with forced sign: sign first, then spaces (e.g., "+ 1970")
                let sign = result.chars().next().unwrap();
                let rest = &result[1..];
                result = format!("{sign}{}{rest}", " ".repeat(padding));
            } else {
                // Default: pad on the left (e.g., "  -22" or "  1999")
                result = format!("{}{result}", pad_char.to_string().repeat(padding));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::{civil, tz::TimeZone};

    fn make_test_date(year: i16, month: i8, day: i8, hour: i8) -> Zoned {
        civil::date(year, month, day)
            .at(hour, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap()
    }

    fn get_config() -> Config<PosixCustom> {
        Config::new().custom(PosixCustom::new()).lenient(true)
    }

    #[test]
    fn test_width_and_padding_modifiers() {
        let date = make_test_date(1999, 6, 1, 0);
        let config = get_config();

        // Test basic width with zero padding
        let result = format_with_modifiers(&date, "%10Y", &config).unwrap();
        assert_eq!(result, "0000001999");

        // Test large width
        let result = format_with_modifiers(&date, "%20Y", &config).unwrap();
        assert_eq!(result, "00000000000000001999");
        assert_eq!(result.len(), 20);

        // Test underscore (space) padding with month
        let result = format_with_modifiers(&date, "%_10m", &config).unwrap();
        assert_eq!(result, "         6");
        assert_eq!(result.len(), 10);

        // Test underscore padding with day
        let date_day5 = make_test_date(1999, 6, 5, 0);
        let result = format_with_modifiers(&date_day5, "%_10d", &config).unwrap();
        assert_eq!(result, "         5");
    }

    #[test]
    fn test_alignment_and_case_flags() {
        let date = make_test_date(1999, 6, 1, 0);
        let config = get_config();

        // Test left-align: %-10Y should left-align year
        let result = format_with_modifiers(&date, "%-10Y", &config).unwrap();
        assert_eq!(result, "1999      ");
        assert_eq!(result.len(), 10);

        // Test uppercase: %^B should uppercase month name
        let result = format_with_modifiers(&date, "%^B", &config).unwrap();
        assert_eq!(result, "JUNE");

        // Test uppercase with width: %^10B should uppercase and pad
        let result = format_with_modifiers(&date, "%^10B", &config).unwrap();
        assert_eq!(result, "000000JUNE");
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn test_sign_flags() {
        let date = make_test_date(1970, 1, 1, 0);
        let config = get_config();

        // Test force sign with century: %+4C
        let result = format_with_modifiers(&date, "%+4C", &config).unwrap();
        assert!(result.starts_with('+'));
        assert_eq!(result.len(), 4);

        // Test force sign with zero padding: %+6Y
        let result = format_with_modifiers(&date, "%+6Y", &config).unwrap();
        assert_eq!(result, "+01970");
    }

    #[test]
    fn test_combined_flags_underscore_and_sign() {
        let date = make_test_date(1970, 1, 1, 0);
        let config = get_config();
        // %_+6Y should show year with + sign and space padding
        let result = format_with_modifiers(&date, "%_+6Y", &config).unwrap();
        assert_eq!(result, "+ 1970");
    }

    #[test]
    fn test_combined_flags_left_align_and_uppercase() {
        let date = make_test_date(1999, 6, 1, 0);
        let config = get_config();
        // %-^10B should uppercase and left-align month name
        let result = format_with_modifiers(&date, "%-^10B", &config).unwrap();
        assert_eq!(result, "JUNE      ");
    }

    #[test]
    fn test_edge_cases_and_special_formats() {
        let date = make_test_date(1999, 6, 1, 0);
        let config = get_config();

        // Test width zero (no effect)
        let result = format_with_modifiers(&date, "%Y", &config).unwrap();
        assert_eq!(result, "1999");

        // Test no modifiers (standard format)
        let result = format_with_modifiers(&date, "%Y-%m-%d", &config).unwrap();
        assert_eq!(result, "1999-06-01");

        // Test %% escape sequence
        let result = format_with_modifiers(&date, "%%Y=%Y", &config).unwrap();
        assert_eq!(result, "%Y=1999");

        // Test multiple modifiers in one format string
        let result = format_with_modifiers(&date, "%10Y-%_5m-%-5d", &config).unwrap();
        assert_eq!(result, "0000001999-    6-1    ");
    }

    #[test]
    fn test_modifier_detection() {
        let date = make_test_date(1999, 6, 1, 0);
        let config = get_config();

        // Should detect modifiers
        let result = format_with_modifiers_if_present(&date, "%10Y", &config);
        assert!(result.is_some());

        // Should not detect modifiers
        let result = format_with_modifiers_if_present(&date, "%Y-%m-%d", &config);
        assert!(result.is_none());

        // Should detect flag without width
        let result = format_with_modifiers_if_present(&date, "%^B", &config);
        assert!(result.is_some());
    }

    #[test]
    fn test_negative_values_with_space_padding() {
        // Test case from GNU test: neg-secs2
        // Format: %_5s with value -22 should produce "  -22" (space-padded)
        use jiff::Timestamp;

        let ts = Timestamp::from_second(-22).unwrap();
        let date = ts.to_zoned(TimeZone::UTC);
        let config = get_config();

        let result = format_with_modifiers(&date, "%_5s", &config).unwrap();
        assert_eq!(
            result, "  -22",
            "Space padding should pad before the sign for negative numbers"
        );
    }
}
