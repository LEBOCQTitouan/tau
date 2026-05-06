//! In-process HTTP cassette replayer for plugin integration tests.
//!
//! Loads YAML cassettes describing recorded request/response pairs
//! and serves them in-order from a `tokio::net::TcpListener`.
//! Captures the request body (and arbitrary other headers) into a
//! `Vec<RecordedRequest>` so tests can assert on what the plugin sent.
//!
//! Originated in `crates/tau-plugins/anthropic/tests/common/cassette.rs`;
//! lifted here as the rule-of-three refactor when ollama and openai
//! became consumers.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One recorded request/response pair from a cassette YAML file.
#[derive(Debug, Deserialize)]
pub struct CassetteEntry {
    /// The recorded request metadata (method, URI, headers, body).
    pub request: RecordedRequest,
    /// The response to replay when this entry is consumed.
    pub response: RecordedResponse,
}

/// A request captured (or loaded from a cassette) during replay.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct RecordedRequest {
    /// HTTP method (e.g. `"POST"`).
    pub method: String,
    /// Request URI path (e.g. `"/v1/messages"`).
    pub uri: String,
    /// HTTP headers, keyed by lowercase header name.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Raw request body bytes decoded as UTF-8.
    #[serde(default)]
    pub body: String,
}

/// A response to replay for a cassette entry.
#[derive(Debug, Deserialize, Clone)]
pub struct RecordedResponse {
    /// HTTP status code (e.g. `200`).
    pub status: u16,
    /// Response headers to include.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Response body to send.
    #[serde(default)]
    pub body: String,
}

/// An in-process HTTP server that replays a cassette.
pub struct CassetteServer {
    base_url: String,
    received: Arc<Mutex<Vec<RecordedRequest>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl CassetteServer {
    /// Returns the base URL of the cassette server (e.g. `"http://127.0.0.1:PORT"`).
    pub fn uri(&self) -> &str {
        &self.base_url
    }

    /// Returns the list of requests received by the server so far.
    pub fn received_requests(&self) -> Vec<RecordedRequest> {
        self.received.lock().unwrap().clone()
    }
}

/// Load a cassette YAML and start a server.
pub async fn replay(path: impl AsRef<Path>) -> CassetteServer {
    let yaml = std::fs::read_to_string(path.as_ref())
        .unwrap_or_else(|e| panic!("failed to read cassette {}: {e}", path.as_ref().display()));
    let entries: Vec<CassetteEntry> = serde_yaml::from_str(&yaml)
        .unwrap_or_else(|e| panic!("failed to parse cassette {}: {e}", path.as_ref().display()));
    start_server(entries).await
}

async fn start_server(entries: Vec<CassetteEntry>) -> CassetteServer {
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let handle = tokio::spawn(async move {
        for entry in entries {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => return,
            };
            let received_clone = received_clone.clone();
            tokio::spawn(async move {
                handle_connection(stream, entry, received_clone).await;
            });
        }
        // After exhausting the cassette, accept any further connections
        // and respond with 500 to surface "you ran out of cassette
        // entries" failures rather than hanging forever.
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\n\
                      content-length: 28\r\n\
                      connection: close\r\n\
                      \r\n\
                      cassette ran out of entries\n",
                )
                .await;
            let _ = sock.flush().await;
        }
    });

    CassetteServer {
        base_url: format!("http://127.0.0.1:{port}"),
        received,
        _handle: handle,
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    entry: CassetteEntry,
    received: Arc<Mutex<Vec<RecordedRequest>>>,
) {
    // Read up to ~16 KiB of request bytes — enough for typical
    // CompletionRequest bodies. For larger bodies, increase or chunk.
    let mut buf = vec![0u8; 16 * 1024];
    let mut total_read = 0;
    // Read until we have headers + body. Crude: keep reading until we
    // see "\r\n\r\n" and then read content-length more bytes (if any).
    while total_read < buf.len() {
        match stream.read(&mut buf[total_read..]).await {
            Ok(0) => break,
            Ok(n) => {
                total_read += n;
                let chunk = &buf[..total_read];
                if let Some(headers_end) = find_subsequence(chunk, b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&chunk[..headers_end]).unwrap_or("");
                    let content_length = parse_content_length(headers);
                    let body_so_far = total_read - (headers_end + 4);
                    if body_so_far >= content_length {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }

    let request_text = String::from_utf8_lossy(&buf[..total_read]).to_string();
    let recorded = parse_request(&request_text);
    received.lock().unwrap().push(recorded);

    let response = build_response(&entry.response);
    let _ = stream.write_all(&response).await;
    let _ = stream.flush().await;
}

fn parse_request(text: &str) -> RecordedRequest {
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    RecordedRequest {
        method: parts.first().copied().unwrap_or("").to_string(),
        uri: parts.get(1).copied().unwrap_or("").to_string(),
        headers,
        body: body.to_string(),
    }
}

fn parse_content_length(headers: &str) -> usize {
    for line in headers.split("\r\n") {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    return n;
                }
            }
        }
    }
    0
}

fn build_response(resp: &RecordedResponse) -> Vec<u8> {
    let status_text = status_text(resp.status);
    let mut out = format!("HTTP/1.1 {} {}\r\n", resp.status, status_text);
    let mut has_content_length = false;
    let mut has_connection = false;
    for (k, v) in &resp.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
        if k.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if k.eq_ignore_ascii_case("connection") {
            has_connection = true;
        }
    }
    if !has_content_length {
        out.push_str(&format!("content-length: {}\r\n", resp.body.len()));
    }
    if !has_connection {
        out.push_str("connection: close\r\n");
    }
    out.push_str("\r\n");

    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(resp.body.as_bytes());
    bytes
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[tokio::test]
    async fn serves_recorded_response_in_order() {
        // Inline cassette: 1 entry.
        let yaml = r#"
- request:
    method: POST
    uri: /v1/messages
  response:
    status: 200
    headers:
      content-type: application/json
    body: |-
      {"hello":"world"}
"#;
        let entries: Vec<CassetteEntry> = serde_yaml::from_str(yaml).unwrap();
        let server = start_server(entries).await;

        let resp = reqwest::Client::new()
            .post(format!("{}/v1/messages", server.uri()))
            .body(r#"{"x":1}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert_eq!(body, r#"{"hello":"world"}"#);

        let received = server.received_requests();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].method, "POST");
        assert_eq!(received[0].uri, "/v1/messages");
        assert_eq!(received[0].body, r#"{"x":1}"#);
    }

    #[tokio::test]
    async fn returns_500_after_cassette_exhausted() {
        let yaml = r#"
- request:
    method: GET
    uri: /
  response:
    status: 200
    body: |-
      first
"#;
        let entries: Vec<CassetteEntry> = serde_yaml::from_str(yaml).unwrap();
        let server = start_server(entries).await;
        let client = reqwest::Client::new();

        // First call serves the cassette.
        let r1 = client.get(server.uri()).send().await.unwrap();
        assert_eq!(r1.status(), 200);

        // Second call — cassette exhausted, server returns 500.
        let r2 = client.get(server.uri()).send().await.unwrap();
        assert_eq!(r2.status(), 500);
    }
}
