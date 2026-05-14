## Test Ignore Inventory

**Generated:** 2026-05-13
**Workspace state:** based on `origin/main` at `7bec3ab754d16623879a2ae9fe557c393477e623`
**Total `#[ignore]` annotations counted:** 20

Notes:

- This inventory does NOT include the 10 `#[ignore]` annotations added on local branch
  `feat/tests-explicit-skips-compat` (Task 3 of the test-suite-upgrades plan) in
  `crates/tau-plugin-compat/tests/layer4_*.rs`, because those have not yet merged into
  `main`. They will be re-triaged when that PR lands.
- Test function names are the immediate `fn` declaration following the `#[ignore]`
  attribute. Reasons are quoted from the ignore-attribute string, normalized to one line.

### Triage table

| File:line | Test fn | Reason | Status | Added (commit / date) | Notes |
|-----------|---------|--------|--------|-----------------------|-------|
| crates/tau-pkg/tests/install_cross_check.rs:212 | `cross_check_fires_and_fails_for_non_protocol_binary` | requires cargo + full release build + 10s handshake timeout; un-ignore when CI budget is established | ENVIRONMENT | b81de81 (2026-05-05) | Slow (~30s build + 10s timeout). Keep ignored in default `cargo test`; gate behind a dedicated slow-tests job, or run under `--features integration-tests` once a budget is agreed. |
| crates/tau-pkg/tests/install_cross_check.rs:264 | `install_with_matching_manifest_succeeds_and_populates_required_shapes` | requires a tau-protocol-compliant fixture binary; pending sub-project D | STALE | b81de81 (2026-05-05) | Sub-project D shipped at `6c8be31` (PR #25). Blocker resolved, but the test body is still `todo!()` — needs a real fixture-binary wiring before un-ignoring. Candidate for follow-up PR. |
| crates/tau-pkg/tests/install_cross_check.rs:278 | `install_force_after_cross_check_fix_succeeds` | requires a tau-protocol-compliant fixture binary; pending sub-project D | STALE | b81de81 (2026-05-05) | Same status as the row above — sub-project D blocker resolved; body still `todo!()`. Pair with the previous test in the follow-up PR. |
| crates/tau-plugins/anthropic/tests/live.rs:45 | `live_complete_smoke` | live: requires TAU_ANTHROPIC_LIVE_TESTS=1 and ANTHROPIC_API_KEY | ENVIRONMENT | 0a9597d (2026-04-29) | Live API smoke test. Test guards itself with an env-var check that returns early. Keep ignored in default CI; run in a dedicated nightly/secret-bearing job. |
| crates/tau-plugins/anthropic/tests/live.rs:59 | `live_stream_smoke` | live: requires TAU_ANTHROPIC_LIVE_TESTS=1 and ANTHROPIC_API_KEY | ENVIRONMENT | 0a9597d (2026-04-29) | Same as above — paired live smoke test. |
| crates/tau-plugins/ollama/tests/live.rs:49 | `live_complete_smoke` | live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance | ENVIRONMENT | e3df202 (2026-04-29) | Requires local Ollama daemon. Keep ignored in default CI; run in a dedicated job that has Ollama provisioned. |
| crates/tau-plugins/ollama/tests/live.rs:63 | `live_stream_smoke` | live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance | ENVIRONMENT | e3df202 (2026-04-29) | Same as above — paired live smoke test. |
| crates/tau-plugins/openai/tests/live.rs:45 | `live_complete_smoke` | live: requires TAU_OPENAI_LIVE_TESTS=1 + OPENAI_API_KEY | ENVIRONMENT | 87fe0d4 (2026-04-29) | Same shape as the Anthropic / Ollama live tests. |
| crates/tau-plugins/openai/tests/live.rs:59 | `live_stream_smoke` | live: requires TAU_OPENAI_LIVE_TESTS=1 + OPENAI_API_KEY | ENVIRONMENT | 87fe0d4 (2026-04-29) | Same as above — paired live smoke test. |
| crates/tau-runtime/tests/run_kernel_errors.rs:127 | `plugin_contract_violation` | deserialize_tool_args is a passthrough at v0.1; PluginContractViolation triggers land in Phase 1 with schema validation. See Task 10 commit 2562996 and ADR-0006 for the deferral. | STALE | a50ed1d (2026-04-28) | Schema validation has since landed (`crates/tau-runtime/src/tool_args.rs` replaces the v0.1 passthrough; `RuntimeError::PluginContractViolation` is constructed at `plugin_host/mod.rs:471`). Blocker resolved; test body is currently empty and must be implemented before un-ignoring. |
| crates/tau-runtime/tests/sandbox_container.rs:16 | `fs_read_works_inside_container` | requires Linux + docker or podman on PATH | ENVIRONMENT | 2215cf1 (2026-05-03) | File is `#![cfg(all(target_os = "linux", feature = "integration-tests"))]` so it already only compiles under the integration-tests feature. Run via `cargo test -p tau-runtime --features integration-tests -- --ignored` on a Linux runner with a container runtime. |
| crates/tau-runtime/tests/sandbox_container.rs:44 | `shell_plugin_runs_under_container` | requires Linux + docker or podman on PATH | ENVIRONMENT | 2215cf1 (2026-05-03) | Same as above — paired wrap_spawn structural test. |
| crates/tau-cli/tests/cmd_orchestration.rs:26 | `pattern_a_linear_pipeline` | requires MockLlmBackend with multi-turn structured responses; complete in follow-up | DEFERRED | a0daa36 (2026-05-12) | Will be enabled in test-upgrades Task 6 (subagent-driven plan in `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`). |
| crates/tau-cli/tests/cmd_orchestration.rs:46 | `pattern_b_worker_pool` | requires MockLlmBackend; complete in follow-up | DEFERRED | a0daa36 (2026-05-12) | Will be enabled in test-upgrades Task 6 (subagent-driven plan in `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`). |
| crates/tau-cli/tests/cmd_orchestration.rs:63 | `pattern_c_supervisor_critic` | requires MockLlmBackend; complete in follow-up | DEFERRED | a0daa36 (2026-05-12) | Will be enabled in test-upgrades Task 6 (subagent-driven plan in `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`). |
| crates/tau-cli/tests/cmd_orchestration.rs:82 | `pattern_d_hierarchical_team_lead` | requires MockLlmBackend; complete in follow-up | DEFERRED | a0daa36 (2026-05-12) | Will be enabled in test-upgrades Task 6 (subagent-driven plan in `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`). |
| crates/tau-cli/tests/cmd_orchestration.rs:107 | `pattern_e_plan_revise_loop` | requires MockLlmBackend; complete in follow-up | DEFERRED | a0daa36 (2026-05-12) | Will be enabled in test-upgrades Task 6 (subagent-driven plan in `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`). |
| crates/tau-cli/tests/cmd_resolve_check_sandbox.rs:373 | `no_adapter_emits_clear_error` | requires a host with no strict-capable sandbox adapter (no Docker, no Linux native); run in sub-project D e2e CI | ENVIRONMENT | 7fe6cfb (2026-05-04) | Negative-path test that needs a sandbox-free host. Sub-project D shipped but the e2e CI matrix for "no sandbox adapter" isn't wired up; confirm CI job exists or file an issue. |
| crates/tau-cli/tests/cmd_resolve_check_sandbox.rs:538 | `check_sandbox_errors_when_only_passthrough_available` | requires a host with no non-passthrough sandbox adapter (no Docker, no Linux native); run in sub-project D e2e CI | ENVIRONMENT | 7fe6cfb (2026-05-04) | Same shape as the row above — paired negative-path test. |
| crates/tau-cli/tests/cmd_workflow.rs:42 | `workflow_run_writes_jsonl_and_succeeds` | requires echo-llm plugin fixture + project scaffold; lift from cmd_chat.rs/cmd_run.rs when those helpers stabilize | DEFERRED | c9bf67d (2026-05-12) | Waiting on a shared fixture helper. Body is `todo!()`. Candidate to bundle with the orchestration enablement work in Task 6 since both need a stable LLM-backend fixture. |

### Summary

- **ENVIRONMENT:** 11 tests
- **DEFERRED:** 6 tests
- **STALE:** 3 tests (immediate follow-up candidates)
- **UNCLEAR:** 0 tests

### Recommended follow-ups

**STALE — immediate candidates for re-enable PRs:**

- `tau-pkg::install_cross_check::install_with_matching_manifest_succeeds_and_populates_required_shapes` — sub-project D landed (`6c8be31`). Next step: author a tau-protocol-compliant fixture binary (or reuse one already living in `crates/tau-plugin-compat`) and wire it into the test; remove the `#[ignore]`.
- `tau-pkg::install_cross_check::install_force_after_cross_check_fix_succeeds` — bundle with the row above; both need the same fixture.
- `tau-runtime::run_kernel_errors::plugin_contract_violation` — schema validation has landed (`tau-runtime::tool_args` + `plugin_host/mod.rs`). Next step: implement the test body (construct a tool call with malformed input, expect `RuntimeError::PluginContractViolation`); remove the `#[ignore]`.

**DEFERRED — track against their blocking work:**

- 5 orchestration patterns A–E (`cmd_orchestration.rs`) — blocked on MockLlmBackend multi-turn fixture. Tracked as Task 6 of the test-suite-upgrades plan.
- `cmd_workflow::workflow_run_writes_jsonl_and_succeeds` — blocked on a shared LLM-backend fixture helper. Natural candidate to bundle with Task 6.

**ENVIRONMENT — confirm CI surfaces exist:**

- 6 live LLM smoke tests (anthropic / ollama / openai × {complete, stream}) — confirm a nightly job exists that sets the `TAU_*_LIVE_TESTS=1` env vars and provides credentials / a running Ollama daemon. If no such job exists, file an issue.
- 2 container-sandbox structural tests (`sandbox_container.rs`) — already gated by `--features integration-tests` and a Linux `cfg`; confirm a CI matrix entry runs them under `--ignored`.
- 1 cross-check slow test (`cross_check_fires_and_fails_for_non_protocol_binary`) — confirm or open a slow-tests CI lane with a ≥60s budget.
- 2 sandbox-resolver negative-path tests (`cmd_resolve_check_sandbox.rs`) — need a host with no Docker and no Linux native sandbox. Sub-project D was supposed to provide this e2e matrix; verify it exists, otherwise file an issue.
