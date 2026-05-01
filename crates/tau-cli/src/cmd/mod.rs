//! Subcommand handlers. Each subcommand has its own submodule with
//! a `pub async fn run(args: &<SubArgs>) -> anyhow::Result<()>`
//! entry point.
//!
//! Tasks 10-14 implement each handler. At Task 4 they are stubs
//! returning an "unimplemented" error.

pub mod chat;
pub mod init;
pub mod install;
pub mod list;
pub mod plugin;
pub(crate) mod plugin_loader;
pub mod resolve;
pub(crate) mod resolve_helpers;
pub mod run;
pub mod uninstall;
pub mod update;
pub mod verify;
