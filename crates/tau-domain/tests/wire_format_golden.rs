//! Golden-file tests for the wire format. Each canonical input file
//! deserializes and serializes back byte-for-byte (after normalization).
//!
//! When a derive(Serialize) change ships a wire break, these tests fail
//! at PR review.

#![cfg(feature = "serde")]

use std::str::FromStr;

use tau_domain::{Message, PackageSource};

fn read(path: &str) -> String {
    std::fs::read_to_string(path).expect(path)
}

fn round_trip_message(path: &str) {
    let raw = read(path);
    let parsed: Message = serde_json::from_str(&raw).unwrap();
    let re = serde_json::to_string_pretty(&parsed).unwrap();
    // Normalize trailing whitespace.
    assert_eq!(re.trim(), raw.trim(), "wire format drifted for {path}");
}

#[test]
fn message_text_golden() {
    round_trip_message("tests/wire_format/message_text.json");
}

#[test]
fn message_tool_call_golden() {
    round_trip_message("tests/wire_format/message_tool_call.json");
}

#[test]
fn message_tool_result_golden() {
    round_trip_message("tests/wire_format/message_tool_result.json");
}

#[test]
fn message_lifecycle_golden() {
    round_trip_message("tests/wire_format/message_lifecycle.json");
}

#[test]
fn package_source_https_golden() {
    let raw = read("tests/wire_format/package_source_https.txt");
    let parsed = PackageSource::from_str(raw.trim()).unwrap();
    assert_eq!(parsed.to_string(), raw.trim());
}

#[test]
fn package_source_scp_golden() {
    let raw = read("tests/wire_format/package_source_scp.txt");
    let parsed = PackageSource::from_str(raw.trim()).unwrap();
    assert_eq!(parsed.to_string(), raw.trim());
}
