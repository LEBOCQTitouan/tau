# Sandbox platform support

This document records the kernel features required by tau's native sandbox adapter, the distros tested in CI, and the known limitations of the current v0.1 enforcement.

## Required kernel features

The native adapter (`tau-sandbox-native`) requires:

- **Linux kernel ≥ 5.13** for [landlock V1](https://docs.kernel.org/userspace-api/landlock.html). Landlock provides per-path filesystem access control.
- **Unprivileged user namespaces** (kernel ≥ 4.18; enabled by default on most modern distros). Required for the namespace-based isolation phase of `wrap_spawn`.
- **seccomp BPF** (kernel ≥ 3.5; ubiquitous on any modern Linux). Used to install the syscall allow-list at Strict tier.

If any of these are missing, the native adapter probes `Unavailable` and the resolver falls through to other adapters per [ADR-0015](../decisions/0015-sandbox-activation.md).

## Tested distros

CI runs the e2e landlock + seccomp + network-filter tests on:

- **Ubuntu 22.04 LTS** (`ubuntu-latest` GH Actions runner; kernel 6.x).

Other distros are unverified but likely work if they meet the kernel requirements above. Reports of working / non-working distros welcome via GitHub issues.

## Known limitations (v0.1)

These limitations are enumerated in [the sandboxing followups doc](../superpowers/specs/2026-05-03-sandboxing-followups.md) and tracked as discrete sub-projects:

- **Per-host network filtering machinery is in place; strict.rs integration is deferred to task 6.5.** The full `tau-sandbox-native::net_filter` module (probe + validate + resolve + veth/netns + nft ruleset + `NetFilterHandle` + orchestrator) shipped at PR #35 (commit d4438ae). However, `Sandbox::wrap_spawn` returns before the child is spawned and the child PID is unknown at that point — per-host filter setup requires the child PID to move the veth peer into the child's netns. The clean solution (`Sandbox::apply_post_spawn` trait method + sync pipe rendezvous) is deferred to task 6.5. Until task 6.5 ships, `unshare_flags_for_plan` retains the v0.1 fallback: when `Network(Http)` is in the plan, the child inherits the parent's network namespace. **Prerequisites for the filter when task 6.5 ships:** Linux + nftables ≥ 0.9 + iproute2 (`ip`) + `nsenter`; `CAP_NET_ADMIN` must be grantable inside an unprivileged user namespace (supported on most modern Linux distros; GHA host runners do NOT support this, hence the `test-net-filter / linux` CI job runs inside privileged Docker). The 3 `#[ignore]`'d Layer 4 container × HTTP plugin tests in `tau-plugin-compat/tests/layer4_container.rs` remain blocked on task 6.5. See [ADR-0019](../decisions/0019-per-host-network-filter.md) for full design rationale.

- **Per-command exec gating is active (sub-project E).** `Capability::Filesystem(Exec { paths })` and `Capability::Process(Spawn { commands })` now grant `AccessFs::Execute` (landlock V1, kernel ≥ 5.13) for the listed paths only; attempting to exec any unlisted path returns EACCES. The earlier doc reference to "landlock V2 required" was incorrect — `AccessFs::Execute` is present in V1. No kernel-version gate beyond the existing V1 requirement is needed.

- **macOS native sandboxing is not yet implemented.** macOS hosts use the container adapter (Docker / Podman). A native macOS adapter via `sandbox-exec` is **sub-project J**.

- **Windows native sandboxing is not yet implemented.** Windows hosts use the container adapter. A native Windows adapter via AppContainer is **sub-project K**.

- **3 container × HTTP plugin Layer 4 tests** are currently `#[ignore]`'d in `tau-plugin-compat/tests/layer4_container.rs` because the localhost cassette-replay server isn't reachable from inside a container's netns without sub-project F's per-host network filter. They flip when F lands. See [ADR-0017](../decisions/0017-e2e-landlock-and-driver.md) Decision 3.

## Verification

The native adapter's kernel-enforcement is verified end-to-end on Linux CI via:

- `test (tau-sandbox-native e2e / linux)` — runs `cargo test -p tau-sandbox-native --features integration-tests --tests`
- `test (tau-runtime e2e / linux)` — runs `cargo test -p tau-runtime --features integration-tests --tests`
- `test (tau-plugin-compat / linux)` — runs the Layer 3 + Layer 4 plugin compat tests under both Container and Native adapters

Branch protection requires all three to pass on every PR. See [ADR-0017](../decisions/0017-e2e-landlock-and-driver.md) for the design rationale.
