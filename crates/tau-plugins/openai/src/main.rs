//! `openai-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The full implementation lands in Task 11. For Task 1, this stub
//! exists only so that `cargo build` succeeds.

fn main() {
    eprintln!("openai-plugin: not yet wired (placeholder; see Task 11)");
    std::process::exit(1);
}
