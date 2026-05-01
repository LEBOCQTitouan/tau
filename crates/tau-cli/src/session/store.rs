//! JSONL session file I/O.
//!
//! - `SessionHeader` is line 1 of every session file. Schema version,
//!   id, agent metadata, package metadata, llm backend.
//! - `SessionEntry` enumerates the line types found after the header
//!   (`Message` and `TurnSummary`).
//! - `SessionWriter` opens an append-only handle; per-turn calls
//!   append message + turn_summary lines.
//! - `SessionReader` parses an existing file into header + entries.
//!   Tolerates a trailing malformed line (logs a `tracing::warn!` and
//!   continues), so a crashed REPL can still resume.

#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tau_domain::Message;

use super::id::SessionId;
use super::SessionError;

/// Current session-file schema version. Bump on breaking changes only.
pub const SCHEMA_VERSION: u32 = 1;

/// Package metadata recorded in the session header for drift checks.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPackage {
    /// Package name (e.g. `"my-coder-agent"`).
    pub name: String,
    /// Resolved semver at session-creation time.
    pub version: String,
    /// 40-char git commit SHA at session-creation time.
    pub resolved_commit: String,
}

/// Session header — line 1 of every session file.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHeader {
    /// Schema discriminator: always `"header"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Schema version. Bump on breaking changes.
    pub schema: u32,
    /// Full UUID v7 string.
    pub id: String,
    /// Timestamp at session creation. Serializes to RFC 3339.
    #[serde(with = "humantime_serde")]
    pub created_at: SystemTime,
    /// Named entry in the project tau.toml (e.g. `"coder"`).
    pub agent_id: String,
    /// Package metadata for drift checks.
    pub package: SessionPackage,
    /// LLM backend package name (e.g. `"anthropic"`).
    pub llm_backend: String,
    /// User-supplied title (deferred polish; v0.1 always None).
    #[serde(default)]
    pub title: Option<String>,
}

impl SessionHeader {
    /// Construct a fresh header for a new session.
    pub fn new(
        id: &SessionId,
        agent_id: String,
        package: SessionPackage,
        llm_backend: String,
    ) -> Self {
        Self {
            kind: "header".to_string(),
            schema: SCHEMA_VERSION,
            id: id.as_str(),
            created_at: SystemTime::now(),
            agent_id,
            package,
            llm_backend,
            title: None,
        }
    }
}

/// Parsed entries that follow the header in a session file.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SessionEntry {
    /// A conversation message (`{"type":"message","msg":<Message>}`).
    Message(Message),
    /// Per-turn metadata (`{"type":"turn_summary","turn":N,...}`).
    TurnSummary {
        /// 1-indexed turn number.
        turn: u32,
        /// Stop reason as a string (e.g. `"EndTurn"`, `"ToolUse"`).
        stop_reason: String,
        /// Optional input/output token counts; `None` if absent.
        input_tokens: Option<u64>,
        /// Optional output tokens.
        output_tokens: Option<u64>,
    },
}

/// Append-only writer. Owns an open file handle for the REPL's
/// duration.
pub struct SessionWriter {
    file: File,
    path: PathBuf,
}

impl SessionWriter {
    /// Create a new session file at `<sessions_dir>/<id>.jsonl`.
    /// Writes the header line as the first line. Creates
    /// `<sessions_dir>` if missing.
    pub fn create(
        sessions_dir: &Path,
        id: &SessionId,
        header: &SessionHeader,
    ) -> Result<Self, SessionError> {
        std::fs::create_dir_all(sessions_dir).map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("creating sessions dir: {e}"),
        })?;
        let path = sessions_dir.join(format!("{}.jsonl", id.as_str()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| SessionError::Io {
                path: path.clone(),
                message: format!("creating session file: {e}"),
            })?;

        let header_line = serde_json::to_string(header).map_err(|e| SessionError::Io {
            path: path.clone(),
            message: format!("serializing header: {e}"),
        })?;
        file.write_all(header_line.as_bytes())
            .map_err(|e| SessionError::Io {
                path: path.clone(),
                message: format!("writing header: {e}"),
            })?;
        file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: path.clone(),
            message: format!("writing header newline: {e}"),
        })?;

        Ok(Self { file, path })
    }

    /// Open an existing session file in append mode (used after
    /// resume).
    pub fn open_append(path: &Path) -> Result<Self, SessionError> {
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| SessionError::Io {
                path: path.to_path_buf(),
                message: format!("opening for append: {e}"),
            })?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Append one message line.
    pub fn append_message(&mut self, msg: &Message) -> Result<(), SessionError> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            msg: &'a Message,
        }
        let w = Wire {
            kind: "message",
            msg,
        };
        let line = serde_json::to_string(&w).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("serializing message: {e}"),
        })?;
        self.file
            .write_all(line.as_bytes())
            .map_err(|e| SessionError::Io {
                path: self.path.clone(),
                message: format!("writing message: {e}"),
            })?;
        self.file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing message newline: {e}"),
        })?;
        Ok(())
    }

    /// Append several message lines (one per turn's worth of new
    /// messages).
    pub fn append_messages(&mut self, msgs: &[Message]) -> Result<(), SessionError> {
        for m in msgs {
            self.append_message(m)?;
        }
        Ok(())
    }

    /// Append a turn-summary line.
    pub fn append_turn_summary(
        &mut self,
        turn: u32,
        stop_reason: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<(), SessionError> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            turn: u32,
            stop_reason: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            input_tokens: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            output_tokens: Option<u64>,
        }
        let w = Wire {
            kind: "turn_summary",
            turn,
            stop_reason,
            input_tokens,
            output_tokens,
        };
        let line = serde_json::to_string(&w).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("serializing turn_summary: {e}"),
        })?;
        self.file
            .write_all(line.as_bytes())
            .map_err(|e| SessionError::Io {
                path: self.path.clone(),
                message: format!("writing turn_summary: {e}"),
            })?;
        self.file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing turn_summary newline: {e}"),
        })?;
        Ok(())
    }

    /// Flush and close (consumes self).
    pub fn close(mut self) -> Result<(), SessionError> {
        self.file.flush().map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("flushing on close: {e}"),
        })?;
        Ok(())
    }

    /// Path to the session file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Parser for an existing session file.
pub struct SessionReader;

impl SessionReader {
    /// Open and parse a session file. Returns the header + every
    /// non-header entry. Skips a trailing malformed line gracefully
    /// (logs `tracing::warn!`).
    pub fn read(path: &Path) -> Result<(SessionHeader, Vec<SessionEntry>), SessionError> {
        let file = File::open(path).map_err(|e| SessionError::Io {
            path: path.to_path_buf(),
            message: format!("opening for read: {e}"),
        })?;
        let reader = BufReader::new(file);

        let mut lines = reader.lines();
        let header_line = lines.next().ok_or_else(|| SessionError::InvalidHeader {
            id: file_stem(path),
            detail: "empty file".to_string(),
        })?;
        let header_line = header_line.map_err(|e| SessionError::Io {
            path: path.to_path_buf(),
            message: format!("reading header line: {e}"),
        })?;
        let header: SessionHeader =
            serde_json::from_str(&header_line).map_err(|e| SessionError::InvalidHeader {
                id: file_stem(path),
                detail: format!("parsing header JSON: {e}"),
            })?;

        if header.kind != "header" {
            return Err(SessionError::InvalidHeader {
                id: file_stem(path),
                detail: format!("first line type is {:?}, expected \"header\"", header.kind),
            });
        }
        if header.schema != SCHEMA_VERSION {
            return Err(SessionError::UnsupportedSchema {
                id: header.id.clone(),
                schema: header.schema,
                supported: SCHEMA_VERSION,
            });
        }

        let mut entries = Vec::new();
        let collected: Vec<_> = lines.collect();
        let total = collected.len();
        for (idx, line) in collected.into_iter().enumerate() {
            let line_no = idx + 2;
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    return Err(SessionError::Io {
                        path: path.to_path_buf(),
                        message: format!("reading line {line_no}: {e}"),
                    });
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            match parse_entry(&line) {
                Ok(entry) => entries.push(entry),
                Err(_) if idx + 1 == total => {
                    tracing::warn!(
                        name = "session.partial_line_skipped",
                        path = %path.display(),
                        line_no = line_no,
                        "trailing malformed line skipped (likely crashed mid-write)"
                    );
                }
                Err(e) => {
                    return Err(SessionError::Parse {
                        path: path.to_path_buf(),
                        line: line_no,
                        message: e,
                    });
                }
            }
        }

        Ok((header, entries))
    }
}

fn parse_entry(line: &str) -> Result<SessionEntry, String> {
    #[derive(Deserialize)]
    struct Discriminator {
        #[serde(rename = "type")]
        kind: String,
    }
    let disc: Discriminator =
        serde_json::from_str(line).map_err(|e| format!("discriminator: {e}"))?;
    match disc.kind.as_str() {
        "message" => {
            #[derive(Deserialize)]
            struct Wire {
                msg: Message,
            }
            let w: Wire = serde_json::from_str(line).map_err(|e| format!("message body: {e}"))?;
            Ok(SessionEntry::Message(w.msg))
        }
        "turn_summary" => {
            #[derive(Deserialize)]
            struct Wire {
                turn: u32,
                stop_reason: String,
                #[serde(default)]
                input_tokens: Option<u64>,
                #[serde(default)]
                output_tokens: Option<u64>,
            }
            let w: Wire =
                serde_json::from_str(line).map_err(|e| format!("turn_summary body: {e}"))?;
            Ok(SessionEntry::TurnSummary {
                turn: w.turn,
                stop_reason: w.stop_reason,
                input_tokens: w.input_tokens,
                output_tokens: w.output_tokens,
            })
        }
        other => Err(format!("unknown entry type: {other}")),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

/// Lightweight session summary used by `tau session list`.
///
/// Built from the file's header line + a count of subsequent
/// non-header lines. Reads the whole file (cheap for typical
/// session sizes < 10MB).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    /// Full session id.
    pub id: String,
    /// 8-char id prefix.
    pub short: String,
    /// Agent named entry from project tau.toml.
    pub agent_id: String,
    /// Created-at timestamp (SystemTime; same as in SessionHeader).
    pub created_at: std::time::SystemTime,
    /// Number of `message` + `turn_summary` lines (best-effort; on
    /// read errors falls back to `0`).
    pub turn_count: u32,
    /// Optional title (always None at v0.1).
    pub title: Option<String>,
    /// Path to the session file.
    pub path: PathBuf,
}

/// List session metadata for every `*.jsonl` in `<sessions_dir>`.
///
/// Sort is descending by `created_at`. `agent_filter` filters by
/// `header.agent_id` if `Some(name)`. Files that fail to parse the
/// header line are silently skipped (logged at `warn`).
pub fn list_sessions(
    sessions_dir: &Path,
    agent_filter: Option<&str>,
) -> Result<Vec<SessionMetadata>, SessionError> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(sessions_dir).map_err(|e| SessionError::Io {
        path: sessions_dir.to_path_buf(),
        message: format!("listing sessions dir: {e}"),
    })?;

    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("reading dir entry: {e}"),
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        let (header, count) = match SessionReader::read(&path) {
            Ok((h, entries)) => (h, entries.len() as u32),
            Err(e) => {
                tracing::warn!(
                    name = "session.list_skipped",
                    path = %path.display(),
                    error = %e,
                    "skipping malformed session file"
                );
                continue;
            }
        };

        if let Some(filter) = agent_filter {
            if header.agent_id != filter {
                continue;
            }
        }

        let short = header.id[..super::id::MIN_PREFIX_LEN].to_string();
        out.push(SessionMetadata {
            id: header.id,
            short,
            agent_id: header.agent_id,
            created_at: header.created_at,
            turn_count: count,
            title: header.title,
            path,
        });
    }

    // Descending by created_at — newest first.
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::{Address, MessagePayload};
    use tempfile::TempDir;

    fn fixture_header(id: &SessionId) -> SessionHeader {
        SessionHeader::new(
            id,
            "coder".to_string(),
            SessionPackage {
                name: "my-coder-agent".to_string(),
                version: "1.0.0".to_string(),
                resolved_commit: "0".repeat(40),
            },
            "anthropic".to_string(),
        )
    }

    fn user_msg(text: &str) -> Message {
        Message::new(
            Address::User,
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    #[test]
    fn writer_create_writes_header_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(r#""type":"header""#));
    }

    #[test]
    fn writer_append_message_adds_one_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn writer_append_turn_summary_serializes_optional_fields() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer
            .append_turn_summary(1, "EndTurn", Some(10), Some(5))
            .unwrap();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let last = contents.lines().last().unwrap();
        assert!(last.contains(r#""input_tokens":10"#));
        assert!(last.contains(r#""output_tokens":5"#));
    }

    #[test]
    fn reader_round_trips_header_and_messages() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header_in = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header_in).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.append_message(&user_msg("world")).unwrap();
        writer
            .append_turn_summary(1, "EndTurn", Some(7), Some(3))
            .unwrap();
        writer.close().unwrap();

        let (header_out, entries) = SessionReader::read(&path).unwrap();
        assert_eq!(header_in, header_out);
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0], SessionEntry::Message(_)));
        assert!(matches!(entries[1], SessionEntry::Message(_)));
        let SessionEntry::TurnSummary { turn, .. } = &entries[2] else {
            panic!("expected TurnSummary")
        };
        assert_eq!(*turn, 1);
    }

    #[test]
    fn reader_rejects_unsupported_schema() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let path = td.path().join(format!("{}.jsonl", id.as_str()));
        std::fs::write(
            &path,
            r#"{"type":"header","schema":99,"id":"x","created_at":"2026-05-01T14:33:21Z","agent_id":"x","package":{"name":"x","version":"x","resolved_commit":"x"},"llm_backend":"x"}
"#,
        )
        .unwrap();
        let err = SessionReader::read(&path).unwrap_err();
        assert!(matches!(err, SessionError::UnsupportedSchema { .. }));
    }

    #[test]
    fn reader_skips_trailing_malformed_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.close().unwrap();
        // Append a partial line as if a crash happened mid-write.
        let mut handle = OpenOptions::new().append(true).open(&path).unwrap();
        handle.write_all(b"{not json").unwrap();
        drop(handle);

        let (_, entries) = SessionReader::read(&path).unwrap();
        // First message survives; trailing malformed line skipped.
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn list_sessions_empty_dir_returns_empty() {
        let td = TempDir::new().unwrap();
        let result = list_sessions(td.path(), None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_sessions_returns_descending_by_created_at() {
        let td = TempDir::new().unwrap();
        let id_a = crate::session::id::mint();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id_b = crate::session::id::mint();
        let header_a = fixture_header(&id_a);
        let header_b = fixture_header(&id_b);

        SessionWriter::create(td.path(), &id_a, &header_a)
            .unwrap()
            .close()
            .unwrap();
        SessionWriter::create(td.path(), &id_b, &header_b)
            .unwrap()
            .close()
            .unwrap();

        let result = list_sessions(td.path(), None).unwrap();
        assert_eq!(result.len(), 2);
        // b is newer, so it comes first (descending).
        assert_eq!(result[0].id, id_b.as_str());
        assert_eq!(result[1].id, id_a.as_str());
    }

    #[test]
    fn list_sessions_filters_by_agent() {
        let td = TempDir::new().unwrap();
        let id_a = crate::session::id::mint();
        let id_b = crate::session::id::mint();

        let mut header_a = fixture_header(&id_a);
        header_a.agent_id = "coder".into();
        let mut header_b = fixture_header(&id_b);
        header_b.agent_id = "notes".into();

        SessionWriter::create(td.path(), &id_a, &header_a)
            .unwrap()
            .close()
            .unwrap();
        SessionWriter::create(td.path(), &id_b, &header_b)
            .unwrap()
            .close()
            .unwrap();

        let result = list_sessions(td.path(), Some("coder")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_id, "coder");
    }
}
