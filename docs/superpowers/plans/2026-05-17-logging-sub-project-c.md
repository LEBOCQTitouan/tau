# Logging Sub-project C — Sensitive-Data Preview Helpers

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `tau_observe::preview` helpers (`preview`, `preview_json`, `full`, `full_json`) and migrate every kernel call site that currently formats raw message/argument bodies to use them. The discipline is the §3.9 rule: 256-byte preview at DEBUG, full content at TRACE only.

**Architecture:** Two pairs of helpers in a new module. `preview*` always truncates at 256 bytes ending on a UTF-8 boundary. `full*` returns the value verbatim — call site must be at `tracing::trace!` or below. A `grep`-based CI smoke test rejects any non-TRACE call site that uses `full*`. Per ADR-0006 NG9, this is kernel-internal discipline only.

**Tech Stack:** Rust 2021, `tracing`, `serde_json`. No new transitive deps.

**Depends on:** Sub-project A merged. Sub-project B can land in parallel — they touch different files and the only overlap is at event call sites, which C migrates after B settles.

---

## File Structure

**Created:**
- `crates/tau-observe/src/preview.rs` — the four helpers + tests.
- `crates/tau-observe/tests/preview_smoke.rs` — UTF-8 boundary tests + display contract.
- `crates/tau-runtime/tests/preview_discipline.rs` — `grep`-based CI test asserting no DEBUG-or-above call site uses `full*`.

**Modified:**
- `crates/tau-observe/src/lib.rs` — `pub mod preview;`
- `crates/tau-observe/Cargo.toml` — add `serde_json = { workspace = true }` dependency.
- Various `crates/tau-runtime/src/*.rs` — migrate emissions that include arg/payload data.

---

## Task 1: `preview::preview` for strings

**Files:**
- Create: `crates/tau-observe/src/preview.rs`
- Modify: `crates/tau-observe/src/lib.rs` (add `pub mod preview;`)

- [ ] **Step 1: Write the failing tests**

Create `crates/tau-observe/src/preview.rs`:

```rust
//! Sensitive-data preview helpers (ADR-0006 §3.9 discipline).
//!
//! The kernel emits structured logs containing arguments, message
//! payloads, and LLM responses. Per §3.9 these bodies are previewed
//! (256 bytes, UTF-8-boundary-clipped) at `DEBUG` and below; full
//! content is emitted only at `TRACE`. These helpers make that policy
//! mechanical at every call site.
//!
//! Per ADR-0006 NG9 ("tau does not redact for the caller") this module
//! is **kernel-internal**. Plugin authors may use it but are not
//! required to.

use std::fmt::{self, Display, Formatter};

const PREVIEW_LIMIT_BYTES: usize = 256;

/// Render a `&str` truncated to at most 256 bytes ending on a UTF-8
/// boundary, with a `"…"` ellipsis if truncation occurred.
///
/// Use at `DEBUG` (and below) call sites for argument / payload /
/// message content.
pub fn preview(value: &str) -> impl Display + '_ {
    Preview(value)
}

struct Preview<'a>(&'a str);

impl Display for Preview<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = self.0;
        if s.len() <= PREVIEW_LIMIT_BYTES {
            f.write_str(s)
        } else {
            // Walk back from byte 256 to the nearest char boundary.
            let mut cut = PREVIEW_LIMIT_BYTES;
            while cut > 0 && !s.is_char_boundary(cut) {
                cut -= 1;
            }
            f.write_str(&s[..cut])?;
            f.write_str("…")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_string_passes_through_verbatim() {
        let s = "hello";
        assert_eq!(format!("{}", preview(s)), "hello");
    }

    #[test]
    fn empty_string_passes_through() {
        assert_eq!(format!("{}", preview("")), "");
    }

    #[test]
    fn exactly_256_bytes_no_truncation() {
        let s = "a".repeat(256);
        assert_eq!(format!("{}", preview(&s)), s);
    }

    #[test]
    fn over_256_bytes_truncates_with_ellipsis() {
        let s = "a".repeat(300);
        let out = format!("{}", preview(&s));
        assert!(out.ends_with('…'));
        // Body before the ellipsis is at most 256 bytes.
        let body = out.trim_end_matches('…');
        assert!(body.len() <= 256, "body was {} bytes", body.len());
    }

    #[test]
    fn truncation_respects_utf8_boundary_at_3_byte_codepoint() {
        // "é" is 2 bytes; build a string where the 256th byte falls
        // mid-codepoint by placing a 3-byte char straddling 255-257.
        let mut s = "a".repeat(254);
        s.push('€'); // 3 bytes (U+20AC)
        s.push_str(&"b".repeat(50));
        let out = format!("{}", preview(&s));
        // The € starts at byte 254 and runs through 256 — preview must
        // either include the full € or cut before it. Either way the
        // body must be valid UTF-8.
        let body = out.trim_end_matches('…');
        assert!(std::str::from_utf8(body.as_bytes()).is_ok(), "invalid UTF-8 in preview");
    }

    #[test]
    fn truncation_respects_utf8_boundary_at_4_byte_codepoint() {
        let mut s = "a".repeat(253);
        s.push('𝄞'); // 4 bytes (U+1D11E, musical G clef)
        s.push_str(&"b".repeat(50));
        let out = format!("{}", preview(&s));
        let body = out.trim_end_matches('…');
        assert!(std::str::from_utf8(body.as_bytes()).is_ok(), "invalid UTF-8 in preview");
    }
}
```

In `crates/tau-observe/src/lib.rs`:

```rust
pub mod preview;
```

- [ ] **Step 2: Run + commit**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib preview::`
Expected: 6 tests pass.

```bash
git add crates/tau-observe/src/preview.rs crates/tau-observe/src/lib.rs
git commit -m "feat(tau-observe): preview() for strings — 256-byte UTF-8-safe truncation"
```

---

## Task 2: `preview_json` for `serde_json::Value`

**Files:**
- Modify: `crates/tau-observe/src/preview.rs`
- Modify: `crates/tau-observe/Cargo.toml` (add `serde_json = { workspace = true }`)

- [ ] **Step 1: Add serde_json dep**

In `crates/tau-observe/Cargo.toml`, under `[dependencies]`:

```toml
serde_json         = { workspace = true }
```

- [ ] **Step 2: Write the failing tests in `preview.rs`**

Append to the `tests` module:

```rust
#[test]
fn preview_json_short_value_passes_through() {
    let v = serde_json::json!({"name": "ada"});
    let out = format!("{}", preview_json(&v));
    assert_eq!(out, r#"{"name":"ada"}"#);
}

#[test]
fn preview_json_long_value_truncates() {
    let v = serde_json::json!({"data": "x".repeat(500)});
    let out = format!("{}", preview_json(&v));
    assert!(out.ends_with('…'));
    let body = out.trim_end_matches('…');
    assert!(body.len() <= 256);
}
```

- [ ] **Step 3: Implement `preview_json`**

Add to `preview.rs` (after the existing `preview` definitions, before the `tests` module):

```rust
/// Render a `serde_json::Value` as compact JSON, truncated to at most
/// 256 bytes ending on a UTF-8 boundary, with a `"…"` ellipsis if
/// truncation occurred.
pub fn preview_json(value: &serde_json::Value) -> impl Display + '_ {
    PreviewJson(value)
}

struct PreviewJson<'a>(&'a serde_json::Value);

impl Display for PreviewJson<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_string(&self.0).unwrap_or_else(|_| "<unserializable>".to_string());
        write!(f, "{}", Preview(s.as_str()))
    }
}
```

- [ ] **Step 4: Run + commit**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib preview::`
Expected: 8 tests pass.

```bash
git add crates/tau-observe/src/preview.rs crates/tau-observe/Cargo.toml
git commit -m "feat(tau-observe): preview_json() for serde_json::Value"
```

---

## Task 3: `full` / `full_json` helpers (TRACE-only)

**Files:**
- Modify: `crates/tau-observe/src/preview.rs`

- [ ] **Step 1: Tests first**

```rust
#[test]
fn full_returns_string_verbatim_regardless_of_length() {
    let s = "x".repeat(1000);
    assert_eq!(format!("{}", full(&s)), s);
}

#[test]
fn full_json_returns_value_verbatim_regardless_of_size() {
    let v = serde_json::json!({"data": "x".repeat(1000)});
    let out = format!("{}", full_json(&v));
    assert!(out.contains(&"x".repeat(1000)));
}
```

- [ ] **Step 2: Implement**

```rust
/// Render a `&str` in full.
///
/// **Only call this at `tracing::trace!` (or below) sites.** At any
/// higher level, the macros emit the event unconditionally (subject to
/// the filter) and the full content gets persisted. Use [`preview`]
/// instead for DEBUG and above.
pub fn full(value: &str) -> impl Display + '_ {
    Full(value)
}

struct Full<'a>(&'a str);

impl Display for Full<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Render a `serde_json::Value` as compact JSON in full.
///
/// Same rule as [`full`]: TRACE-only call sites.
pub fn full_json(value: &serde_json::Value) -> impl Display + '_ {
    FullJson(value)
}

struct FullJson<'a>(&'a serde_json::Value);

impl Display for FullJson<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_string(&self.0).unwrap_or_else(|_| "<unserializable>".to_string());
        f.write_str(&s)
    }
}
```

- [ ] **Step 3: Run + commit**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib preview::`
Expected: 10 tests pass.

```bash
git add crates/tau-observe/src/preview.rs
git commit -m "feat(tau-observe): full() / full_json() TRACE-only helpers"
```

---

## Task 4: CI guard — no `full*` outside `trace!`

**Files:**
- Create: `crates/tau-observe/tests/preview_discipline.rs`

This test scans the workspace source and fails if it finds any call to `tau_observe::preview::full` or `full_json` that is *not* inside a `tracing::trace!` or `trace_span!` invocation. It is intentionally simple: a line-based heuristic, not a parser. False positives are acceptable (a developer adds `#[allow(tau_observe_preview_full)]` comment to suppress).

- [ ] **Step 1: Write the test**

Create `crates/tau-observe/tests/preview_discipline.rs`:

```rust
//! CI guard: every call to `preview::full` / `preview::full_json` must
//! be inside a `tracing::trace!` or `trace_span!` invocation. We can't
//! enforce this with the type system (the helpers return `impl Display`
//! and tracing's macros don't know their semantics), so a grep-style
//! lint runs in CI instead.
//!
//! False positives can be silenced by adding the comment
//! `// tau_observe_preview_full_allowed` on the same line.

use std::fs;
use std::path::PathBuf;

#[test]
fn no_full_helper_at_non_trace_callsite() {
    let workspace_root = workspace_root();
    let mut violations = Vec::new();
    walk(&workspace_root.join("crates"), &mut |path, contents| {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        let mut last_macro = None;
        for (line_no, line) in contents.lines().enumerate() {
            if let Some(m) = find_tracing_macro(line) {
                last_macro = Some((m, line_no));
            }
            if (line.contains("preview::full(") || line.contains("preview::full_json("))
                && !line.contains("tau_observe_preview_full_allowed")
            {
                let in_trace_window = match last_macro {
                    Some((m, ln)) if line_no.saturating_sub(ln) < 5 => {
                        m == "trace" || m == "trace_span"
                    }
                    _ => false,
                };
                if !in_trace_window {
                    violations.push(format!("{}:{}: {}", path.display(), line_no + 1, line.trim()));
                }
            }
        }
    });
    assert!(
        violations.is_empty(),
        "preview::full* used at non-trace call sites:\n{}",
        violations.join("\n")
    );
}

fn find_tracing_macro(line: &str) -> Option<&'static str> {
    for macro_name in ["trace", "debug", "info", "warn", "error"] {
        let needles = [
            format!("tracing::{macro_name}!"),
            format!("{macro_name}!("),
            format!("{macro_name}_span!"),
        ];
        for needle in &needles {
            if line.contains(needle) {
                if needle.contains("span") {
                    return Some(Box::leak(format!("{macro_name}_span").into_boxed_str()));
                }
                return Some(Box::leak(macro_name.to_string().into_boxed_str()));
            }
        }
    }
    None
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/tau-observe`; go up two levels.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

fn walk(dir: &std::path::Path, cb: &mut dyn FnMut(&std::path::Path, &str)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ to avoid scanning build artifacts.
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            walk(&path, cb);
        } else if let Ok(contents) = fs::read_to_string(&path) {
            cb(&path, &contents);
        }
    }
}
```

- [ ] **Step 2: Run — should pass trivially since no call sites use `full*` yet**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --test preview_discipline`
Expected: 1 test passes (zero violations).

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/tests/preview_discipline.rs
git commit -m "test(tau-observe): grep-style guard against full* at non-trace callsites"
```

---

## Task 5: Migrate Sub-project B's tool-events to log payloads via `preview_json`

**Files:**
- Modify: `crates/tau-runtime/src/plugin_host/<file>.rs` (the `send_invoke` etc. functions touched in Sub-project B Task 7)

In B Task 7 we deliberately logged only `args_size_bytes` and `result_size_bytes`. Now we add the payload-aware DEBUG variant.

- [ ] **Step 1: Migrate `tool.args_received` to include preview**

Before:

```rust
tracing::debug!(
    target: "tau_runtime::dispatch",
    tool_name = %tool_name,
    args_size_bytes = args_size,
    "{}",
    tau_observe::vocabulary::EV_TOOL_ARGS_RECEIVED,
);
```

After:

```rust
use tau_observe::preview::preview_json;

tracing::debug!(
    target: "tau_runtime::dispatch",
    tool_name = %tool_name,
    args_size_bytes = args_size,
    args = %preview_json(&args_json),
    "{}",
    tau_observe::vocabulary::EV_TOOL_ARGS_RECEIVED,
);

tracing::trace!(
    target: "tau_runtime::dispatch",
    tool_name = %tool_name,
    args = %tau_observe::preview::full_json(&args_json),
    "tool.args_received_full",
);
```

- [ ] **Step 2: Same migration for `tool.result_received`**

Before/after pattern identical to Step 1, with `result_json` in place of `args_json`.

- [ ] **Step 3: Same for `llm.request_built` and `llm.response_received`**

In `stream.rs`. The LLM call accepts a `CompletionRequest` and produces a `CompletionResponse`; preview the serialized form of each.

```rust
use tau_observe::preview::preview_json;

tracing::debug!(
    target: "tau_runtime::stream",
    messages_len = messages.len(),
    request = %preview_json(&serde_json::to_value(&request).unwrap_or_default()),
    "{}",
    tau_observe::vocabulary::EV_LLM_REQUEST_BUILT,
);

tracing::trace!(
    target: "tau_runtime::stream",
    request = %tau_observe::preview::full_json(&serde_json::to_value(&request).unwrap_or_default()),
    "llm.request_built_full",
);
```

- [ ] **Step 4: Test**

In `crates/tau-runtime/tests/preview_in_emissions.rs` (new file):

```rust
//! Assert that DEBUG-level emissions include a previewed payload field
//! and that the value is ≤ 256 bytes.

use tau_observe::capture::Captor;
use tau_observe::vocabulary::*;

#[tokio::test]
async fn tool_args_received_preview_is_within_limit() {
    let captor = Captor::new();
    tracing::subscriber::with_default(captor.subscriber(), || {
        run_tool_call_with_large_args_blocking();
    });
    let event = captor.events().into_iter().find(|e| e.name == EV_TOOL_ARGS_RECEIVED).expect("missing event");
    let preview = event.fields.get("args").expect("missing args field");
    assert!(preview.len() <= 257, "preview was {} bytes (>256 + ellipsis)", preview.len());
}
```

The fixture `run_tool_call_with_large_args_blocking` feeds an args object whose serialized form exceeds 256 bytes.

- [ ] **Step 5: Run + commit**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-runtime --test preview_in_emissions
git add crates/tau-runtime/src crates/tau-runtime/tests/preview_in_emissions.rs
git commit -m "feat(tau-runtime): preview payloads at DEBUG, full content at TRACE"
```

---

## Task 6: Sweep — every remaining call site that includes raw payload data

**Files:** the rest of `crates/tau-runtime/src/`. Locate via:

```bash
grep -rn "tracing::\(debug\|info\|warn\)!" crates/tau-runtime/src | grep -E "args|content|response|message|payload|body"
```

- [ ] **Step 1: For each match, replace direct interpolation with `preview*`**

For example, before:

```rust
tracing::debug!(args = ?args, "tool received");
```

After:

```rust
tracing::debug!(args = %preview_json(&args), "tool received");
```

- [ ] **Step 2: Verify the preview-discipline test still passes**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --test preview_discipline`
Expected: zero violations.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-runtime/src
git commit -m "refactor(tau-runtime): route raw-payload log fields through preview helpers"
```

---

## Task 7: Final verification + push

- [ ] **Step 1: Clippy + nextest**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-observe -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-runtime -- -D warnings
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-observe
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-runtime
```

Expected: all green.

- [ ] **Step 2: Pre-push gate + push**

```bash
timeout 1800 lefthook run pre-push
scripts/agent-push.sh -u origin HEAD
```

- [ ] **Step 3: PR**

Title: `feat(tau-observe): preview/full helpers + kernel sensitive-data discipline (Sub-project C)`.

---

## Spec coverage check

- Spec sub-project C "`preview` and `preview_json` helpers" → Tasks 1, 2.
- Spec sub-project C "`full` and `full_json` helpers" → Task 3.
- Spec sub-project C "kernel-internal discipline only" → discipline enforced by Task 4 grep test; no public-API additions to plugins.
- Spec testing C "UTF-8 boundary tests" → Task 1 (3-byte + 4-byte codepoint cases).
- Spec testing C "lint or workspace deny rule out of scope for v1; enforcement is by code review against the rule" → Task 4 ships the rule as a test instead of code review (stricter than spec required).
