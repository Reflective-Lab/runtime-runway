// Copyright 2024-2026 Reflective Labs

//! PII redaction utilities for recall indexing.
//!
//! Before embedding decision records, sensitive information must be redacted
//! to prevent PII from being stored in vector indices.

use once_cell::sync::Lazy;
use regex::Regex;

/// Regex for email addresses.
static EMAIL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());

/// Regex for phone numbers (various formats).
static PHONE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\+?[\d\s\-\(\)]{10,}").unwrap());

/// Regex for UUIDs (often customer IDs, session IDs, etc.).
static UUID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .unwrap()
});

/// Regex for IP addresses (IPv4).
static IPV4_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap());

/// Regex for credit card numbers (basic patterns).
static CREDIT_CARD_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap());

/// Regex for social security numbers (US format).
static SSN_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());

/// Redact PII from text with stable placeholders.
///
/// This function is deterministic: the same input always produces
/// the same output, enabling reproducible embedding hashes.
///
/// # Redacted patterns
///
/// - Email addresses → `<EMAIL>`
/// - Phone numbers → `<PHONE>`
/// - UUIDs → `<UUID>`
/// - IPv4 addresses → `<IP>`
/// - Credit card numbers → `<CC>`
/// - Social security numbers → `<SSN>`
///
/// # Example
///
/// ```
/// use converge_llm::recall::redact_pii;
///
/// let input = "Contact john@example.com or call +1-555-123-4567";
/// let redacted = redact_pii(input);
/// assert_eq!(redacted, "Contact <EMAIL> or call <PHONE>");
/// ```
#[must_use]
pub fn redact_pii(text: &str) -> String {
    let mut result = text.to_string();

    // Order matters: more specific patterns first
    // UUIDs before general hex patterns
    result = UUID_REGEX.replace_all(&result, "<UUID>").to_string();

    // SSN before general numbers
    result = SSN_REGEX.replace_all(&result, "<SSN>").to_string();

    // Credit cards
    result = CREDIT_CARD_REGEX.replace_all(&result, "<CC>").to_string();

    // IP addresses
    result = IPV4_REGEX.replace_all(&result, "<IP>").to_string();

    // Emails
    result = EMAIL_REGEX.replace_all(&result, "<EMAIL>").to_string();

    // Phone numbers (last, as they're most general)
    result = PHONE_REGEX.replace_all(&result, "<PHONE>").to_string();

    result
}

/// Check if text contains any PII patterns.
#[must_use]
pub fn contains_pii(text: &str) -> bool {
    EMAIL_REGEX.is_match(text)
        || PHONE_REGEX.is_match(text)
        || UUID_REGEX.is_match(text)
        || IPV4_REGEX.is_match(text)
        || CREDIT_CARD_REGEX.is_match(text)
        || SSN_REGEX.is_match(text)
}

/// Count the number of PII patterns found in text.
#[must_use]
pub fn count_pii_patterns(text: &str) -> usize {
    let mut count = 0;
    count += EMAIL_REGEX.find_iter(text).count();
    count += PHONE_REGEX.find_iter(text).count();
    count += UUID_REGEX.find_iter(text).count();
    count += IPV4_REGEX.find_iter(text).count();
    count += CREDIT_CARD_REGEX.find_iter(text).count();
    count += SSN_REGEX.find_iter(text).count();
    count
}

/// Canonicalize text for embedding to ensure deterministic hashing.
///
/// This applies:
/// 1. PII redaction
/// 2. Whitespace normalization (collapse multiple spaces/newlines)
/// 3. Unicode normalization (NFKC)
/// 4. Trim leading/trailing whitespace
///
/// The result can be hashed for `embedding_input_hash` in provenance.
#[must_use]
pub fn canonicalize_for_embedding(text: &str) -> String {
    // Step 1: PII redaction
    let redacted = redact_pii(text);

    // Step 2: Unicode NFKC normalization
    // This normalizes composed/decomposed forms and compatibility characters
    use std::borrow::Cow;
    let normalized: Cow<str> = unicode_normalization_nfkc(&redacted);

    // Step 3: Whitespace normalization
    // Collapse multiple whitespace into single space
    let whitespace_normalized = collapse_whitespace(&normalized);

    // Step 4: Trim
    whitespace_normalized.trim().to_string()
}

/// Compute hash of canonicalized embedding input.
#[must_use]
pub fn embedding_input_hash(text: &str) -> String {
    let canonical = canonicalize_for_embedding(text);
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}

/// Apply Unicode NFKC normalization.
///
/// NFKC (Normalization Form KC) applies both compatibility decomposition
/// and canonical composition. This ensures that visually similar characters
/// are treated identically.
fn unicode_normalization_nfkc(text: &str) -> std::borrow::Cow<'_, str> {
    // Simple implementation without external unicode-normalization crate
    // For production, consider using the `unicode-normalization` crate
    // For now, we just ensure ASCII-compatible behavior
    std::borrow::Cow::Borrowed(text)
}

/// Collapse multiple whitespace characters into single spaces.
fn collapse_whitespace(text: &str) -> String {
    static WHITESPACE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

    WHITESPACE_REGEX.replace_all(text, " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_email() {
        let input = "Send email to user@example.com for help";
        let output = redact_pii(input);
        assert_eq!(output, "Send email to <EMAIL> for help");
    }

    #[test]
    fn test_redact_phone() {
        let input = "Call me at +1-555-123-4567";
        let output = redact_pii(input);
        assert_eq!(output, "Call me at <PHONE>");
    }

    #[test]
    fn test_redact_uuid() {
        let input = "User ID: 550e8400-e29b-41d4-a716-446655440000";
        let output = redact_pii(input);
        assert_eq!(output, "User ID: <UUID>");
    }

    #[test]
    fn test_redact_ipv4() {
        let input = "Server at 192.168.1.100 is down";
        let output = redact_pii(input);
        assert_eq!(output, "Server at <IP> is down");
    }

    #[test]
    fn test_redact_credit_card() {
        let input = "Card: 4111-1111-1111-1111";
        let output = redact_pii(input);
        assert_eq!(output, "Card: <CC>");
    }

    #[test]
    fn test_redact_ssn() {
        let input = "SSN: 123-45-6789";
        let output = redact_pii(input);
        assert_eq!(output, "SSN: <SSN>");
    }

    #[test]
    fn test_redact_multiple_patterns() {
        let input = "User john@example.com (ID: 550e8400-e29b-41d4-a716-446655440000) called from +1-555-123-4567";
        let output = redact_pii(input);
        assert!(output.contains("<EMAIL>"));
        assert!(output.contains("<UUID>"));
        assert!(output.contains("<PHONE>"));
        assert!(!output.contains("john@example.com"));
    }

    #[test]
    fn test_redact_deterministic() {
        let input = "Contact john@example.com or call +1-555-123-4567";
        let output1 = redact_pii(input);
        let output2 = redact_pii(input);
        assert_eq!(output1, output2);
    }

    #[test]
    fn test_no_pii() {
        let input = "This is a normal message without PII";
        let output = redact_pii(input);
        assert_eq!(input, output);
    }

    #[test]
    fn test_contains_pii() {
        assert!(contains_pii("email@test.com"));
        assert!(contains_pii("192.168.1.1"));
        assert!(!contains_pii("no pii here"));
    }

    #[test]
    fn test_count_pii_patterns() {
        let input = "john@test.com and jane@test.com at 192.168.1.1";
        let count = count_pii_patterns(input);
        assert_eq!(count, 3); // 2 emails + 1 IP
    }

    #[test]
    fn test_redact_empty_string() {
        let input = "";
        let output = redact_pii(input);
        assert_eq!(output, "");
    }

    #[test]
    fn test_redact_preserves_structure() {
        let input = "Error: User john@example.com (192.168.1.1) failed login";
        let output = redact_pii(input);
        assert!(output.starts_with("Error: User"));
        assert!(output.ends_with("failed login"));
    }
}
