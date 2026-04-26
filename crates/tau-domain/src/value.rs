//! JSON-shaped values used by manifest capability params and tool
//! args/results.
//!
//! `BTreeMap` (not `HashMap`) for deterministic iteration order — matters
//! for golden tests and stable wire format.

use std::collections::BTreeMap;

/// A JSON-shaped value: nullable, scalar, or recursive.
///
/// # Example
///
/// ```
/// use tau_domain::Value;
/// use std::collections::BTreeMap;
///
/// let v = Value::Object({
///     let mut m = BTreeMap::new();
///     m.insert("paths".into(), Value::Array(vec![Value::String("/tmp".into())]));
///     m
/// });
/// assert_eq!(
///     v.as_object().unwrap().get("paths").unwrap().as_array().unwrap().len(),
///     1,
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Value {
    /// JSON null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Signed 64-bit integer.
    Integer(i64),
    /// IEEE-754 double-precision float.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Binary blob (image data, file contents, etc.).
    Bytes(Vec<u8>),
    /// Ordered list of values.
    Array(Vec<Value>),
    /// Sorted-key map of values.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Return the inner string if this is `Value::String`, else `None`.
    pub fn as_string(&self) -> Option<&str> {
        if let Value::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    /// Return the inner integer if this is `Value::Integer`, else `None`.
    pub fn as_integer(&self) -> Option<i64> {
        if let Value::Integer(i) = self {
            Some(*i)
        } else {
            None
        }
    }
    /// Return the inner float if this is `Value::Float`, else `None`.
    pub fn as_float(&self) -> Option<f64> {
        if let Value::Float(f) = self {
            Some(*f)
        } else {
            None
        }
    }
    /// Return the inner bool if this is `Value::Bool`, else `None`.
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
    /// Return the inner bytes if this is `Value::Bytes`, else `None`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(b) = self {
            Some(b)
        } else {
            None
        }
    }
    /// Return the inner array if this is `Value::Array`, else `None`.
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self {
            Some(a)
        } else {
            None
        }
    }
    /// Return the inner object if this is `Value::Object`, else `None`.
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        if let Value::Object(o) = self {
            Some(o)
        } else {
            None
        }
    }
    /// True if this is `Value::Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_match_variants() {
        assert!(Value::Null.is_null());
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Integer(42).as_integer(), Some(42));
        assert_eq!(Value::Float(1.5).as_float(), Some(1.5));
        assert_eq!(Value::String("x".into()).as_string(), Some("x"));
        assert_eq!(Value::Bytes(vec![1, 2]).as_bytes(), Some(&[1u8, 2][..]));
        assert_eq!(Value::Array(vec![Value::Null]).as_array().unwrap().len(), 1);
        let mut o = BTreeMap::new();
        o.insert("k".into(), Value::Bool(false));
        assert!(Value::Object(o).as_object().is_some());
    }

    #[test]
    fn accessors_return_none_for_other_variants() {
        assert_eq!(Value::Null.as_integer(), None);
        assert_eq!(Value::Bool(true).as_string(), None);
        assert!(!Value::Bool(false).is_null());
    }
}
