//! Path validation + glob admission for `fs-read`.
//!
//! See `docs/superpowers/specs/2026-04-29-tool-plugins-design.md`
//! §4.3, §7.

#![allow(dead_code)] // consumed by Task 9 (plugin.rs invoke).

/// Reasons a path is rejected at validation time.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BadArgs {
    /// The path string was empty.
    Empty,
    /// The path contained a NUL byte.
    NullByte,
    /// The path was relative; absolute paths required.
    NotAbsolute,
    /// The path contained a `..` segment.
    Traversal,
    /// The path was outside the agent's fs.read capability scope.
    NotInScope,
}

impl BadArgs {
    /// Human-readable reason string surfaced in `ToolError::BadArgs`.
    pub(crate) fn reason(&self) -> String {
        match self {
            BadArgs::Empty => "fs-read: path is empty".into(),
            BadArgs::NullByte => "fs-read: path contains a NUL byte".into(),
            BadArgs::NotAbsolute => "fs-read: path is not absolute".into(),
            BadArgs::Traversal => "fs-read: path contains a `..` segment".into(),
            BadArgs::NotInScope => "fs-read: path is not in capability scope".into(),
        }
    }
}

/// Validate the syntactic shape of a path. Returns the path on
/// success, or a [`BadArgs`] reason on failure.
pub(crate) fn validate_path(path: &str) -> Result<&str, BadArgs> {
    if path.is_empty() {
        return Err(BadArgs::Empty);
    }
    if path.bytes().any(|b| b == 0) {
        return Err(BadArgs::NullByte);
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(BadArgs::NotAbsolute);
    }
    if path.split(std::path::MAIN_SEPARATOR).any(|seg| seg == "..") {
        return Err(BadArgs::Traversal);
    }
    Ok(path)
}

/// Check whether `path` is admissible under the active glob list.
/// Returns true iff at least one glob matches.
pub(crate) fn admit(path: &str, allowed_globs: &[String]) -> bool {
    use globset::Glob;
    allowed_globs.iter().any(|g| {
        Glob::new(g)
            .ok()
            .map(|gl| gl.compile_matcher().is_match(path))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_empty_rejected() {
        assert_eq!(validate_path(""), Err(BadArgs::Empty));
    }

    #[test]
    fn validate_path_null_byte_rejected() {
        assert_eq!(validate_path("/tmp/foo\0bar"), Err(BadArgs::NullByte));
    }

    #[test]
    fn validate_path_relative_rejected() {
        assert_eq!(validate_path("./foo"), Err(BadArgs::NotAbsolute));
        assert_eq!(validate_path("foo/bar"), Err(BadArgs::NotAbsolute));
    }

    #[test]
    fn validate_path_traversal_rejected_dotdot_segment() {
        assert_eq!(validate_path("/../etc/passwd"), Err(BadArgs::Traversal));
    }

    #[test]
    fn validate_path_traversal_rejected_in_middle() {
        assert_eq!(validate_path("/tmp/../etc/passwd"), Err(BadArgs::Traversal));
    }

    #[test]
    fn validate_path_happy_path_returns_path() {
        assert_eq!(validate_path("/tmp/foo.txt"), Ok("/tmp/foo.txt"));
    }

    #[test]
    fn admit_matches_simple_glob() {
        let globs = vec!["/tmp/**".to_string()];
        assert!(admit("/tmp/foo.txt", &globs));
        assert!(admit("/tmp/sub/bar.txt", &globs));
    }

    #[test]
    fn admit_does_not_match_outside_scope() {
        let globs = vec!["/var/**".to_string()];
        assert!(!admit("/tmp/foo.txt", &globs));
    }

    #[test]
    fn admit_returns_false_for_invalid_glob() {
        // An invalid glob pattern is treated as no-match (defensive).
        let globs = vec!["[unclosed".to_string()];
        assert!(!admit("/tmp/foo", &globs));
    }

    #[test]
    fn admit_empty_glob_list_returns_false() {
        let globs: Vec<String> = vec![];
        assert!(!admit("/tmp/foo", &globs));
    }

    #[test]
    fn admit_multiple_globs_first_match_wins() {
        let globs = vec!["/var/**".to_string(), "/tmp/**".to_string()];
        assert!(admit("/tmp/foo", &globs));
    }
}
