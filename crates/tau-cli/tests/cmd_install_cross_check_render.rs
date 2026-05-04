//! Insta snapshot tests for render_cross_check_error — sub-project B Task 10.
//!
//! Three snapshots cover the three "structured" CrossCheckError variants
//! that produce different guided output:
//! - BinaryClaimsExtra
//! - ManifestDeclaresUnused
//! - SpawnFailed
//!
//! HandshakeFailed shares a similar shape to SpawnFailed; covered indirectly.
//! Future variants render as a generic fallback (covered by the function's
//! own catch-all arm).

use insta::assert_snapshot;
use tau_pkg::sandbox_check::CrossCheckError;

// Helper: deserialize a Capability from a JSON literal (matching the
// pattern used in tau-pkg's sandbox_check tests, since #[non_exhaustive]
// Capability variants block struct-expression construction outside
// tau-domain).
fn cap_from_json(v: serde_json::Value) -> tau_domain::Capability {
    serde_json::from_value(v).expect("valid Capability JSON")
}

use tau_cli::cmd::error_render::render_cross_check_error;

#[test]
fn render_cross_check_binary_claims_extra() {
    let claimed = cap_from_json(serde_json::json!({
        "kind": "fs.read",
        "paths": ["/etc/passwd"]
    }));
    let err = CrossCheckError::BinaryClaimsExtra {
        plugin: "anthropic".to_string(),
        claimed,
    };
    let rendered = render_cross_check_error(&err);
    assert_snapshot!(rendered);
}

#[test]
fn render_cross_check_manifest_declares_unused() {
    let declared = cap_from_json(serde_json::json!({
        "kind": "net.http",
        "hosts": ["api.example.com"],
        "methods": ["GET"]
    }));
    let err = CrossCheckError::ManifestDeclaresUnused {
        plugin: "ollama".to_string(),
        declared,
    };
    let rendered = render_cross_check_error(&err);
    assert_snapshot!(rendered);
}

#[test]
fn render_cross_check_spawn_failed() {
    let err = CrossCheckError::SpawnFailed(
        "No such file or directory: /tmp/nonexistent-binary".to_string(),
    );
    let rendered = render_cross_check_error(&err);
    assert_snapshot!(rendered);
}
