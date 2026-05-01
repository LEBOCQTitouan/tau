//! UUID v7 session ids with prefix resolution.
//!
//! v7 = timestamp-prefixed UUIDs (sortable lexicographically by
//! creation time). Matches the `AgentInstanceId::new()` precedent in
//! `tau_domain`. CLI accepts shortened prefixes (≥8 chars); resolution
//! finds the longest unique match.

use std::fs;
use std::path::Path;

use uuid::Uuid;

use super::SessionError;

/// Minimum prefix length the CLI will accept.
pub const MIN_PREFIX_LEN: usize = 8;

/// A session id wrapping a UUID v7.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Wrap a raw `Uuid` (used by parsers/tests).
    // Staged for Task 6 (resume parser).
    #[allow(dead_code)]
    pub fn from_uuid(u: Uuid) -> Self {
        Self(u)
    }

    /// Underlying UUID.
    // Staged for Task 6+ internals.
    #[allow(dead_code)]
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Full 36-char canonical form.
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// 8-char prefix used for displays and stem of the JSONL filename.
    pub fn short(&self) -> String {
        self.0.to_string()[..MIN_PREFIX_LEN].to_string()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Mint a new session id (UUID v7 — timestamp-prefixed, sortable).
pub fn mint() -> SessionId {
    SessionId(Uuid::now_v7())
}

/// Resolve a user-supplied id-or-prefix to an exact `SessionId` by
/// scanning `<sessions_dir>/*.jsonl`.
// Staged for Task 6 (--resume flag).
#[allow(dead_code)]
///
/// - Exact 36-char match: short-circuits.
/// - 8+ char prefix: matches against canonical filenames; one hit
///   = success, multiple = `AmbiguousPrefix`, zero = `NotFound`.
/// - Anything else: `NotFound`.
///
/// `sessions_dir` is `<scope.state_path()>/sessions`. If the dir does
/// not exist, returns `NotFound`.
pub fn resolve_id_prefix(
    sessions_dir: &Path,
    id_or_prefix: &str,
) -> Result<SessionId, SessionError> {
    // Exact UUID? Skip the directory walk.
    if let Ok(uuid) = id_or_prefix.parse::<Uuid>() {
        let target = sessions_dir.join(format!("{uuid}.jsonl"));
        if target.exists() {
            return Ok(SessionId(uuid));
        }
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    if id_or_prefix.len() < MIN_PREFIX_LEN {
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    if !sessions_dir.exists() {
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    let entries = fs::read_dir(sessions_dir).map_err(|e| SessionError::Io {
        path: sessions_dir.to_path_buf(),
        message: format!("listing sessions dir: {e}"),
    })?;

    let mut matches: Vec<Uuid> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("reading dir entry: {e}"),
        })?;
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        if !stem.starts_with(id_or_prefix) {
            continue;
        }
        if let Ok(uuid) = stem.parse::<Uuid>() {
            matches.push(uuid);
        }
    }

    match matches.len() {
        0 => Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        }),
        1 => Ok(SessionId(matches.into_iter().next().unwrap())),
        _ => {
            let candidates = matches
                .iter()
                .map(|u| u.to_string()[..MIN_PREFIX_LEN].to_string())
                .collect();
            Err(SessionError::AmbiguousPrefix {
                prefix: id_or_prefix.to_string(),
                candidates,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch_session(dir: &Path, id: &str) {
        fs::write(dir.join(format!("{id}.jsonl")), b"{}\n").unwrap();
    }

    #[test]
    fn mint_returns_v7_uuid() {
        let id1 = mint();
        let id2 = mint();
        // v7 → timestamp-prefixed; sortable; non-equal across calls.
        assert_ne!(id1, id2);
        assert_eq!(id1.as_str().len(), 36);
        assert_eq!(id1.short().len(), 8);
    }

    #[test]
    fn resolve_exact_match_succeeds() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let got = resolve_id_prefix(td.path(), &id.as_str()).unwrap();
        assert_eq!(got, id);
    }

    #[test]
    fn resolve_prefix_match_succeeds() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let prefix = &id.as_str()[..10];
        let got = resolve_id_prefix(td.path(), prefix).unwrap();
        assert_eq!(got, id);
    }

    #[test]
    fn resolve_short_prefix_returns_not_found() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        // Less than MIN_PREFIX_LEN chars.
        let err = resolve_id_prefix(td.path(), "abc").unwrap_err();
        assert!(matches!(err, SessionError::NotFound { .. }));
    }

    #[test]
    fn resolve_unknown_id_returns_not_found() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let err = resolve_id_prefix(td.path(), "00000000").unwrap_err();
        assert!(matches!(err, SessionError::NotFound { .. }));
    }

    #[test]
    fn resolve_ambiguous_prefix_returns_candidates() {
        let td = TempDir::new().unwrap();
        // Two ids that share a known prefix. Use UUIDs constructed by
        // hand so we can guarantee the shared first byte.
        let a = "01234567-0000-7000-8000-000000000001";
        let b = "01234567-0000-7000-8000-000000000002";
        touch_session(td.path(), a);
        touch_session(td.path(), b);
        let err = resolve_id_prefix(td.path(), "01234567").unwrap_err();
        let SessionError::AmbiguousPrefix { candidates, .. } = err else {
            panic!("expected AmbiguousPrefix")
        };
        assert_eq!(candidates.len(), 2);
    }
}
