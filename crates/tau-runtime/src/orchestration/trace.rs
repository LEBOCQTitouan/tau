//! TraceStream. Implementation lands in Task 5.

use tau_ports::TraceEvent;
use tokio::sync::mpsc;

/// Sender side of a trace subscriber.
pub type TraceSubscriber = mpsc::UnboundedSender<TraceEvent>;

/// Container for active subscribers. Implementation lands in Task 5.
#[derive(Default)]
pub struct TraceStream {
    _placeholder: (),
}

impl TraceStream {
    /// Empty stream.
    pub fn new() -> Self {
        Self::default()
    }
}
