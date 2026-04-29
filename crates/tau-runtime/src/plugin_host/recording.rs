//! Protocol recording: capture every wire frame to a `JsonlFile` sink.
//!
//! When [`crate::plugin_host::PluginHostOptions::recording`] is set to
//! [`crate::plugin_host::RecordingSink::JsonlFile`], the read-loop and
//! the writer-mutex tap every frame and append a JSON line per frame
//! to the file:
//!
//! ```text
//! {"ts":1714316451.123,"plugin":"echo-llm","dir":"h2p",
//!  "msgid":1,"method":"meta.handshake","frame":"<base64>"}
//! ```
//!
//! Direction codes are `h2p` (host → plugin) and `p2h` (plugin → host).
//! `method` and `msgid` are decoded from the frame body for indexing
//! convenience but are also redundantly encoded inside `frame` (base64)
//! for lossless replay.
//!
//! Recording is best-effort. IO errors are logged via `tracing::warn!`
//! and do not propagate — the runtime continues even if the file is
//! full or permission-denied. Likewise, failure to open the file at
//! plugin-load time disables recording for that plugin (the host emits
//! a `tracing::warn!` and otherwise behaves as if recording were `None`).
//!
//! See spec §7.8 (recording) and §9.1 (debug tier — protocol recording).

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use tau_plugin_protocol::Frame;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Direction of a recorded frame.
///
/// `pub` (rather than `pub(crate)`) so the test-only `__internals`
/// re-exports can surface it; production code only sees the JSONL
/// `dir` string field, never the typed enum.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    /// Host → plugin (request, host-side notification).
    HostToPlugin,
    /// Plugin → host (response, plugin-side notification).
    PluginToHost,
}

impl Direction {
    fn as_str(&self) -> &'static str {
        match self {
            Direction::HostToPlugin => "h2p",
            Direction::PluginToHost => "p2h",
        }
    }
}

/// A recording target. Holds an opened file handle behind a mutex so
/// concurrent writes from the read- and write-side tap points serialize
/// cleanly and never interleave a half-written line.
///
/// `pub` (rather than `pub(crate)`) because the test-only
/// `__internals` re-exports under [`crate::plugin_host::__internals`]
/// need to surface the type to integration tests; production code
/// reaches the recorder only via [`crate::plugin_host::PluginHostOptions::recording`].
pub struct Recorder {
    plugin_name: String,
    file: Mutex<tokio::fs::File>,
}

impl Recorder {
    /// Open a `JsonlFile` sink for the named plugin. Creates the parent
    /// directory if necessary; opens the file in append mode (creating
    /// it if missing). Failure to create the parent directory is
    /// silently tolerated — only the actual file open is fallible from
    /// the caller's perspective.
    pub async fn open_jsonl(plugin_name: &str, path: &Path) -> Result<Self, std::io::Error> {
        if let Some(parent) = path.parent() {
            // Ignore: the open below will surface the real error if the
            // parent really is missing and uncreatable.
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Recorder {
            plugin_name: plugin_name.to_string(),
            file: Mutex::new(file),
        })
    }

    /// Record a frame. Best-effort: write errors are logged at WARN and
    /// the line is dropped.
    ///
    /// `frame_bytes` is the raw MessagePack frame body (post-framing
    /// decode); the function additionally decodes a [`Frame`] to extract
    /// `msgid` + `method` for indexing convenience. Decoding failures
    /// are tolerated: the line is still written with `null` for those
    /// fields and the (lossless) base64-encoded frame body intact.
    pub async fn record(&self, dir: Direction, frame_bytes: &[u8]) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let (msgid, method) = decode_frame_metadata(frame_bytes);
        let encoded = B64.encode(frame_bytes);
        let line = serde_json::json!({
            "ts": ts,
            "plugin": self.plugin_name,
            "dir": dir.as_str(),
            "msgid": msgid,
            "method": method,
            "frame": encoded,
        });
        let mut buf = line.to_string();
        buf.push('\n');
        let mut file = self.file.lock().await;
        if let Err(e) = file.write_all(buf.as_bytes()).await {
            tracing::warn!(
                target: "tau_runtime::plugin_host::recording",
                plugin = self.plugin_name.as_str(),
                err = %e,
                "recording write failed; dropping frame"
            );
        }
    }

    /// Flush the underlying file, draining any tokio-side buffering so
    /// downstream readers (e.g. the integration tests reading the
    /// JSONL log via the blocking [`std::fs::read_to_string`]) observe
    /// every recorded line. Call sites should not rely on this for
    /// production correctness — it's a best-effort drain primarily
    /// intended for tests and for `tau --record-protocol` to call on
    /// runtime drop (Task 20).
    ///
    /// Best-effort: errors are logged at WARN, like [`Recorder::record`].
    pub async fn flush(&self) {
        let mut file = self.file.lock().await;
        if let Err(e) = file.flush().await {
            tracing::warn!(
                target: "tau_runtime::plugin_host::recording",
                plugin = self.plugin_name.as_str(),
                err = %e,
                "recording flush failed"
            );
        }
    }
}

/// Best-effort decode of the frame body to extract `msgid` and `method`
/// for the JSONL `msgid` / `method` indexing fields. Decode failures
/// (or future [`Frame`] variants — the type is `#[non_exhaustive]`)
/// map to `(None, None)` — the raw base64-encoded frame is still
/// recorded losslessly under the `frame` field.
fn decode_frame_metadata(frame_bytes: &[u8]) -> (Option<u32>, Option<String>) {
    match Frame::decode(frame_bytes) {
        Ok(Frame::Request { id, method, .. }) => (Some(id), Some(method)),
        Ok(Frame::Response { id, .. }) => (Some(id), None),
        Ok(Frame::Notification { method, .. }) => (None, Some(method)),
        // `Frame` is `#[non_exhaustive]`; a future variant decodes to
        // an indexer-less line rather than panicking.
        Ok(_) => (None, None),
        Err(_) => (None, None),
    }
}

/// Shared recorder handle. `Arc<Recorder>` clones cheaply; tap points
/// share one open file via the inner [`Mutex`]. See [`Recorder`] for
/// why this is `pub` rather than `pub(crate)`.
pub type RecorderHandle = Arc<Recorder>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_jsonl_creates_file_in_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rec.jsonl");
        let _r = Recorder::open_jsonl("plug", &path).await.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn open_jsonl_creates_missing_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("under").join("rec.jsonl");
        let _r = Recorder::open_jsonl("plug", &path).await.unwrap();
        assert!(path.exists());
    }

    /// Force the recorder's underlying file to flush so a subsequent
    /// synchronous `std::fs::read_to_string` observes the bytes. Async
    /// `tokio::fs::File` doesn't implicitly flush on drop the way the
    /// blocking `std::fs::File` does, so the unit tests below have to
    /// explicitly drain the buffer.
    async fn flush_recorder(recorder: &Recorder) {
        let mut file = recorder.file.lock().await;
        file.flush().await.expect("flush");
        file.sync_all().await.expect("sync_all");
    }

    #[tokio::test]
    async fn record_appends_line_with_metadata_for_known_request_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rec.jsonl");
        let recorder = Recorder::open_jsonl("plug", &path).await.unwrap();

        let frame = Frame::Request {
            id: 7,
            method: "llm.complete".to_string(),
            params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap(),
        };
        let bytes = frame.encode().unwrap();
        recorder.record(Direction::HostToPlugin, &bytes).await;
        flush_recorder(&recorder).await;

        let contents = std::fs::read_to_string(&path).unwrap();
        let line = contents.lines().next().expect("at least one line");
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["plugin"], "plug");
        assert_eq!(parsed["dir"], "h2p");
        assert_eq!(parsed["msgid"], 7);
        assert_eq!(parsed["method"], "llm.complete");
        assert!(parsed["frame"].is_string());
        assert!(parsed["ts"].is_number());
    }

    #[tokio::test]
    async fn record_tolerates_undecodable_frame_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rec.jsonl");
        let recorder = Recorder::open_jsonl("plug", &path).await.unwrap();
        // Garbage that won't parse as a Frame.
        recorder
            .record(Direction::PluginToHost, b"\xff\xff\xff\xff")
            .await;
        flush_recorder(&recorder).await;

        let contents = std::fs::read_to_string(&path).unwrap();
        let line = contents.lines().next().expect("at least one line");
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["dir"], "p2h");
        assert!(parsed["msgid"].is_null());
        assert!(parsed["method"].is_null());
        assert!(parsed["frame"].is_string());
    }
}
