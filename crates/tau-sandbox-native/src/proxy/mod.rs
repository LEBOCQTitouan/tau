//! Sandbox proxy — userspace HTTP-CONNECT proxy replacing F's veth+nft
//! per-host filter (sub-project H, ADR-0020).
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

mod validate;
mod connect;

pub(crate) use validate::{validate_hosts, ValidationError};
pub(crate) use connect::{ConnectRequest, parse_connect_request, peek_sni};
