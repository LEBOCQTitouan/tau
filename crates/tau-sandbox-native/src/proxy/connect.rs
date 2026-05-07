//! HTTP CONNECT request parsing + TLS ClientHello SNI peek.
//!
//! Pure parsing functions over byte slices. The async splice loop lives
//! in proxy::mod (T4). Tested without any tokio runtime.

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("malformed request: {0}")]
    Malformed(&'static str),
    #[error("non-CONNECT method")]
    NonConnect,
    #[error("missing port")]
    MissingPort,
}

/// Parse the first line of an HTTP request.
///
/// Expected: `CONNECT host:port HTTP/1.1\r\n`
///
/// Other forms (GET, POST, etc.) → `NonConnect`.
/// Missing port → `MissingPort`.
pub fn parse_connect_request(buf: &[u8]) -> Result<ConnectRequest, ParseError> {
    let line_end = buf
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .ok_or(ParseError::Malformed("no CRLF"))?;
    let line = std::str::from_utf8(&buf[..line_end])
        .map_err(|_| ParseError::Malformed("non-utf8"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(ParseError::Malformed("empty"))?;
    if method != "CONNECT" {
        return Err(ParseError::NonConnect);
    }
    let target = parts.next().ok_or(ParseError::Malformed("no target"))?;
    let (host, port) = target
        .rsplit_once(':')
        .ok_or(ParseError::MissingPort)?;
    let port: u16 = port.parse().map_err(|_| ParseError::MissingPort)?;
    Ok(ConnectRequest {
        host: host.to_string(),
        port,
    })
}

/// Extract the SNI extension value from a TLS ClientHello.
///
/// `buf` must contain the first ~512 bytes of the TLS connection (peeked,
/// not consumed). Returns `Some(server_name)` if SNI extension is present
/// and well-formed; `None` otherwise (proxy treats absent SNI as a hard
/// failure per spec Decision 7).
pub fn peek_sni(buf: &[u8]) -> Option<String> {
    // TLS record layer: type (1) + version (2) + length (2) = 5 bytes
    if buf.len() < 5 || buf[0] != 0x16 {
        return None; // not Handshake record
    }
    let record_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    if buf.len() < 5 + record_len {
        return None; // not enough bytes
    }
    // Handshake message: type (1) + length (3) = 4 bytes
    let hs = &buf[5..5 + record_len];
    if hs.is_empty() || hs[0] != 0x01 {
        return None; // not ClientHello
    }
    // Skip handshake header (4 bytes) + version (2) + random (32)
    let mut p: usize = 4 + 2 + 32;
    if hs.len() < p + 1 {
        return None;
    }
    // session_id_len (1) + session_id
    let session_id_len = hs[p] as usize;
    p += 1 + session_id_len;
    // cipher_suites_len (2)
    if hs.len() < p + 2 {
        return None;
    }
    let cipher_len = u16::from_be_bytes([hs[p], hs[p + 1]]) as usize;
    p += 2 + cipher_len;
    // compression_methods_len (1)
    if hs.len() < p + 1 {
        return None;
    }
    let comp_len = hs[p] as usize;
    p += 1 + comp_len;
    // extensions_len (2)
    if hs.len() < p + 2 {
        return None;
    }
    let ext_total_len = u16::from_be_bytes([hs[p], hs[p + 1]]) as usize;
    p += 2;
    let ext_end = p + ext_total_len;
    if hs.len() < ext_end {
        return None;
    }
    // Walk extensions looking for type 0x0000 (SNI)
    while p + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([hs[p], hs[p + 1]]);
        let ext_len = u16::from_be_bytes([hs[p + 2], hs[p + 3]]) as usize;
        let ext_data = p + 4;
        if ext_type == 0x0000 && ext_data + 5 <= ext_end {
            // SNI extension layout: server_name_list_len (2) + server_name_list
            // server_name_list[0]: name_type (1) + name_len (2) + name
            let name_type = hs[ext_data + 2];
            if name_type != 0x00 {
                return None; // not host_name
            }
            let name_len = u16::from_be_bytes([hs[ext_data + 3], hs[ext_data + 4]]) as usize;
            let name_start = ext_data + 5;
            if name_start + name_len > ext_end {
                return None;
            }
            return std::str::from_utf8(&hs[name_start..name_start + name_len])
                .ok()
                .map(str::to_string);
        }
        p = ext_data + ext_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_connect_well_formed() {
        let buf = b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n";
        let req = parse_connect_request(buf).expect("parse");
        assert_eq!(req.host, "api.anthropic.com");
        assert_eq!(req.port, 443);
    }

    #[test]
    fn reject_get_method() {
        let buf = b"GET / HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::NonConnect)
        ));
    }

    #[test]
    fn reject_post_method() {
        let buf = b"POST /endpoint HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::NonConnect)
        ));
    }

    #[test]
    fn reject_missing_port() {
        let buf = b"CONNECT api.anthropic.com HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::MissingPort)
        ));
    }

    #[test]
    fn reject_non_numeric_port() {
        let buf = b"CONNECT api.anthropic.com:notaport HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::MissingPort)
        ));
    }

    #[test]
    fn reject_no_crlf() {
        let buf = b"CONNECT api.anthropic.com:443 HTTP/1.1";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::Malformed(_))
        ));
    }

    #[test]
    fn peek_sni_empty_buffer_returns_none() {
        assert_eq!(peek_sni(&[]), None);
    }

    #[test]
    fn peek_sni_non_tls_returns_none() {
        assert_eq!(peek_sni(b"GET / HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn peek_sni_truncated_record_returns_none() {
        // Looks like TLS handshake but truncated
        let buf = &[0x16, 0x03, 0x01, 0x00, 0x10][..];
        assert_eq!(peek_sni(buf), None);
    }

    // Note: full SNI extraction with a real ClientHello byte stream is
    // tested via the proxy's integration tests (T8). The unit tests above
    // cover the negative paths.
}
