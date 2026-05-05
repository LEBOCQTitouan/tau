//! Sub-project D Task 2 — real-kernel exec-gating e2e tests.
//!
//! `#[ignore]`'d stub. Per-command exec gating requires landlock V2
//! (kernel >= 5.19) which is sub-project E's scope.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

#[tokio::test]
#[ignore = "Per-command exec gating requires landlock V2 (kernel >= 5.19); pending sub-project E"]
async fn exec_blocked_without_process_capability() {
    // Will exercise: plan with no Process(Spawn) cap -> exec fails.
}
