//! Shared test harness for Layer 2 serve-mode tests.
//!
//! Constructs a `Dispatcher` backed by in-memory channels and a minimal
//! Runtime (MockLlmBackend). No subprocess, no real LLM — fast and
//! deterministic.

#![allow(dead_code)]

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tau_app::serve::{CancelRegistry, Dispatcher, HandshakeState, Inbound, Outbound, Project};
use tau_ports::fixtures::MockLlmBackend;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

pub struct Harness {
    pub in_tx: mpsc::Sender<Inbound>,
    pub out_rx: mpsc::Receiver<Outbound>,
    /// Keeps the dispatcher thread alive until Harness is dropped.
    _dispatcher_thread: std::thread::JoinHandle<()>,
}

impl Harness {
    /// Build a Harness from a fixture project directory.
    pub async fn new(fixture_dir: PathBuf) -> Self {
        let (in_tx, in_rx) = mpsc::channel::<Inbound>(32);
        let (out_tx, out_rx) = mpsc::channel::<Outbound>(64);

        let project = Arc::new(Project::load(&fixture_dir).await.expect("load fixture"));

        // Build a minimal Runtime with a MockLlmBackend.
        // Runtime::builder().build() returns NoLlmBackend if empty, so we
        // wire in a mock backend that is never actually called in handshake tests.
        let backend = MockLlmBackend::new("mock-llm");
        let runtime = Arc::new(
            tau_runtime::Runtime::builder()
                .with_llm_backend(backend)
                .build()
                .expect("build runtime"),
        );

        let dispatcher = Dispatcher {
            project,
            runtime,
            handshake: HandshakeState::default(),
            cancel_reg: CancelRegistry::default(),
            max_concurrent: 8,
            out_tx,
        };

        // `LocalSet` is !Send, so it cannot be spawned with tokio::spawn.
        // Instead, run the dispatcher on a dedicated OS thread that owns a
        // current_thread tokio runtime + LocalSet — the same topology
        // `lifecycle::run` uses in production (local_set.run_until).
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build current_thread rt for dispatcher thread");
            let local_set = LocalSet::new();
            rt.block_on(local_set.run_until(async move {
                let _ = dispatcher.run(in_rx).await;
            }));
        });

        Self {
            in_tx,
            out_rx,
            _dispatcher_thread: thread,
        }
    }

    /// Send a raw JSON line to the dispatcher.
    pub async fn send_raw(&self, line: &str) {
        let v: Value = serde_json::from_str(line).expect("test json");
        let _ = self.in_tx.send(Inbound::Json(v)).await;
    }

    /// Receive the next outbound message, with a 500 ms timeout.
    pub async fn recv(&mut self) -> Option<Value> {
        match tokio::time::timeout(Duration::from_millis(500), self.out_rx.recv()).await {
            Ok(Some(out)) => serde_json::to_value(&out).ok(),
            _ => None,
        }
    }
}
