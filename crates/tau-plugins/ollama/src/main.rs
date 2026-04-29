//! `ollama-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The full implementation (handshake + dispatch loop) lands in Task 8.
//! For Task 1, this stub exists only so that `cargo build` succeeds.

fn main() {
    eprintln!("ollama-plugin: not yet wired (placeholder; see Task 8)");
    std::process::exit(1);
}
