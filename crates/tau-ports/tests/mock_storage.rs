//! Integration tests for [`tau_ports::fixtures::MockStorage`].
//!
//! Asserts the in-memory `MockStorage` satisfies the [`Storage`] trait
//! contract:
//! - `get` returns `None` for absent keys, `Some(value)` after `put`.
//! - `put` overwrites prior values for the same `(namespace, key)`.
//! - `delete` returns `true` when a key was present, `false` when not.
//! - `list` filters by namespace + prefix; cross-namespace keys do not
//!   leak.
//!
//! Gated behind the `test-fixtures` feature: imports `MockStorage`.

#![cfg(feature = "test-fixtures")]

use tau_ports::fixtures::MockStorage;
use tau_ports::storage::{Key, Namespace, Storage};

fn ns(s: &str) -> Namespace {
    Namespace::try_new(s).unwrap()
}

fn k(s: &str) -> Key {
    Key::try_new(s).unwrap()
}

/// `get` returns `None` for missing keys.
#[tokio::test]
async fn get_returns_none_for_absent_key() {
    let storage = MockStorage::new("mem");
    assert_eq!(storage.name(), "mem");

    let v = storage.get(&ns("scope-a"), &k("missing")).await.unwrap();
    assert!(v.is_none());
}

/// `put` then `get` round-trips the byte payload.
#[tokio::test]
async fn put_then_get_round_trip() {
    let storage = MockStorage::new("mem");
    storage
        .put(&ns("scope"), &k("foo"), b"hello world")
        .await
        .unwrap();

    let v = storage.get(&ns("scope"), &k("foo")).await.unwrap();
    assert_eq!(v.as_deref(), Some(&b"hello world"[..]));
}

/// `put` overwrites a prior value at the same `(namespace, key)`.
#[tokio::test]
async fn put_overwrites_prior_value() {
    let storage = MockStorage::new("mem");
    storage
        .put(&ns("scope"), &k("foo"), b"first")
        .await
        .unwrap();
    storage
        .put(&ns("scope"), &k("foo"), b"second")
        .await
        .unwrap();

    let v = storage.get(&ns("scope"), &k("foo")).await.unwrap();
    assert_eq!(v.as_deref(), Some(&b"second"[..]));
}

/// `delete` returns `true` on first call, `false` thereafter.
#[tokio::test]
async fn delete_idempotent_truth_value() {
    let storage = MockStorage::new("mem");
    storage.put(&ns("scope"), &k("foo"), b"v").await.unwrap();

    let first = storage.delete(&ns("scope"), &k("foo")).await.unwrap();
    assert!(first, "first delete should report true");

    let second = storage.delete(&ns("scope"), &k("foo")).await.unwrap();
    assert!(!second, "second delete should report false (absent)");

    // get is now None.
    assert!(storage
        .get(&ns("scope"), &k("foo"))
        .await
        .unwrap()
        .is_none());
}

/// `list` filters by namespace and prefix; keys in other namespaces
/// don't leak in.
#[tokio::test]
async fn list_filters_by_namespace_and_prefix() {
    let storage = MockStorage::new("mem");

    storage
        .put(&ns("scope-a"), &k("foo:1"), b"x")
        .await
        .unwrap();
    storage
        .put(&ns("scope-a"), &k("foo:2"), b"x")
        .await
        .unwrap();
    storage
        .put(&ns("scope-a"), &k("bar:1"), b"x")
        .await
        .unwrap();
    storage
        .put(&ns("scope-b"), &k("foo:1"), b"x")
        .await
        .unwrap();

    // Empty prefix in scope-a returns all three scope-a keys.
    let mut all_a = storage.list(&ns("scope-a"), "").await.unwrap();
    all_a.sort();
    assert_eq!(all_a.len(), 3);
    assert_eq!(all_a, vec![k("bar:1"), k("foo:1"), k("foo:2")]);

    // Prefix "foo:" in scope-a returns only foo:* keys.
    let mut foo_a = storage.list(&ns("scope-a"), "foo:").await.unwrap();
    foo_a.sort();
    assert_eq!(foo_a, vec![k("foo:1"), k("foo:2")]);

    // scope-b sees only its own foo:1 (no leak from scope-a).
    let all_b = storage.list(&ns("scope-b"), "").await.unwrap();
    assert_eq!(all_b, vec![k("foo:1")]);

    // Non-matching prefix returns empty.
    let none = storage.list(&ns("scope-a"), "zz").await.unwrap();
    assert!(none.is_empty());
}
