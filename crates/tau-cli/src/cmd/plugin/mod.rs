//! `tau plugin {describe, run, protocol decode}` subcommands.
//!
//! Spec §9 (debug tier): these handlers expose the plugin protocol to
//! human operators for inspection, REPL-style probing, and post-hoc
//! analysis of recorded sessions.
//!
//! - [`describe`] resolves a plugin from the lockfile, drives one
//!   handshake, prints the advertised metadata + per-method schemas,
//!   then shuts the plugin down cleanly.
//! - [`run`] spawns an arbitrary plugin binary and drives either an
//!   interactive REPL (`<method> <json-args>` per line) or a scripted
//!   JSONL input file. Standalone — no agent, no kernel.
//! - [`protocol_decode`] reads a JSONL recording produced by
//!   `--record-protocol` and emits a human-readable transcript with
//!   filter/time-range/JSON projection options.

pub mod describe;
pub mod protocol_decode;
pub mod run;

use std::path::PathBuf;

use crate::cli::{PluginAction, PluginProtocolAction};
use crate::output::Output;

/// Dispatch a `tau plugin <action>` subcommand.
///
/// `record_protocol` is the global `--record-protocol` flag; it only
/// affects [`describe`] (which spawns a plugin) — neither
/// [`protocol_decode`] (offline) nor [`run`] (which manages its own
/// process IO directly) consult it.
pub async fn dispatch(
    action: PluginAction,
    record_protocol: Option<PathBuf>,
    output: &mut Output,
) -> anyhow::Result<()> {
    match action {
        PluginAction::Describe(args) => describe::run(&args, record_protocol, output).await,
        PluginAction::Run(args) => run::run(&args, output).await,
        PluginAction::Protocol { action } => match action {
            PluginProtocolAction::Decode(args) => protocol_decode::run(&args, output).await,
        },
    }
}
