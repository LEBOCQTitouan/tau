//! `tau-sandbox-proxy` — userspace HTTP-CONNECT proxy for tau sandboxed plugins.
//!
//! Shared by both the native (Linux landlock/seccomp) and container
//! (docker/podman) sandbox adapters. Extracted from `tau-sandbox-native`
//! because the proxy logic is purely tokio-based and cross-platform, while
//! `tau-sandbox-native` itself is Linux-specific.
//!
//! Architecture: a tokio task in tau's parent address space accepts
//! Unix-socket connections from the per-plugin `tau-net-bridge` binary.
//! Each connection arrives carrying an HTTP `CONNECT host:port`
//! request; the proxy validates the host against the plan's allow-list,
//! peeks the TLS ClientHello to verify SNI matches, then opens a TCP
//! connection to the remote and splices bytes both ways.
//!
//! Pass-through mode only — proxy does NOT terminate TLS. Plugin's TLS
//! handshake goes end-to-end with the real remote server.

mod connect;
mod validate;

pub use connect::{parse_connect_request, peek_sni, ConnectRequest};
pub use validate::{validate_hosts, ValidationError};

use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::task::JoinHandle;

/// Handle to a running proxy task. Drop aborts the task and unlinks the
/// temp Unix socket file.
#[non_exhaustive]
pub struct ProxyHandle {
    sock_path: PathBuf,
    task: JoinHandle<()>,
}

impl ProxyHandle {
    /// Returns the path to the Unix socket the proxy is listening on.
    pub fn sock_path(&self) -> &Path {
        &self.sock_path
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

/// Spawn a tokio task that listens for HTTP CONNECT requests on a
/// temp Unix socket file. Returns a `ProxyHandle` whose Drop cleans up.
///
/// Caller is responsible for granting the child access to the returned
/// socket path (e.g. via landlock rules for native, bind-mount for container)
/// so the bridge inside the sandbox can dial it.
pub fn spawn_proxy(allowed_hosts: Vec<String>) -> std::io::Result<ProxyHandle> {
    let sock_path = make_temp_sock_path()?;
    let listener = UnixListener::bind(&sock_path)?;
    let task = tokio::spawn(accept_loop(listener, allowed_hosts));
    Ok(ProxyHandle { sock_path, task })
}

fn make_temp_sock_path() -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!("tau-proxy-{}-{}.sock", std::process::id(), n);
    p.push(suffix);
    // Ensure the file does not exist (clean state from a prior aborted run)
    let _ = std::fs::remove_file(&p);
    Ok(p)
}

async fn accept_loop(listener: UnixListener, allowed_hosts: Vec<String>) {
    loop {
        match listener.accept().await {
            Ok((mut conn, _)) => {
                let hosts = allowed_hosts.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(&mut conn, &hosts).await {
                        tracing::warn!(error = %e, "proxy connection failed");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "proxy accept failed");
                return;
            }
        }
    }
}

async fn handle_connection(
    plugin_sock: &mut UnixStream,
    allowed_hosts: &[String],
) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = plugin_sock.read(&mut buf).await?;
    let req = match parse_connect_request(&buf[..n]) {
        Ok(r) => r,
        Err(_) => {
            plugin_sock
                .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
                .await?;
            return Ok(());
        }
    };
    if !allowed_hosts.iter().any(|h| h == &req.host) {
        plugin_sock
            .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
            .await?;
        return Ok(());
    }
    if req.port != 443 {
        plugin_sock
            .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            .await?;
        return Ok(());
    }
    let mut remote = TcpStream::connect((req.host.as_str(), req.port)).await?;
    plugin_sock
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    // Peek the first chunk — should be TLS ClientHello with SNI matching CONNECT host
    let mut peek_buf = [0u8; 1024];
    let n = plugin_sock.read(&mut peek_buf).await?;
    if let Some(sni) = peek_sni(&peek_buf[..n]) {
        if sni != req.host {
            return Err(std::io::Error::other(format!(
                "SNI mismatch: CONNECT={} SNI={}",
                req.host, sni
            )));
        }
    } else {
        return Err(std::io::Error::other("missing SNI in TLS ClientHello"));
    }
    // Forward the peeked bytes onward, then splice
    remote.write_all(&peek_buf[..n]).await?;
    let (mut pr, mut pw) = plugin_sock.split();
    let (mut rr, mut rw) = remote.split();
    let _ = tokio::try_join!(
        tokio::io::copy(&mut pr, &mut rw),
        tokio::io::copy(&mut rr, &mut pw),
    );
    Ok(())
}

#[cfg(test)]
mod proxy_lifecycle_tests {
    use super::*;
    use tokio::io::AsyncReadExt as _;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test]
    async fn proxy_handle_drop_unlinks_socket_file() {
        let h = spawn_proxy(vec!["example.com".to_string()]).expect("spawn");
        let path = h.sock_path().to_path_buf();
        assert!(path.exists(), "socket file should exist after spawn");
        drop(h);
        // Give the OS a beat to unlink
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!path.exists(), "socket file should be unlinked on drop");
    }

    #[tokio::test]
    async fn forbidden_host_returns_403() {
        let h = spawn_proxy(vec!["allowed.example.com".to_string()]).expect("spawn");
        let mut conn = UnixStream::connect(h.sock_path()).await.expect("connect");
        conn.write_all(b"CONNECT denied.example.com:443 HTTP/1.1\r\n\r\n")
            .await
            .expect("write");
        let mut resp = [0u8; 256];
        let n = conn.read(&mut resp).await.expect("read");
        let s = std::str::from_utf8(&resp[..n]).expect("utf8");
        assert!(s.starts_with("HTTP/1.1 403"), "got: {s}");
    }

    #[tokio::test]
    async fn malformed_request_returns_400() {
        let h = spawn_proxy(vec!["example.com".to_string()]).expect("spawn");
        let mut conn = UnixStream::connect(h.sock_path()).await.expect("connect");
        conn.write_all(b"GET / HTTP/1.1\r\n\r\n")
            .await
            .expect("write");
        let mut resp = [0u8; 256];
        let n = conn.read(&mut resp).await.expect("read");
        let s = std::str::from_utf8(&resp[..n]).expect("utf8");
        assert!(s.starts_with("HTTP/1.1 400"), "got: {s}");
    }

    #[tokio::test]
    async fn non_443_port_returns_400() {
        let h = spawn_proxy(vec!["example.com".to_string()]).expect("spawn");
        let mut conn = UnixStream::connect(h.sock_path()).await.expect("connect");
        conn.write_all(b"CONNECT example.com:80 HTTP/1.1\r\n\r\n")
            .await
            .expect("write");
        let mut resp = [0u8; 256];
        let n = conn.read(&mut resp).await.expect("read");
        let s = std::str::from_utf8(&resp[..n]).expect("utf8");
        assert!(s.starts_with("HTTP/1.1 400"), "got: {s}");
    }
}
