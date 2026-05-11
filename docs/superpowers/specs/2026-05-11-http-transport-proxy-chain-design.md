# HTTP transport ↔ proxy chain — design

> **Status:** spec, executing inline. Cuts from main at `b0a0f41` (PR #52 — comment refresh + regression fix).
>
> **Amended 2026-05-11 (post-T0a):** T0a's first hypothesis (HTTPS_PROXY-vs-HTTP_PROXY scheme gating) was **falsified** by Podman test — see "Investigation findings" → "T0a" section below. This spec has been amended in place to reflect the new diagnosis (reqwest's loopback bypass) and a renewed two-step investigation cascade.

## Goal

Make the 3 HTTP layer4 tests in `crates/tau-plugin-compat/tests/layer4_native.rs` (anthropic, ollama, openai) actually pass. PR #49 (Phase 0) fixed spawn; PR #51 fixed bridge survival under strict tier; the remaining failure is at HTTP transport — reqwest reports `error sending request for url (http://127.0.0.1:<random>/...)` when dispatching to the cassette server through the bridge → proxy → host chain.

## Hypothesis (post-T0a amendment)

**reqwest bypasses proxy env vars for loopback targets by default.** The cassette server runs on the host's loopback at a random port and the test fixtures pass `http://127.0.0.1:<port>/...` as the plugin's `base_url`. Inside the plugin's empty netns, reqwest sees the loopback URL, short-circuits any configured proxy (including the renewed `HTTPS_PROXY` + `HTTP_PROXY` env vars verified in T0a), and tries direct TCP. That fails because the plugin's netns has its own loopback — separate from the host's — where no random port is listening.

This is reqwest's intentional safety default ("don't accidentally proxy localhost") interacting poorly with the strict-tier sandbox's netns isolation.

The fix must make reqwest actually use the proxy for the cassette server's loopback URL. Two paths:

1. **C — `NO_PROXY=""` in `wrap_spawn`.** Set an explicit-empty NO_PROXY env alongside the existing HTTPS_PROXY + HTTP_PROXY. May override reqwest's loopback default (depends on whether the loopback bypass is NO_PROXY-driven or hardcoded). 15-min Podman experiment.
2. **D — cassette returns a non-loopback `base_url`.** Modify `tau-plugin-test-support::cassette::CassetteServer::base_url` to return a URL whose host triggers proxy routing in reqwest (non-loopback). The cassette already binds `0.0.0.0:0`; only the returned URL needs to change. Exact mechanism TBD by investigation if C falsifies — likely a synthetic `.test` hostname or the host's primary network IP.

## Locked decisions (amended post-T0a)

| # | Decision |
|---|---|
| 1 | **Two-step investigation cascade.** Renewed T0a' first tests option C (NO_PROXY="" env addition). If C passes → T0b applies the one-line fix + un-`#[ignore]` 3 tests; PR closes. If C falsifies → T0a' escalates to user (HARD GATE) before any further code commit; main agent decides whether to expand this PR to option D's investigation or open a separate D-specific PR. |
| 2 | **Spec amendment in place.** This file is rewritten to reflect the post-T0a diagnosis; the original (falsified) hypothesis is preserved in the "Investigation findings" section as historical context. No new spec file; the work item is unchanged (close the 3 HTTP layer4 tests). |
| 3 | **Same branch, same PR.** Continue on `feat/http-transport-proxy-chain`. Append the renewed T0a' findings on top of the existing commits (spec, plan, original T0a falsification). The eventual PR (when C succeeds or D resolves) ships from this branch. |
| 4 | **Layer4 tests ARE the regression coverage.** Unchanged from the original spec: the 3 un-`#[ignore]`'d tests exercise the full plugin → bridge → proxy → cassette chain end-to-end; a future regression breaks them. YAGNI: no narrower test added. |
| 5 | **Security envelope unchanged.** For option C: adding `NO_PROXY=""` alongside the existing proxy envs doesn't broaden the network destinations the seccomp + landlock baseline allows — those are still gated by the strict-tier filter. For option D: a cassette-infra-only change touches `tau-plugin-test-support` (test code), not production plugins. Both options preserve Constitution G12 narrowness. |

## Components (post-amendment)

**Conditional MODIFIED — option C (renewed T0a' confirms hypothesis):**

- `crates/tau-sandbox-native/src/strict.rs` (line 453 area) — `wrap_spawn` gets one additional line: `cmd.env("NO_PROXY", "");` immediately after the existing `HTTPS_PROXY` + `HTTP_PROXY` lines. Update the surrounding comment to explain that the explicit empty disables reqwest's default loopback exemption.
- `crates/tau-plugin-compat/tests/layer4_native.rs` lines 538, 642, 739 — remove `#[ignore = "..."]` attributes.

**Conditional MODIFIED — option D (renewed T0a' falsifies; main agent decides this PR's scope, OR opens new PR):**

- `crates/tau-plugin-test-support/src/cassette.rs` line ~124 (`base_url:` field assignment) — change the returned `base_url` to use a non-loopback host. Exact mechanism TBD by investigation.
- Possibly: `crates/tau-sandbox-proxy/` or `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs` — to handle synthetic hostname routing, if D's mechanism is "synthetic `.test` TLD".
- `crates/tau-plugin-compat/tests/layer4_native.rs` lines 538, 642, 739 — remove `#[ignore]` attributes (same as C).

**Amendment to original (T0a-falsified) MODIFIED list:**

The first amendment of `wrap_spawn` added `HTTP_PROXY=http://127.0.0.1:8443` alongside `HTTPS_PROXY`. T0a verified that's necessary (without it, reqwest doesn't even attempt the proxy for HTTP URLs) but not sufficient. **Keep that change** as part of T0b regardless of which option (C or D) closes the work — both options ship the `HTTP_PROXY` env addition because reqwest still needs it for plain-HTTP URLs.

**NEW:**

- (none — same files modified; no new files)

## Architecture (post-amendment)

```
Renewed PR: feat/http-transport-proxy-chain (cut from b0a0f41)
─ Commits already on branch:
   ├─ ba71bc5: spec (this file — pre-amendment)
   ├─ 34324f2: plan
   └─ 20e5fa9: T0a findings (HTTPS_PROXY/HTTP_PROXY scheme hypothesis falsified)
─ Renewed T0a' — option C test (no code commit; spec edit + sign-off)
   ├─ edit wrap_spawn LOCALLY to add NO_PROXY="" env alongside HTTP_PROXY + HTTPS_PROXY
   ├─ re-run 3 HTTP layer4 tests in Podman gate
   ├─ if PASS: revert local edit, populate T0a' findings, commit spec
   └─ if FAIL: HARD GATE escalate to user (option D investigation)
─ T0b — code fix + un-#[ignore] (single commit, if C succeeds)
   ├─ apply HTTP_PROXY + NO_PROXY="" env additions to wrap_spawn
   ├─ remove #[ignore] from 3 HTTP tests (lines 538, 642, 739)
   ├─ update wrap_spawn doc comment
   └─ verify locally in Podman gate
─ T0c — USER GATE: push via scripts/agent-push.sh
─ T0d — USER GATE: squash-merge

(If renewed T0a' falsifies option C → main agent escalates;
 D-investigation expands scope or branches off.)
```

## Verification

**T0a (hypothesis verification):**
- Spec edit only; no Rust commit.
- Output: "Investigation findings" section populated with: exact env-var change tested, test result (pass / fail), confidence rating for hypothesis.
- If hypothesis fails: HARD GATE — escalate to user. Do not proceed to T0b.

**T0b (fix + un-`#[ignore]`):**
- `cargo nextest run -p tau-sandbox-native --lib` continues passing.
- Inside Podman gate: 3 HTTP layer4 tests PASS; `fs_read_layer4_native_reads_data_file` continues passing; `strict_bridge::bridge_survives_strict_tier_filter` + `strict_proxy::proxy_handle_drop_cleans_up_temp_socket` continue passing.
- Clippy + fmt clean.

**T0c (USER GATE push + CI):**
- `scripts/agent-push.sh -u origin feat/http-transport-proxy-chain` succeeds.
- CI green on 14 required checks (especially `test (tau-plugin-compat / linux)` — the load-bearing job for closure of the 3 HTTP tests).

**T0d (USER GATE squash-merge):**
- `gh pr merge --squash --delete-branch`.

**Branch protection:** No new CI jobs.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| Renewed T0a' falsifies option C (reqwest's loopback bypass is hardcoded, not NO_PROXY-driven). | HARD GATE: escalate to user before code commit. Main agent decides whether to expand this PR's scope to option D or open a separate PR. |
| Option C succeeds but exposes a downstream issue (e.g., plugin handshake survives but cassette playback fails for another reason). | Scope-amend mid-execution (same pattern as Phase 0 / PR #51). Document the new failure shape in the spec; decide between fixing in same PR or escalating. |
| Option D's exact mechanism is fragile (synthetic `.test` DNS requires host `/etc/hosts` or proxy intercept rules). | Defer mechanism choice until after C's outcome is known. If D is needed, T0a' findings will inform the choice between synthetic hostname vs host-IP vs proxy-side intercept. |
| Adding `NO_PROXY=""` env breaks an existing test that relied on its absence. | Run full `tau-sandbox-native` lib + integration tests before committing. Run `strict_proxy.rs` + `strict_bridge.rs` specifically. Run the 2 already-un-`#[ignore]`'d layer4 tests (shell stays ignored — sub-project E; fs-read is the regression check). |
| The first amendment to `wrap_spawn` (adding `HTTP_PROXY` alongside `HTTPS_PROXY` per T0a) is kept regardless of C/D outcome. | T0b commit includes both env additions. Verified necessary by T0a (without HTTP_PROXY, reqwest doesn't even attempt proxy for HTTP URLs); proven insufficient by T0a as well, so additional change (C's `NO_PROXY=""` OR D's cassette change) is also needed. |

## Out of scope

- Sub-project E (per-command exec gating; closes shell layer4 test).
- Phase 2 Windows sandbox.
- macOS sandbox-darwin proxy parity (different mechanics on darwin).
- Adding more cassette-replay infrastructure or test fixtures.

## Investigation findings

### T0a — HTTP_PROXY hypothesis verification (2026-05-11)

**Investigator:** subagent (T0a implementer).

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`) on darwin-arm64 host.

**Hypothesis tested:** Setting `HTTP_PROXY=http://127.0.0.1:8443` alongside `HTTPS_PROXY` in `wrap_spawn`'s child env will unblock the 3 HTTP layer4 tests.

**Local edit applied (NOT committed):** 2-line addition at `crates/tau-sandbox-native/src/strict.rs:453` (right after the existing `cmd.env("HTTPS_PROXY", ...)`):

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
        // T0a (2026-05-11): reqwest scheme-gates HTTPS_PROXY (HTTPS-only)
        // vs HTTP_PROXY (HTTP-only). Cassette tests use plain-HTTP URLs.
        // Both env vars route to the same bridge inside the netns.
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
```

**Test command:**

```bash
podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$PWD:/workspace" \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/workspace/target/lefthook-podman \
  docker.io/library/rust:1.82-bookworm \
  bash -c '
set -ex
apt-get update -qq && apt-get install -y -qq iproute2 nftables
ARCH=$(uname -m)
case "$ARCH" in
  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
esac
if ! command -v cargo-nextest >/dev/null; then
  curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
fi

cargo build --release -p anthropic -p ollama -p openai -p tau-sandbox-native --bin tau-net-bridge
cargo build -p tau-cli --bin tau

mkdir -p target/release
for bin in anthropic-plugin ollama-plugin openai-plugin tau tau-net-bridge; do
  cp -f target/lefthook-podman/release/$bin target/release/$bin 2>/dev/null || true
done

timeout 180 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  ollama_layer4_native_completes_via_cassette \
  openai_layer4_native_completes_via_cassette \
  --features integration-tests \
  --no-fail-fast \
  -- --include-ignored 2>&1 | tail -40
'
```

**Outcome:**

Verbatim nextest summary: `3 tests run: 0 passed, 3 failed, 2 skipped`.

All 3 HTTP layer4 tests FAILED with identical failure shape — `reqwest` returned `error sending request for url (http://127.0.0.1:<port>/...)` where `<port>` is the cassette server's loopback port on the host:

- `anthropic_layer4_native_completes_via_cassette` (line 609): `transport: anthropic transport: error sending request for url (http://127.0.0.1:46737/v1/messages)`
- `ollama_layer4_native_completes_via_cassette` (line 710): `transport: ollama transport: error sending request for url (http://127.0.0.1:34929/api/chat)`
- `openai_layer4_native_completes_via_cassette` (line 805): `transport: openai transport: error sending request for url (http://127.0.0.1:42075/v1/chat/completions)`

The URLs being attempted are the cassette server's host-side loopback addresses (random high ports). Inside the plugin's netns the bridge listens on `127.0.0.1:8443`, but reqwest's request never reaches it.

**Confidence assessment:**

Hypothesis **FALSIFIED**. Adding `HTTP_PROXY=http://127.0.0.1:8443` alongside `HTTPS_PROXY` is not sufficient to unblock the tests. The failure shape changed from previous runs (the request now fails inside reqwest's transport layer rather than at some earlier point), but the tests still do not pass.

Likely root cause hypothesis (un-verified — for follow-up investigation): reqwest by default bypasses configured `HTTP_PROXY`/`HTTPS_PROXY` for loopback addresses (`127.0.0.1`, `localhost`). The cassette test fixtures configure the plugin's `base_url` to a `127.0.0.1:<random-port>` URL on the host, so reqwest sees a loopback target and short-circuits the proxy, attempting a direct connection inside the netns where no such port exists. Setting `HTTP_PROXY` is therefore necessary but not sufficient; the proxy must also be applied to loopback targets (e.g., by configuring reqwest with `no_proxy()` disabled, or by using a proxy build-mode that doesn't auto-exempt loopback).

**Decision:**

**Escalate to user.** Do not proceed to T0b with the one-line `HTTP_PROXY` fix — it's insufficient. The chain needs deeper investigation: (1) confirm reqwest's loopback-bypass behavior, (2) decide whether to configure the plugin's HTTP clients to disable proxy bypass for loopback, configure cassette-test base_urls differently, or expose a proxy-handling toggle in tau-runtime. Pattern: same HARD GATE escalation as Phase 0 / PR #51 — investigate full chain (consider `strace` of the child, or instrumenting the plugin's `reqwest::Client` builder) before code commit.

---

### T0a' — Renewed test (option C: NO_PROXY="")

**Investigator:** subagent (T0a' implementer).

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`) on darwin-arm64 host.

**Hypothesis tested:** Setting `NO_PROXY=""` (explicit empty string) alongside the existing `HTTPS_PROXY` + `HTTP_PROXY` in `wrap_spawn`'s child env will override reqwest's default loopback bypass and route the cassette-server requests through the bridge → proxy chain.

**Local edit applied (NOT committed):** Addition at `crates/tau-sandbox-native/src/strict.rs:453` area (after the T0a-added `HTTP_PROXY` line):

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
        // T0a' (2026-05-11): explicit-empty NO_PROXY disables reqwest's
        // default loopback bypass. Without this, reqwest sees the
        // cassette's 127.0.0.1:<port> URL and short-circuits the proxy.
        cmd.env("NO_PROXY", "");
```

(Keep T0a's `HTTP_PROXY` addition — necessary even though not sufficient.)

**Test command:** Same Podman invocation as T0a (verbatim — see above).

**Outcome:** `Summary [   0.021s] 3 tests run: 0 passed, 3 failed, 2 skipped`. All 3 HTTP layer4 tests failed with identical-shape transport errors against the cassette's loopback URL:

- `ollama_layer4_native_completes_via_cassette` → `plugin error code -32603 message complete failed: transport: ollama transport: error sending request for url (http://127.0.0.1:43359/api/chat)`
- `openai_layer4_native_completes_via_cassette` → `plugin error code -32603 message complete failed: transport: openai transport: error sending request for url (http://127.0.0.1:43753/v1/chat/completions)`
- `anthropic_layer4_native_completes_via_cassette` → `plugin error code -32603 message complete failed: transport: anthropic transport: error sending request for url (http://127.0.0.1:37669/v1/messages)`

All three tests reproduce the same "error sending request for url" shape pointing at the cassette's `127.0.0.1:<random-port>` URL — identical to the T0a baseline. NO_PROXY="" did not change behavior.

**Confidence assessment:** Hypothesis FALSIFIED. Setting `NO_PROXY=""` alongside `HTTPS_PROXY` + `HTTP_PROXY` in the child env did NOT override reqwest's loopback bypass. The cassette requests still short-circuit the proxy and attempt direct TCP inside the empty netns. reqwest's loopback exemption is not driven by (or is not overrideable via) the `NO_PROXY` env var — it appears hardcoded or governed by an internal `no_proxy()` builder path that the env-var read does not override.

**Decision:** Escalate to user for option D investigation (cassette-side non-loopback base_url, or plugin-side `reqwest::Proxy::custom`/`no_proxy(None)` builder change). Do NOT proceed to T0b with the env-only fix — it is provably insufficient.
