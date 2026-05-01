# Spec: REPL persistence (`tau chat --resume`, `tau session`)

**Sub-project:** Tier 3 priority 11 of the tau project.

**Date:** 2026-05-01

**Closes:** [ADR-0007](../../decisions/0007-tau-cli.md) §11 reservation
— `/save`, `/load` slash commands and "history persistence" were
explicitly deferred to Phase 1+ at v0.1. This sub-project closes the
"history persistence" half. The deferred slash commands `/save` and
`/load` become moot once auto-save is the default; their concerns are
absorbed into `tau chat --resume <id>` and the new `tau session`
subcommand group.

**Future ADR:** ADR-0013 will lock the 5 design decisions documented
here (storage format, resume semantics, auto-save policy, CLI
surface split, slash command revision).

## Goals

- Persist `tau chat` REPL sessions to disk so users can resume them.
- Support session listing, viewing, deleting, and exporting via a new
  `tau session` subcommand group.
- Resume continues an existing transcript: load history, validate
  agent-package state hasn't drifted (or `--force`), continue the
  loop.
- Pair naturally with the streaming work shipped in Tier 2 priority
  8 — streamed transcripts now have a destination.

## Non-goals

- **Cross-session full-text search.** That's the headline feature
  for SQLite-backed CLIs (e.g., simonw/llm). Not in v0.1's scope;
  ADR-0013 documents SQLite as the planned migration target if
  search becomes a requirement.
- **In-REPL session navigation UI.** No TUI, no curses, no
  full-screen list. Session management lives outside the REPL via
  `tau session list` etc.
- **Persisted memory in the runtime.** Per ADR-0006 NG6, persistent
  agent memory is NOT a runtime concern. Session storage lives in
  `tau-cli`; the runtime stays stateless across CLI invocations.
- **Cryptographic encryption of session files.** Sessions store
  whatever the LLM and tools see, including tool-result file
  contents. Privacy posture is documented; encryption is a future
  Phase 2+ ADR.
- **`/title` slash command.** Nice-to-have polish; not v0.1.
- **Session pruning / retention policy.** v0.1: indefinite. Users
  delete via `tau session delete <id>`. Auto-prune is a future ADR.
- **Multi-version session migration.** v0.1 ships schema v1; future
  schema bumps require their own ADR amendment.

## Architecture

**Storage:** JSONL files, one per session, append-only.

**Path:** `<scope.state_path()>/sessions/<session_id>.jsonl`. Per-scope:
project sessions stay with the project, global sessions live in
`~/.tau/sessions/`.

**ID scheme:** UUID v7 (timestamp-prefixed, sortable). 36-char
canonical form. CLI accepts shortened prefixes (≥8 chars) — like git
short hashes; resolves to the longest unique prefix at command time.
Lists display the 8-char prefix.

**File format:**

```jsonl
{"type":"header","schema":1,"id":"<uuid>","created_at":"<rfc3339>","agent_id":"<id>","package":{"name":"<pkg>","version":"<semver>","resolved_commit":"<sha>"},"llm_backend":"<pkg>","title":null}
{"type":"message","msg":<Message>}
{"type":"message","msg":<Message>}
{"type":"turn_summary","turn":1,"stop_reason":"<reason>","usage":{...}}
{"type":"message","msg":<Message>}
...
```

- Line 1 is the **mandatory header**. Schema version, session id,
  agent + package + LLM backend metadata.
- Subsequent lines are appended per turn. `message` lines wrap
  `tau_domain::Message` (already `serde::Serialize`).
- `turn_summary` lines are optional metadata between turns. They
  preserve `RunOutcome.total_turns` + `token_usage` per turn for
  inspection without re-running.

**No new workspace member.** Session-storage code in
`crates/tau-cli/src/session/` (new module). Code organization:

| File | Purpose |
|---|---|
| `mod.rs` | Public types: `SessionId`, `SessionHeader`, `SessionEntry`, `SessionMetadata`, `SessionError`. |
| `id.rs` | UUID v7 generation + `resolve_id_prefix(scope, prefix) -> Result<SessionId>`. |
| `store.rs` | `SessionWriter` (append handle), `SessionReader` (parse), `list_sessions(scope) -> Vec<SessionMetadata>`. |
| `render.rs` | Human-mode rendering for `tau session show` (markdown via termimad). |

**No tau-runtime changes.** The runtime's `Runtime::run_streaming_with_history` and `run_with_history` accept a `Vec<Message>` and return a `RunOutcome` — the CLI is responsible for passing the loaded history in and writing the result out.

## 1. Storage format: JSONL (industry-standard for terminal LLM tools)

**Decision:** JSONL files, one per session, append-only. Header on
line 1; messages and turn_summaries on subsequent lines.

**Rationale:**

The terminal-LLM tooling landscape splits four ways: JSONL per
session (goose, gptme), SQLite single DB (simonw/llm, continue.dev,
Cursor), single JSON/YAML per session (mods, aichat), and Markdown
append-only (aider). Trade-offs:

| Concern | JSONL | SQLite | JSON | Markdown |
|---|---|---|---|---|
| Append cost (per turn) | O(1) line append | O(1) INSERT | O(history) full rewrite | O(1) line append |
| Crash safety | Partial line recoverable | ACID | Risk on mid-write | Crash-safe |
| Cross-session search | O(N) `grep -r` / `jq` | O(log N) indexed query | O(N) walk + parse | O(N) `grep` |
| Single-session size scaling | Streams fine to 100MB+ | Same (with proper schema) | Reads slow at 10MB+ | Reads slow at 10MB+ |
| Dependency cost | None | ~2MB rusqlite + schema migrations | None | None |
| Concurrent multi-session writes | OS file locks | Built-in WAL mode | Fragile | OS file locks |
| Human-inspectable | `cat file.jsonl` works | Need sqlite3 CLI | Pretty-print | Just read |
| Migrate later | Trivial walk + INSERT | Hard to escape | Trivial | Trivial |

JSONL wins on every access pattern except cross-session search.
SQLite is the right destination if and when search becomes a v0.1
requirement (it isn't). JSONL is also free to migrate to SQLite
later: walk all `.jsonl` files, `INSERT` rows. Reversible.

Tau already has the JSONL precedent at
`crates/tau-runtime/src/plugin_host/recording.rs` (plugin protocol
recording). Adopting JSONL for sessions reuses the mental model and
tooling.

### 1.1. Header line

```jsonl
{"type":"header","schema":1,"id":"e8b97f2c-3a14-7b2d-9e6c-f4a1b2c3d4e5","created_at":"2026-05-01T14:33:21Z","agent_id":"coder","package":{"name":"my-coder-agent","version":"1.0.0","resolved_commit":"abc123..."},"llm_backend":"anthropic","title":null}
```

Fields:
- `type: "header"` — schema discriminator.
- `schema: 1` — bump on breaking changes only. v0.1 ships `1`.
- `id` — UUID v7 (full 36-char form).
- `created_at` — RFC 3339 timestamp at session creation.
- `agent_id` — the named entry in project `tau.toml`.
- `package` — the resolved package at session-creation time. Used by
  Q4-A strict-mode validation.
- `llm_backend` — the LLM backend package name.
- `title` — `null` in v0.1 (slash command deferred).

### 1.2. Message lines

```jsonl
{"type":"message","msg":<tau_domain::Message>}
```

`tau_domain::Message` already derives `serde::Serialize` /
`Deserialize` behind the `serde` feature. The `msg` field wraps the
existing type unchanged. Messages cover Text, ToolCall, ToolResult,
ToolError, and lifecycle variants.

### 1.3. Turn summary lines (optional)

```jsonl
{"type":"turn_summary","turn":N,"stop_reason":"<StopReason>","usage":{"input_tokens":N,"output_tokens":N}}
```

Emitted after each `RunCompleted { Completed }`. Provides
per-turn metadata without re-running the agent. `tau session show`
includes them in the rendered output; `tau session export --format
json` includes them in the envelope.

### 1.4. Trigger to revisit

- Cross-session full-text search → migrate to SQLite.
- Schema breaking change → bump `schema` field; CLI rejects unknown
  values with an upgrade hint.
- Compression for large sessions (gzip) → tracked here as out of
  scope.

## 2. Session ID scheme: UUID v7 + prefix resolution

**Decision:** UUID v7 (36-char canonical). CLI accepts shortened
prefixes (≥8 chars) at any subcommand that takes a `<id>` argument.
Prefix resolution finds the longest unique prefix; ambiguous prefix
errors with the candidate list.

**Rationale:**

UUID v7 is timestamp-prefixed, so lexicographic sort = chronological
sort. Matches the existing `AgentInstanceId::new()` precedent
(`crates/tau-domain/src/agent.rs` uses `uuid::Uuid::now_v7()`).

8-char prefix is enough for ~10⁹ sessions before collision risk
becomes meaningful (UUID v7 has timestamp entropy, so collisions
within a single user's archive are vanishingly rare). Git-style
prefix matching gives users a short ergonomic id without collision
risk.

Lists display the 8-char prefix; `tau session show <prefix>` /
`tau chat --resume <prefix>` resolve to longest-unique-prefix.

### 2.1. Trigger to revisit

- 100k+ sessions per scope → consider longer prefix display (12
  chars).
- User feedback that UUIDs are unfriendly → optional alias names
  via `/title` slash command (deferred to a future ADR).

## 3. Resume semantics: strict continuation + `--force`

**Decision:** `tau chat <agent> --resume <id>` validates that the
agent's package + version + LLM backend match what was recorded at
session creation. Mismatch errors with a clear message naming the
drifted field. `--force` overrides.

**Rationale:**

Tau has typed errors for everything else (capability denials,
schema validation, install failures); silent drift on resume is
inconsistent with that posture. The `--force` opt-out gives users
an escape hatch for "I know what I'm doing — resume anyway and
accept any breakage."

The alternative — best-effort resume — would let agent behavior
silently change between save and resume (different system prompt,
different tools, different LLM). That breaks the user's mental
model: "this is a continuation of the same conversation."

The other alternative — snapshot resume (pin the package version at
session creation; resume re-installs that exact version if missing)
— is over-engineered. Most users won't need byte-identical
reproducibility, and it doubles disk usage for everyone. If
reproducibility becomes a Tier 4+ requirement, snapshot resume can
be added as `tau chat --resume <id> --pin` opt-in flag.

### 3.1. Drift detection algorithm

On `--resume <id>`:
1. Resolve scope.
2. Resolve `<id>` (or prefix) → exact `SessionId`.
3. `SessionReader::open(scope, id)` → header + entries.
4. Resolve the current state from the project `tau.toml`:
   - Find `agent_id` entry → `AgentDefinition`.
   - Resolve package via existing tau-pkg flow → `PackageManifest +
     resolved version + commit`.
5. Compare to header:
   - `agent_id` (must equal).
   - `package.name` (must equal).
   - `package.version` (must equal).
   - `llm_backend` (must equal).
6. On mismatch (without `--force`): return
   `SessionError::AgentDrift { field, expected, actual }`. Print:
   ```
   error: session 'e8b97f2c' was created with my-coder-agent@1.0.0,
          agent 'coder' now resolves to my-coder-agent@1.1.0.
          Use --force to resume anyway, or `tau session show e8b97f2c`
          to view the transcript without resuming.
   ```
7. With `--force`: print warning, skip the check.
   ```
   ⚠ resuming with my-coder-agent@1.1.0 (session was 1.0.0)
   ```

`resolved_commit` is checked in the header but NOT enforced by
default (it's an audit field, not a gate; commits don't change
within a single version).

### 3.2. Trigger to revisit

- Real-world friction with strict mode (e.g., users always
  `--force`) → consider pin-mode default.
- Cross-version session migration (e.g., upgrade an old session to
  current schema) → `tau session migrate <id>` future subcommand.

## 4. Auto-save default + `--ephemeral` opt-out

**Decision:** Every `tau chat <agent>` invocation auto-creates a
session file from turn 1. No opt-in needed. The `--ephemeral` flag
opts out (in-memory only; no file ever written).

**Rationale:**

Matches dominant idiom: ChatGPT, Claude Desktop, goose, gptme,
simonw/llm. Users have built an expectation that conversations are
recoverable. The "I forgot to save the conversation that just gave
me the answer" footgun outweighs the "I didn't want this saved"
edge case.

`--ephemeral` covers the privacy edge case: when the user
deliberately does not want the conversation persisted (testing,
sensitive content, throwaway exploration).

### 4.1. Privacy posture

Sessions persist:
- Full message text (user prompts, agent responses).
- Tool calls (the LLM's intent: "call fs-read with path=X").
- Tool results (file contents, command output, etc.).
- Tool errors (paths that were denied, etc.).

Sessions do NOT persist:
- Environment variables.
- API keys (LLM-backend credentials).
- Plugin process internals.
- The system prompt is recorded (it's part of the agent's identity,
  not a secret).

Users handling sensitive content should use `--ephemeral`. The CLI
prints a one-line reminder on session start (`✓ Session: e8b97f2c
(project scope)`) so the user is aware they're being saved.

### 4.2. File lifecycle

- Open: `SessionWriter::create(scope, id, header)` on `tau chat
  <agent>` startup. Writes the header line immediately. File handle
  held open for the duration of the REPL.
- Append: each turn appends N message lines + 1 turn_summary line.
- Close: `SessionWriter::close()` on REPL exit. Flushes; no metadata
  rewrite.
- Crash recovery: if the REPL crashes mid-turn, the file ends with
  partial content (some user message lines, possibly no
  turn_summary). On resume, `SessionReader` parses what it can; the
  partial turn is included in the loaded history. Subsequent turns
  append cleanly.

### 4.3. Trigger to revisit

- Per-session encryption (Phase 2+ ADR).
- Auto-prune by age or count → ADR-0013 amendment.
- Tool-result redaction in saved sessions
  (`--no-tool-results` flag) → minor flag addition.

## 5. CLI surface: `tau chat` + `tau session` group

**Decision:** Resume invocation lives on `tau chat` (resuming IS
chatting). Session management lives in a new `tau session`
subcommand group (mirrors the existing `tau plugin` pattern). Listing,
viewing, deleting, exporting are NOT chat actions — they're
archive management.

**Rationale:**

Bundling listing/showing/deleting under `--list`/`--show`/`--delete`
flags on `tau chat` muddies the verb: `tau chat --list` does not
chat. Splitting along the verb boundary keeps each command's
purpose clear:
- `tau chat` = active sessions (start new, resume existing).
- `tau session` = archive management.

### 5.1. `tau chat` surface (modified)

```
tau chat <agent>                         # new session (auto-create file)
tau chat <agent> --resume <id>           # resume by id (or prefix)
tau chat <agent> --resume <id> --force   # bypass drift check
tau chat <agent> --ephemeral             # in-memory only (no file)
tau chat <agent> --no-stream             # existing flag (priority 8)
tau chat <agent> --global                # use global scope
```

The existing `--no-stream` flag (priority 8) is preserved
unchanged.

### 5.2. `tau session` subcommand group (new)

```
tau session list [<agent>]               # list sessions
                  [--global]             # use global scope
                  [--limit N]            # default 20
                  [--all]                # disable limit
                  [--json]               # JSON output

tau session show <id>                    # render transcript
                  [--global]
                  [--json]               # raw JSONL passthrough

tau session delete <id>                  # prompt confirmation
                  [--global]
                  [--force]              # skip prompt

tau session export <id> [--format jsonl|md|json]
                  [--global]
```

`<id>` accepts UUID prefix (≥8 chars).

### 5.3. Output shapes

`tau session list` (human, default 20-row limit):

```
ID        AGENT  CREATED            TURNS  TITLE
e8b97f2c  coder  2026-05-01 14:33      2  -
a3c1f8d4  notes  2026-04-30 09:12      8  -
```

`tau session list --json`:

```json
{"event":"sessions","total":2,"limit":20}
{"event":"session","id":"e8b97f2c-...","prefix":"e8b97f2c","agent":"coder","created_at":"2026-05-01T14:33:21Z","turns":2,"title":null}
{"event":"session","id":"a3c1f8d4-...","prefix":"a3c1f8d4","agent":"notes","created_at":"2026-04-30T09:12:04Z","turns":8,"title":null}
```

`tau session show <id>` (human, markdown via termimad):

```
# Session e8b97f2c-3a14-7b2d-9e6c-f4a1b2c3d4e5
**Agent:** coder (my-coder-agent@1.0.0)
**Started:** 2026-05-01 14:33:21
**Turns:** 2

---

**You:** Hey, can you read src/main.rs and explain what it does?

**coder:** [calls fs-read with {"path": "src/main.rs"}]

**fs-read:** [returned content]

**coder:** The file defines a CLI entry point that...
```

`tau session show <id> --json` is `cat <file>.jsonl` (passthrough).

`tau session delete <id>` (no `--force`):

```
About to delete session e8b97f2c (coder, 2 turns, 2026-05-01).
Continue? [y/N] y
✓ Deleted.
```

`tau session export <id> --format md > /tmp/out.md` writes the
markdown render to stdout.

### 5.4. Exit codes (per ADR-0007 §7)

| Command | Success | Failure |
|---|---|---|
| `tau chat --resume <id>` (drift, no `--force`) | n/a | 2 |
| `tau chat --resume <id>` (id not found) | n/a | 2 |
| `tau session list` | 0 | 2 (scope error) |
| `tau session show <id>` (id found) | 0 | 2 (id not found / prefix ambiguous) |
| `tau session delete <id>` | 0 | 2 (id not found) |
| `tau session export <id>` | 0 | 2 (id not found / format unknown) |

### 5.5. Trigger to revisit

- Cross-session search → `tau session search <query>` subcommand
  (likely SQLite migration).
- Session rename / alias names → `tau session rename <id> <name>`.
- Bulk operations → `tau session delete --all`, `tau session
  delete --older-than 30d`.

## 6. Slash command revision

**Decision:** Drop `/clear` from the v0.1 slash command surface; add
`/info`. Existing `/exit`, `/help`, `/history` unchanged.

**Rationale:**

`/clear`'s existing semantics ("drop in-memory history") are
incoherent with persistence. Three options were considered:

1. Keep `/clear` but make it a no-op for the file: history clears
   in memory, file keeps growing. On resume, the cleared messages
   reappear. **Confusing — `/clear` did not clear what the user
   thinks it cleared.** Rejected.

2. Make `/clear` rotate to a new session (close the current file,
   mint a new id, open a new file). The old session is preserved
   and resumable. Conceptually clean but introduces a non-obvious
   "current session" concept mid-REPL. Rejected for v0.1; can be
   added later as `/new` or `/branch` if demand surfaces.

3. **Drop `/clear` entirely (v0.1 choice).** User exits the REPL
   (`/exit` or Ctrl-D) and re-runs `tau chat <agent>` for a fresh
   session. The dropped command emits a deprecation message
   pointing at this workflow.

Option 3 is the simplest and avoids the semantic murk. The cost
(one extra step for "fresh start") is small.

`/info` is a trivial addition that addresses a real user need:
"what session am I in? where's the file?" One println.

### 6.1. Slash command surface (v0.1)

| Command | Behavior | Source |
|---|---|---|
| `/exit` | Quit the REPL. Existing. | ADR-0007 §11 |
| `/help` | Print available commands. Existing (updated to drop `/clear`). | ADR-0007 §11 |
| `/history` | Print messages so far. Existing. | ADR-0007 §11 |
| `/info` | Print session id (full UUID), file path, turn count, started_at. | This sub-project |
| `/clear` | Removed. Prints a deprecation message: "/clear was removed in 0.x. Exit (/exit) and re-run `tau chat <agent>` for a fresh session." | This sub-project |

### 6.2. `/info` output

```
Session: e8b97f2c-3a14-7b2d-9e6c-f4a1b2c3d4e5
File: .tau/sessions/e8b97f2c-3a14-7b2d-9e6c-f4a1b2c3d4e5.jsonl
Turns: 2
Started: 2026-05-01 14:33:21
Agent: coder (my-coder-agent@1.0.0)
```

For `--ephemeral` sessions:

```
Session: e8b97f2c-3a14-7b2d-9e6c-f4a1b2c3d4e5 (ephemeral; not saved)
Turns: 2
Started: 2026-05-01 14:33:21
```

### 6.3. Trigger to revisit

- `/title <name>` — set human-readable session title. Deferred
  polish.
- `/new` or `/branch` — rotate to fresh session inside the REPL
  (option 2 above) without exiting. If users ask for it.
- `/save <name>` — alias the current session under a name. Deferred
  to a future feature ADR.

## 7. Error handling (per ADR-0009 typed-error policy)

### 7.1. New typed enum

```rust
// crates/tau-cli/src/session/mod.rs

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Session id not found in the scope.
    #[error("session {id_or_prefix:?} not found in scope {scope_path}")]
    NotFound {
        id_or_prefix: String,
        scope_path: PathBuf,
    },

    /// Multiple sessions match the prefix.
    #[error("session prefix {prefix:?} is ambiguous: matches {candidates:?}")]
    AmbiguousPrefix {
        prefix: String,
        candidates: Vec<String>,
    },

    /// Session header is missing or malformed.
    #[error("session {id} has invalid header: {detail}")]
    InvalidHeader {
        id: String,
        detail: String,
    },

    /// Schema version is unsupported.
    #[error("session {id} has unsupported schema {schema}; supported: {supported}")]
    UnsupportedSchema {
        id: String,
        schema: u32,
        supported: u32,
    },

    /// Resume drift detected (without --force).
    #[error("session {id} drift: {field} was {expected:?}, now {actual:?}")]
    AgentDrift {
        id: String,
        field: String,
        expected: String,
        actual: String,
    },

    /// Filesystem I/O error.
    #[error("io error at {path}: {message}")]
    Io {
        path: PathBuf,
        message: String,
    },

    /// JSON parse error.
    #[error("parse error at {path}:{line}: {message}")]
    Parse {
        path: PathBuf,
        line: usize,
        message: String,
    },
}
```

### 7.2. tau-cli conversions

`tau_cli::CliError` (existing) gains `From<SessionError>` mapping
all variants to exit code 2 via the existing dispatch. (Or use
inline `anyhow::anyhow!("{}", e)` per Tasks 6/7 in priority 7's
pattern.)

### 7.3. Existing tau-domain / tau-pkg errors unchanged

The session storage code uses the existing `Scope` API and
`tau_domain::Message` serialization. No changes to upstream crates.

## 8. Testing tier

### 8.1. Unit tests in `tau-cli/src/session/`

- `id::resolve_id_prefix` — exact match, prefix match, ambiguous,
  not found.
- `store::SessionWriter` — header write, message append,
  turn_summary append, file flush, close.
- `store::SessionReader` — parse header, parse message stream,
  parse mid-line crash recovery (partial last line).
- `store::list_sessions` — empty dir, multi-session listing,
  filtered by agent, sorted descending by created_at, limit
  applied.

### 8.2. Integration tests in `tau-cli/tests/`

- `cmd_chat_resume.rs`:
  - `chat_creates_session_file` — `tau chat <agent>` writes header
    + at least one message.
  - `chat_resume_loads_history` — resume picks up where left off.
  - `chat_resume_strict_drift` — drift detected without --force →
    exit 2 + clear error.
  - `chat_resume_force_bypasses_drift` — exit 0; warning printed.
  - `chat_ephemeral_no_file` — `--ephemeral` writes nothing to
    disk.
  - `chat_resume_unknown_id` — exit 2 + clear error.
  - `chat_resume_prefix_ambiguous` — exit 2 + candidate list.

- `cmd_session_list.rs`:
  - `session_list_empty` — no sessions; exit 0; "no sessions"
    message.
  - `session_list_multiple` — 3 sessions; sorted desc by created.
  - `session_list_filter_by_agent` — filter applied.
  - `session_list_json_emits_per_line_events` — line-by-line JSON
    parsing.

- `cmd_session_show.rs`:
  - `session_show_renders_markdown` — human render includes header
    + messages.
  - `session_show_json_passthrough` — `--json` is byte-identical to
    the file.
  - `session_show_unknown_id` — exit 2.

- `cmd_session_delete.rs`:
  - `session_delete_with_force` — file removed; exit 0.
  - `session_delete_unknown` — exit 2.
  - `session_delete_prompt_y` — interactive y → file removed (use
    `assert_cmd::Command::write_stdin("y\n")`).

- `cmd_session_export.rs`:
  - `session_export_jsonl_passthrough`.
  - `session_export_md_renders`.
  - `session_export_json_envelope` — single JSON object with header
    + messages array.

### 8.3. Help snapshot updates

- `tau --help` (top-level): adds `session` subcommand group.
- `tau chat --help`: adds `--resume`, `--ephemeral` flags.
- `tau session --help`: new (lists subcommands).
- `tau session list --help`, `show --help`, `delete --help`,
  `export --help`: 4 new snapshots.

Total help snapshot count grows by 6.

## 9. ADR-0013 outline

Locks five design decisions:

1. **JSONL storage format** — append-only, line-based, per-session
   files. SQLite is the planned migration target if cross-session
   search becomes a requirement.
2. **UUID v7 ids with prefix resolution** — sortable by creation
   time; CLI accepts ≥8-char prefixes.
3. **Strict-mode resume + `--force`** — agent + package + LLM
   backend match; mismatch errors with field name.
4. **Auto-save default with `--ephemeral` opt-out** — every chat
   session persists from turn 1.
5. **`tau chat` + `tau session` CLI split** — resume on chat,
   archive management in a new `tau session` group.

Plus invariants: per-scope storage at `<scope.state_path()>/sessions/`;
no tau-runtime changes (NG6 preserved); `/clear` removed (replaced
by /exit + re-run).

## 10. Task outline (~10-12 tasks for the implementation plan)

The implementation plan (next step) will derive ~10-12 tasks. Likely
structure:

1. `tau-cli::session::id` module + `Uuid` workspace dep (if not
   already present) + unit tests.
2. `tau-cli::session::store` — header types, `SessionWriter`,
   `SessionReader`, append/read tests.
3. `tau-cli::session::store::list_sessions` + `tau-cli::session::id::resolve_id_prefix` + tests.
4. `tau-cli::session::render` — markdown render of a session for
   `tau session show`.
5. `cmd::chat` integration — auto-create session, append per turn,
   `--ephemeral` flag, `/info` slash command, drop `/clear`.
6. `cmd::chat --resume <id>` — drift validation, history replay,
   `--force` flag.
7. `cmd::session list` + e2e tests.
8. `cmd::session show` + e2e tests.
9. `cmd::session delete` + e2e tests.
10. `cmd::session export` + e2e tests.
11. PAUSE — final verification + open PR.
12. PAUSE — ADR-0013 + ROADMAP + squash merge.

## 11. References

- ADR-0006 §16 — "REPL persistence" listed as a Phase 1+ deferral.
- ADR-0006 NG6 — "no persistent agent memory in core"; this
  sub-project respects the boundary.
- ADR-0007 §11 — `/save`/`/load` deferral; v0.1 slash commands
  surface (`/exit`, `/help`, `/clear`, `/history`); existing
  rustyline + termimad REPL stack.
- ADR-0007 §7 — three-bucket exit code policy reused.
- ADR-0009 — typed-error policy; `SessionError` follows.
- ADR-0011 — JSON event-per-line streaming convention reused for
  `tau session list --json` and `tau session show --json` (the
  latter is JSONL passthrough).
- ADR-0012 — three-bucket exit codes for lifecycle commands;
  precedent for the new `tau session` subcommand group's exit-code
  mapping.
- `crates/tau-runtime/src/plugin_host/recording.rs` — JSONL writer
  precedent.
- `crates/tau-domain/src/agent.rs` — `AgentInstanceId::new()` UUID
  v7 precedent.
- `crates/tau-cli/src/cmd/chat.rs` — REPL turn handler (priority 8
  streaming integration).
- `crates/tau-pkg/src/scope.rs` — `Scope::state_path()` per-scope
  state directory.
