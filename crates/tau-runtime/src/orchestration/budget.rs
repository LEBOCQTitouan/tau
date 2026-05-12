//! Budget enforcement. Implementation lands in Task 10.

/// Watchdog handle. Implementation lands in Task 10.
#[derive(Default)]
pub struct BudgetWatchdog {
    _placeholder: (),
}

impl BudgetWatchdog {
    /// New watchdog.
    pub fn new() -> Self {
        Self::default()
    }
}
