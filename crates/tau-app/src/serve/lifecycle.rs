//! Process lifecycle: startup, signals, graceful shutdown.

use super::cancel::CancelRegistry;
use super::dispatch::Dispatcher;
use super::framing;
use super::handshake::HandshakeState;
use super::options::ServeOptions;
use super::project::Project;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use tracing::{info, warn};

/// Main serve entry point. Builds runtime, spawns tasks, blocks until shutdown.
pub async fn run(opts: ServeOptions) -> Result<()> {
    super::tracing_init::install();

    info!(project = %opts.project_path.display(), "serve starting");

    let project = Arc::new(
        Project::load(&opts.project_path)
            .await
            .context("load project")?,
    );

    let runtime = build_runtime(&project)
        .await
        .context("build runtime")?;

    let (in_tx, in_rx) = mpsc::channel(64);
    let (out_tx, out_rx) = mpsc::channel(256);

    // Linux: set PDEATHSIG so we die when parent dies.
    #[cfg(target_os = "linux")]
    set_pdeathsig();

    let cancel_reg = CancelRegistry::default();
    let dispatcher = Dispatcher {
        project: project.clone(),
        runtime: Arc::new(runtime),
        handshake: HandshakeState::default(),
        cancel_reg: cancel_reg.clone(),
        max_concurrent: opts.max_concurrent,
        out_tx: out_tx.clone(),
    };

    let local_set = LocalSet::new();

    // Reader and writer tasks are Send-friendly — spawn on multi-thread side.
    let reader_handle = tokio::spawn(framing::reader_task(in_tx));
    let writer_handle = tokio::spawn(framing::writer_task(out_rx));

    if opts.ready_on_stderr {
        eprintln!("tau-serve ready");
    }

    // Run dispatcher loop on the LocalSet so per-request tasks have a
    // current_thread executor available for non-Send streams.
    // spawn_local (used inside Dispatcher::spawn_run) works within any
    // active LocalSet on the current thread — no &LocalSet borrow needed.
    let dispatch_result = local_set
        .run_until(async move {
            let shutdown_signal = wait_for_shutdown_signal();
            tokio::select! {
                r = dispatcher.run(in_rx) => r,
                _ = shutdown_signal => Ok(()),
            }
        })
        .await;

    // Graceful drain.
    cancel_reg.cancel_all();
    let grace_result = tokio::time::timeout(opts.shutdown_grace, async {
        let _ = reader_handle.await;
    })
    .await;
    if grace_result.is_err() {
        warn!(grace = ?opts.shutdown_grace, "shutdown grace expired");
    }
    drop(out_tx);
    let _ = writer_handle.await;

    info!("serve shutdown complete");
    dispatch_result?;
    Ok(())
}

/// Build the `Runtime` from a loaded `Project`.
///
/// Closed by Task 13 (RuntimeBuilder::from_project refactor).
async fn build_runtime(_project: &Project) -> Result<tau_runtime::Runtime> {
    todo!("see Task 13 — RuntimeBuilder::from_project refactor")
}

/// Wait for any of: SIGTERM, SIGINT, stdin EOF.
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut int = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(_) => return,
    };
    tokio::select! {
        _ = term.recv() => info!("received SIGTERM"),
        _ = int.recv() => info!("received SIGINT"),
    }
}

/// On Linux, ask the kernel to deliver SIGTERM to us when our parent dies.
#[cfg(target_os = "linux")]
fn set_pdeathsig() {
    // SAFETY: prctl is async-signal-safe; the SIGTERM target is the
    // current process which always exists.
    unsafe {
        libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGTERM as libc::c_ulong,
            0,
            0,
            0,
        );
    }
}
