//! `fs-read-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The full implementation lands in Task 9. For Task 1 this stub
//! exists only so that `cargo build` succeeds.

fn main() {
    eprintln!("fs-read-plugin: not yet wired (placeholder; see Task 9)");
    std::process::exit(1);
}
