//! Run-time options for `Runtime::run` (added in Task 10) and the
//! token-usage report carried in `RunOutcome` (added in Task 6).

/// Token usage reported by the LLM backend, summed across the run.
///
/// Some backends report `total_tokens`; some report only input/output.
/// `Default` returns all zeros (useful when no backend was called yet).
///
/// # Example
///
/// ```
/// use tau_runtime::TokenUsage;
///
/// let usage = TokenUsage::default();
/// assert_eq!(usage.input_tokens, 0);
/// assert_eq!(usage.output_tokens, 0);
/// assert_eq!(usage.total_tokens, None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Total input (prompt) tokens used across the run.
    pub input_tokens: u64,
    /// Total output (completion) tokens emitted across the run.
    pub output_tokens: u64,
    /// `Some(n)` when the backend reports a unified count; `None` otherwise.
    pub total_tokens: Option<u64>,
}

/// Options for `Runtime::run`.
///
/// Constructed via `RunOptions::default()` (then mutated via public
/// fields if needed). `#[non_exhaustive]` to allow additive options
/// later — Phase-1+ may add `soft_fail_tool_errors`, `llm_retry_policy`,
/// `overall_timeout`, etc.
///
/// # Example
///
/// ```ignore
/// // `RunOptions` is `#[non_exhaustive]`; default + field mutation
/// // is the only construction pattern.
/// use tau_runtime::RunOptions;
///
/// let mut opts = RunOptions::default();
/// opts.max_turns = 8;
/// opts.trace_label = Some("session-abc".into());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Hard cap on agent loop iterations. Hitting this returns
    /// `Ok(RunOutcome::Failed { kind: OutOfResources, .. })`.
    /// Default: 16.
    pub max_turns: u32,

    /// Optional caller-supplied label included in tracing spans for
    /// log correlation (e.g. session UUID from a TUI).
    pub trace_label: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            max_turns: 16,
            trace_label: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_options_default_max_turns_is_16() {
        let opts = RunOptions::default();
        assert_eq!(opts.max_turns, 16);
        assert_eq!(opts.trace_label, None);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn run_options_can_override_max_turns_and_trace_label() {
        // RunOptions is #[non_exhaustive]: from outside the crate,
        // struct-literal construction is blocked.  This test intentionally
        // exercises the default() + field-mutation pattern callers must use.
        let mut opts = RunOptions::default();
        opts.max_turns = 100;
        opts.trace_label = Some("session-abc".into());
        assert_eq!(opts.max_turns, 100);
        assert_eq!(opts.trace_label.as_deref(), Some("session-abc"));
    }

    #[test]
    fn token_usage_default_is_all_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn token_usage_is_copy() {
        let a = TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: Some(3),
        };
        let b = a; // requires Copy
        assert_eq!(a, b);
    }
}
