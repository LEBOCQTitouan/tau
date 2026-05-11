# HTTP transport ↔ proxy chain — design

> **Status:** spec, executing inline. Cuts from main at `b0a0f41` (PR #52 — comment refresh + regression fix).

## Goal

Make the 3 HTTP layer4 tests in `crates/tau-plugin-compat/tests/layer4_native.rs` (anthropic, ollama, openai) actually pass. PR #49 (Phase 0) fixed spawn; PR #51 fixed bridge survival under strict tier; the remaining failure is at HTTP transport — reqwest reports `error sending request for url (http://127.0.0.1:39179/v1/messages)` when dispatching to the cassette server through the bridge → proxy → host chain.

## Hypothesis

The strict-tier `wrap_spawn` (`crates/tau-sandbox-native/src/strict.rs` or wherever env vars are set on the child) sets `HTTPS_PROXY=http://127.0.0.1:8443` but **not** `HTTP_PROXY`. `reqwest` distinguishes the two: `HTTPS_PROXY` is consulted only for HTTPS URLs; `HTTP_PROXY` only for plain HTTP. The cassette server URL is `http://127.0.0.1:39179` (plain HTTP, random port — chosen because the cassette infrastructure doesn't terminate TLS). So reqwest **doesn't route through the bridge** for that URL — it tries direct TCP to `127.0.0.1:39179` inside the empty netns, fails immediately because nothing's listening there.

Confidence: high. Matches the generic "error sending request" shape, the architecture (HTTPS_PROXY-only env), and reqwest's documented env-var semantics.

## Locked decisions

| # | Decision |
|---|---|
| 1 | **Hypothesis-first investigation.** T0a edits `wrap_spawn` locally to set `HTTP_PROXY=http://127.0.0.1:8443` alongside the existing `HTTPS_PROXY`. Re-runs the 3 HTTP tests in the lefthook Podman gate. If they pass → hypothesis confirmed, ship the one-line fix. If they fail → HARD GATE escalation; investigate the full chain with strace (same pattern as Phase 0 / PR #51). |
| 2 | **Fix lands in existing function.** `tau-sandbox-native::strict::wrap_spawn` (or wherever it currently sets `HTTPS_PROXY`). No new public API. No new functions. |
| 3 | **One PR closes the work.** Single branch `feat/http-transport-proxy-chain`. T0a investigation → T0b fix + un-`#[ignore]` 3 tests → T0c USER GATE push + CI → T0d USER GATE squash-merge. Three layer4 HTTP tests un-`#[ignore]`'d and passing in the same PR that ships the env fix. |
| 4 | **Layer4 tests ARE the regression coverage.** No new test file in `tau-sandbox-native`. The 3 un-`#[ignore]`'d tests exercise the full plugin → bridge → proxy → cassette chain end-to-end; a future regression breaks them. YAGNI: don't add a narrower test when the existing tests already prove the property. |
| 5 | **Security envelope unchanged.** Adding `HTTP_PROXY` alongside `HTTPS_PROXY` doesn't expose anything new — both env vars route to the same proxy on `127.0.0.1:8443` inside the netns, which is the only network destination the seccomp + landlock baseline allows. `HTTP_PROXY` is just another alias for the same destination. |

## Components

**MODIFIED**

- `crates/tau-sandbox-native/src/strict.rs` — `wrap_spawn` (or the function where `HTTPS_PROXY` is set on the child Command's env) gets one additional line: `cmd.env("HTTP_PROXY", format!("http://127.0.0.1:{}", proxy_port));` (or matching format string used for `HTTPS_PROXY`). Update the surrounding comment to explain that both env vars are needed because reqwest scheme-gates them.
- `crates/tau-plugin-compat/tests/layer4_native.rs` lines 538, 642, 739 — remove `#[ignore = "..."]` attributes (just the lines, no body changes). The tests stay unchanged otherwise.

**NEW**

- `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` — this spec.

## Architecture

```
Phase 0 PR: feat/http-transport-proxy-chain (cut from b0a0f41)
─ T0a: hypothesis verification (no code commit; spec edit + sign-off)
   ├─ edit wrap_spawn LOCALLY to add HTTP_PROXY env
   ├─ re-run 3 HTTP layer4 tests in Podman gate
   ├─ if PASS: revert local edit, append findings to spec, commit spec
   └─ if FAIL: HARD GATE escalate (full chain investigation)
─ T0b: code fix + un-#[ignore] (single commit)
   ├─ apply HTTP_PROXY env addition to wrap_spawn
   ├─ remove #[ignore] from 3 HTTP tests (lines 538, 642, 739)
   ├─ update wrap_spawn doc comment
   └─ verify locally in Podman gate
─ T0c: USER GATE — push via scripts/agent-push.sh
─ T0d: USER GATE — squash-merge
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
| Hypothesis falsified (HTTP_PROXY addition alone doesn't fix it). | T0a HARD GATE: escalate to user before code commit. Investigate full chain with strace. Same escalation pattern as Phase 0 / PR #51 — proven to work. |
| `HTTP_PROXY` is set but reqwest still doesn't honor it (e.g., reqwest version-specific behavior). | Read reqwest source as a fallback diagnostic. Unlikely — `HTTP_PROXY`/`HTTPS_PROXY` are documented env vars across reqwest versions. |
| Adding `HTTP_PROXY` env breaks an existing test that relied on its absence. | Run full `tau-sandbox-native` lib + integration tests before committing. Run `strict_proxy.rs` + `strict_bridge.rs` specifically. Run the 2 already-un-`#[ignore]`'d layer4 tests (shell stays ignored — sub-project E; fs-read is the regression check). |
| Test failure shape moves to ANOTHER layer (e.g., the plugin's HTTP request reaches the proxy but the proxy can't dial the cassette server). | T0a documents the new failure shape. If it's a narrow issue, scope-amendment in this PR. If it's broader (e.g., proxy can't reach random localhost ports because the host's network setup is wrong), open a follow-up sub-project. |

## Out of scope

- Sub-project E (per-command exec gating; closes shell layer4 test).
- Phase 2 Windows sandbox.
- macOS sandbox-darwin proxy parity (different mechanics on darwin).
- Adding more cassette-replay infrastructure or test fixtures.

## Investigation findings

To be populated by T0a.

```markdown
### T0a — HTTP_PROXY hypothesis verification (DATE)

**Investigator:** [agent-id or human].

**Environment:** lefthook Podman gate on darwin-arm64 host.

**Hypothesis tested:** Setting `HTTP_PROXY=http://127.0.0.1:8443` alongside `HTTPS_PROXY` in `wrap_spawn`'s child env will unblock the 3 HTTP layer4 tests.

**Local edit applied (NOT committed):** [exact code change made to strict.rs or wherever wrap_spawn sets env]

**Test command:**
[exact Podman command run]

**Outcome:**
[verbatim: did 3 tests pass? if some failed, which ones + what shape?]

**Confidence assessment:**
[hypothesis confirmed / falsified; rationale]

**Decision:**
[proceed to T0b with proposed fix / escalate to user for chain investigation]
```
