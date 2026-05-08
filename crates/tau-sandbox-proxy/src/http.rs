//! Plain-HTTP request parsing for the proxy's HTTP forwarding path.
//!
//! Pure parsing functions over byte slices; the async splice loop lives
//! in lib.rs. Tested without any tokio runtime.

#[derive(Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, etc.).
    pub method: String,
    /// Origin-form path-and-query (e.g. `/api/chat?x=1`).
    pub path_and_query: String,
    /// HTTP version token (e.g. `HTTP/1.1`).
    pub version: String,
    /// Host (without port).
    pub host: String,
    /// Port (defaults to 80 if absent).
    pub port: u16,
    /// Byte offset where the request line ends (just past the `\r\n` /
    /// `\n`). The remainder of the buffer is headers + body to forward
    /// verbatim.
    pub line_end: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpParseError {
    #[error("malformed request: {0}")]
    Malformed(&'static str),
    #[error("not an HTTP method")]
    NotHttp,
    #[error("no host (neither absolute-URI nor Host header)")]
    NoHost,
}

const HTTP_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "TRACE",
];

/// Parse the first line of a plain HTTP request and resolve the host.
///
/// Accepts both:
///   1. Absolute-URI form (proxy form): `GET http://host:port/path HTTP/1.1`
///   2. Origin-form with Host header: `GET /path HTTP/1.1\r\nHost: host:port\r\n`
///
/// In practice reqwest sends form (1) when HTTP_PROXY is set; we support
/// (2) for robustness against alternate clients.
pub fn parse_http_request(buf: &[u8]) -> Result<HttpRequest, HttpParseError> {
    let line_end_idx = buf
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(HttpParseError::Malformed("no newline"))?;
    let line_end = line_end_idx + 1; // include the \n
    let line_bytes = &buf[..line_end_idx];
    // Strip trailing \r if present.
    let line_bytes = if line_bytes.last() == Some(&b'\r') {
        &line_bytes[..line_bytes.len() - 1]
    } else {
        line_bytes
    };
    let line = std::str::from_utf8(line_bytes)
        .map_err(|_| HttpParseError::Malformed("non-utf8 request line"))?;

    // Split: METHOD URI VERSION
    let mut parts = line.splitn(3, ' ');
    let method = parts.next().ok_or(HttpParseError::Malformed("empty"))?;
    let uri = parts.next().ok_or(HttpParseError::Malformed("no URI"))?;
    let version = parts
        .next()
        .ok_or(HttpParseError::Malformed("no version"))?;

    if !HTTP_METHODS.contains(&method) {
        return Err(HttpParseError::NotHttp);
    }
    if !version.starts_with("HTTP/") {
        return Err(HttpParseError::Malformed("not HTTP/x.y"));
    }

    // Try absolute-URI form first.
    if let Some(rest) = uri.strip_prefix("http://") {
        // host[:port]/path-and-query
        let (host_port, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        let (host, port) = parse_host_port(host_port, 80)?;
        return Ok(HttpRequest {
            method: method.to_string(),
            path_and_query: path.to_string(),
            version: version.to_string(),
            host,
            port,
            line_end,
        });
    }

    // Origin-form: parse Host header from subsequent lines.
    let path_and_query = uri.to_string();
    let host_value = find_host_header(&buf[line_end..])?;
    let (host, port) = parse_host_port(&host_value, 80)?;
    Ok(HttpRequest {
        method: method.to_string(),
        path_and_query,
        version: version.to_string(),
        host,
        port,
        line_end,
    })
}

/// Scan headers (terminated by an empty line) for `Host:`. Case-insensitive.
fn find_host_header(buf: &[u8]) -> Result<String, HttpParseError> {
    let mut i = 0;
    while i < buf.len() {
        // Find end of this header line.
        let line_end = match buf[i..].iter().position(|&b| b == b'\n') {
            Some(idx) => i + idx,
            None => return Err(HttpParseError::NoHost),
        };
        let mut line = &buf[i..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if line.is_empty() {
            // End of headers.
            return Err(HttpParseError::NoHost);
        }
        let line_str = std::str::from_utf8(line)
            .map_err(|_| HttpParseError::Malformed("non-utf8 header"))?;
        if let Some((name, value)) = line_str.split_once(':') {
            if name.eq_ignore_ascii_case("host") {
                return Ok(value.trim().to_string());
            }
        }
        i = line_end + 1;
    }
    Err(HttpParseError::NoHost)
}

fn parse_host_port(s: &str, default_port: u16) -> Result<(String, u16), HttpParseError> {
    if let Some((h, p)) = s.rsplit_once(':') {
        // IPv6 literals like `[::1]:port` need special handling — but for
        // proxy use the cassette test cases use IPv4 + hostnames, so plain
        // rsplit_once works. If the host part contains '[' we treat the
        // whole thing as a host with no port.
        if h.starts_with('[') && !h.ends_with(']') {
            return Ok((s.to_string(), default_port));
        }
        let port: u16 = p
            .parse()
            .map_err(|_| HttpParseError::Malformed("invalid port"))?;
        Ok((h.to_string(), port))
    } else {
        Ok((s.to_string(), default_port))
    }
}

/// Build the rewritten origin-form request line + CRLF.
///
/// Used by the splice loop to substitute the absolute-URI line with an
/// origin-form line before forwarding to the destination server.
pub fn rewrite_request_line(req: &HttpRequest) -> String {
    format!("{} {} {}\r\n", req.method, req.path_and_query, req.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_uri_get() {
        let buf = b"GET http://example.com/foo HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let req = parse_http_request(buf).expect("parse");
        assert_eq!(req.method, "GET");
        assert_eq!(req.host, "example.com");
        assert_eq!(req.port, 80);
        assert_eq!(req.path_and_query, "/foo");
        assert_eq!(req.version, "HTTP/1.1");
    }

    #[test]
    fn absolute_uri_with_port() {
        let buf = b"POST http://127.0.0.1:33549/api/chat HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        let req = parse_http_request(buf).expect("parse");
        assert_eq!(req.host, "127.0.0.1");
        assert_eq!(req.port, 33549);
        assert_eq!(req.path_and_query, "/api/chat");
    }

    #[test]
    fn absolute_uri_no_path() {
        let buf = b"GET http://example.com HTTP/1.1\r\n\r\n";
        let req = parse_http_request(buf).expect("parse");
        assert_eq!(req.path_and_query, "/");
    }

    #[test]
    fn origin_form_with_host_header() {
        let buf = b"GET /foo HTTP/1.1\r\nHost: example.com:8080\r\n\r\n";
        let req = parse_http_request(buf).expect("parse");
        assert_eq!(req.host, "example.com");
        assert_eq!(req.port, 8080);
        assert_eq!(req.path_and_query, "/foo");
    }

    #[test]
    fn origin_form_case_insensitive_host_header() {
        let buf = b"GET /foo HTTP/1.1\r\nhOsT: example.com\r\n\r\n";
        let req = parse_http_request(buf).expect("parse");
        assert_eq!(req.host, "example.com");
    }

    #[test]
    fn origin_form_no_host_header_errors() {
        let buf = b"GET /foo HTTP/1.1\r\nFoo: bar\r\n\r\n";
        assert!(matches!(
            parse_http_request(buf),
            Err(HttpParseError::NoHost)
        ));
    }

    #[test]
    fn reject_connect() {
        // CONNECT is not in HTTP_METHODS; falls into NotHttp.
        let buf = b"CONNECT example.com:443 HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_http_request(buf),
            Err(HttpParseError::NotHttp)
        ));
    }

    #[test]
    fn reject_garbage() {
        let buf = b"NOTAMETHOD / HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_http_request(buf),
            Err(HttpParseError::NotHttp)
        ));
    }

    #[test]
    fn reject_no_newline() {
        let buf = b"GET / HTTP/1.1";
        assert!(matches!(
            parse_http_request(buf),
            Err(HttpParseError::Malformed(_))
        ));
    }

    #[test]
    fn rewrite_to_origin_form() {
        let req = HttpRequest {
            method: "GET".into(),
            path_and_query: "/api/x".into(),
            version: "HTTP/1.1".into(),
            host: "example.com".into(),
            port: 80,
            line_end: 0,
        };
        assert_eq!(rewrite_request_line(&req), "GET /api/x HTTP/1.1\r\n");
    }
}
