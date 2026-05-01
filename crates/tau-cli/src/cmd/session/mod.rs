//! `tau session` subcommand group — list, show, delete, export.

pub mod list;
// show, delete, export added in Tasks 8-10.

use crate::cli::{SessionAction, SessionArgs};
use crate::output::Output;

/// Dispatch a `tau session <action>` subcommand.
pub async fn run(args: &SessionArgs, output: &mut Output) -> anyhow::Result<()> {
    match &args.action {
        SessionAction::List(list_args) => list::run(list_args, output).await,
    }
}
