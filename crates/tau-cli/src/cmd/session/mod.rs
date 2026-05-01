//! `tau session` subcommand group — list, show, delete, export.

pub mod delete;
pub mod export;
pub mod list;
pub mod show;

use crate::cli::{SessionAction, SessionArgs};
use crate::output::Output;

/// Dispatch a `tau session <action>` subcommand.
pub async fn run(args: &SessionArgs, output: &mut Output) -> anyhow::Result<()> {
    match &args.action {
        SessionAction::List(list_args) => list::run(list_args, output).await,
        SessionAction::Show(show_args) => show::run(show_args, output).await,
        SessionAction::Delete(delete_args) => delete::run(delete_args, output).await,
        SessionAction::Export(export_args) => export::run(export_args, output).await,
    }
}
