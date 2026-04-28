#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! tau command-line entry point.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tau_cli::run_main().await
}
