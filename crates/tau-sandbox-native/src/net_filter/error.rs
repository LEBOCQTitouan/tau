//! Internal error type for the per-host network filter (sub-project F).
//!
//! `NetFilterError` is rich (carries `std::io::Error` sources, typed variants).
//! At the orchestrator boundary in `mod.rs`, it converts to
//! `tau_ports::SandboxError::NetFilter { message: error.to_string() }`.

use std::io;

/// Error from per-host network filter setup or apply.
///
/// `#[non_exhaustive]` so additive variants are non-breaking.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum NetFilterError {
    /// One or more prerequisites (binaries, kernel features) missing.
    #[error("net-filter prerequisites missing: {missing:?}")]
    PrerequisitesUnavailable { missing: Vec<&'static str> },

    /// A specific binary required for the filter is missing from PATH.
    // Consumed at runtime via probe paths; flagged dead by static analysis
    // without integration-tests feature active.
    #[allow(dead_code)]
    #[error("nftables binary missing: {name}")]
    MissingBinary { name: &'static str },

    /// CAP_NET_ADMIN cannot be acquired in an unprivileged user namespace.
    // Consumed at runtime via probe paths; flagged dead by static analysis
    // without integration-tests feature active.
    #[allow(dead_code)]
    #[error("CAP_NET_ADMIN unavailable in user namespace")]
    CapNetAdminUnavailable,

    /// Wildcard hosts (`*` or `*.x.y`) are forbidden under per-host filtering.
    #[error("wildcard host '{host}' is not supported under strict-tier filtering; declare specific hosts")]
    WildcardForbidden { host: String },

    /// IP literals other than `127.0.0.1` are not supported.
    #[error("IP literal '{host}' is not supported in hosts list (use hostnames); '127.0.0.1' is the only allowed literal")]
    IpLiteralNotSupported { host: String },

    /// DNS lookup for a specific host failed (timeout, NXDOMAIN, network error).
    #[error("DNS resolution for '{host}' failed: {source}")]
    DnsResolutionFailed {
        host: String,
        #[source]
        source: io::Error,
    },

    /// DNS resolution succeeded but returned zero records.
    #[error("no DNS records for '{host}'")]
    DnsNoRecords { host: String },

    /// veth/netns setup failed (likely an `ip` shell-out non-zero).
    #[error("netns setup failed: {context}: {source}")]
    NetnsSetup {
        context: &'static str,
        #[source]
        source: io::Error,
    },

    /// `nft -f -` returned non-zero; stderr is the diagnostic.
    #[error("nft ruleset apply failed: {stderr}")]
    NftApplyFailed { stderr: String },
}
