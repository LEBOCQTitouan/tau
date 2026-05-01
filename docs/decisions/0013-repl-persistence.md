# ADR-0013: REPL persistence — `tau chat --resume` and `tau session` group

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:**
- [ADR-0006](0006-tau-runtime.md) §16 — "REPL persistence" was listed
  as a Phase 1+ deferral.
- [ADR-0007](0007-tau-cli.md) §11 — `/save`/`/load` slash commands
  were deferred to Phase 1+ at v0.1. This ADR closes that
  reservation by making auto-save the default (the explicit
  save/load slash commands become moot).
**Amends:** —
**Refines:** [ADR-0006](0006-tau-runtime.md) NG6 (no persistent agent
memory in core) — this ADR places the persistence concern at the CLI
layer; the runtime stays stateless across CLI invocations.

## Context

`tau chat` REPL sessions live entirely in memory. The
`history: Vec<Message>` accumulates per turn in the existing turn
handler at `crates/tau-cli/src/cmd/chat.rs::run_repl`, threaded
into each `Runtime::run_with_history` (or
`run_streaming_with_history` after Tier 2 priority 8) call. On
`/exit`, Ctrl-D, or process termination, the history is dropped.

ADR-0007 §11 captured this as a deliberate v0.1 limitation:

> Four slash commands: `/exit` (quit), `/help` (show commands),
> `/clear` (drop in-memory history), `/history` (print messages so
> far). `/save`, `/load`, `/system`, `/model` etc. are deferred to
> Phase 1+.

ADR-0006 NG6 placed REPL history outside the runtime's
responsibilities: persistent agent memory is "not a runtime
responsibility." This left the door open for a CLI-side
persistence layer to land later.

The constraints driving the five design decisions below:

1. **Industry idiom is split**. Terminal LLM tools fall into four
   camps: JSONL per session (goose, gptme), SQLite single DB
   (simonw/llm, continue.dev, Cursor), single JSON/YAML per
   session (mods, aichat), and Markdown append-only (aider). Each
   has distinct tradeoffs around append cost, crash safety, search,
   and human inspectability.

2. **`PackageSource` is `#[non_exhaustive]`** with only `Git`
   today, but future variants (registry, tarball, local path) are
   anticipated. ADR-0012 made the verify primitive
   source-agnostic for the same reason; the resume drift check
   keeps that posture by comparing `package.name` + `package.version`
   strings rather than git commits.

3. **Multi-version cohabitation already works** (lockfile schema
   v3 from ADR-0012). Resume must accept that the user may have
   several versions of the same package installed; the session
   header records a single version, and resume validates against
   it.

4. **Three-bucket exit code policy** (ADR-0007 §7) maps cleanly
   to resume errors: drift → 2, not-found → 2, ambiguous prefix
   → 2.

## Decision

Five inter-locking commitments:

### 1. JSONL storage format, source-agnostic, append-only

**Decision:** Sessions are stored as JSONL files at
`<scope.state_path()>/sessions/<uuid>.jsonl`. Header on line 1
(`{"type":"header",...}`); subsequent lines are
`{"type":"message", "msg": <Message>}` and
`{"type":"turn_summary", "turn": N, ...}`. Append-only writes per
turn.

**Rationale:**

The terminal-LLM tooling landscape splits four ways. Trade-offs:

| Concern | JSONL | SQLite | JSON | Markdown |
|---|---|---|---|---|
| Append cost (per turn) | O(1) line append | O(1) INSERT | O(history) full rewrite | O(1) line append |
| Crash safety | Partial line recoverable | ACID | Risk on mid-write | Crash-safe |
| Cross-session search | O(N) `grep -r` / `jq` | O(log N) indexed query | O(N) walk + parse | O(N) `grep` |
| Single-session size scaling | Streams fine to 100MB+ | Same (with proper schema) | Reads slow at 10MB+ | Reads slow at 10MB+ |
| Dependency cost | None | ~2MB rusqlite + schema migrations | None | None |
| Human-inspectable | `cat file.jsonl` works | Need sqlite3 CLI | Pretty-print | Just read |
| Migrate later | Trivial walk + INSERT | Hard to escape | Trivial | Trivial |

JSONL wins on every access pattern except cross-session search.
SQLite is the right destination if and when search becomes a
requirement — and it's not in v0.1's scope. JSONL is also free to
migrate to SQLite later: walk all `.jsonl` files, `INSERT` rows.
Reversible.

Tau already has the JSONL precedent at
`crates/tau-runtime/src/plugin_host/recording.rs` (plugin protocol
recording). Adopting JSONL for sessions reuses the mental model
and tooling.

#### 1.1. File layout

```jsonl
{"type":"header","schema":1,"id":"e8b97f2c-...","created_at":"2026-05-01T14:33:21Z","agent_id":"coder","package":{"name":"my-coder-agent","version":"1.0.0","resolved_commit":"abc123..."},"llm_backend":"anthropic","title":null}
{"type":"message","msg":{...}}
{"type":"message","msg":{...}}
{"type":"turn_summary","turn":1,"stop_reason":"EndTurn","input_tokens":523,"output_tokens":187}
{"type":"message","msg":{...}}
...
```

- Line 1 is the **mandatory header**. Schema version 1 baseline.
- Subsequent lines are appended per turn.
- `turn_summary` lines carry per-turn metadata (stop reason +
  token usage) for inspection without re-running.

#### 1.2. Schema versioning

`SCHEMA_VERSION = 1` baseline. Future bumps require their own ADR
amendment. `SessionReader::read` rejects unsupported versions with
`SessionError::UnsupportedSchema { schema, supported }`.

#### 1.3. Crash recovery

`SessionReader::read` skips a single trailing malformed line
gracefully (logs `tracing::warn! name="session.partial_line_skipped"`).
A REPL crash mid-write leaves the file with a partial last line
that the next resume can recover from. Earlier malformed lines
return `SessionError::Parse`.

#### 1.4. Trigger to revisit

- Cross-session full-text search → migrate to SQLite (planned
  destination if/when this lands).
- Schema breaking change → bump `schema` field; reader emits
  upgrade hint.
- Compression for large sessions (gzip) → tracked here as out of
  scope.

### 2. UUID v7 ids with prefix resolution

**Decision:** Session ids are UUID v7 (timestamp-prefixed,
sortable). The CLI accepts ≥8-char prefixes at any subcommand
that takes `<id>`. Prefix resolution finds the longest unique
match; ambiguous prefixes return `SessionError::AmbiguousPrefix`
with the candidate list.

**Rationale:**

UUID v7 matches the existing `AgentInstanceId::new()` precedent
(`crates/tau-domain/src/id.rs:254` uses `Uuid::now_v7()`).
Lexicographic sort = chronological sort, so listings are naturally
ordered by creation time.

8-char prefix gives users a short ergonomic id without collision
risk (UUID v7 has timestamp entropy; collisions within a single
user's archive are vanishingly rare). Lists display the 8-char
prefix; `tau chat --resume <prefix>` and friends resolve to the
longest unique prefix automatically.

#### 2.1. Trigger to revisit

- 100k+ sessions per scope → consider longer prefix display (12
  chars).
- User feedback that UUIDs are unfriendly → optional `/title <name>`
  slash command for human-readable aliases (deferred polish).

### 3. Strict-mode resume + `--force` opt-out

**Decision:** `tau chat <agent> --resume <id>` validates that the
agent's `agent_id`, `package.name`, `package.version`, and
`llm_backend` match what the session header recorded. Mismatch
errors with `SessionError::AgentDrift { field, expected, actual }`
naming the drifted field. `--force` bypasses the check with a
warning printed to stderr.

**Rationale:**

Tau has typed errors for everything else (capability denials,
schema validation, install failures). Silent drift on resume is
inconsistent with that posture: the user expects "this is a
continuation of the same conversation," and silent drift means
the agent's tools, system prompt, or backend may have changed
between save and resume.

The alternative — best-effort resume — would let agent behavior
silently change. The other alternative — snapshot resume (pin the
package version at session creation; reinstall that exact version
if missing) — is over-engineered. Most users won't need
byte-identical reproducibility, and it doubles disk usage. If
reproducibility becomes a Tier 4+ requirement, snapshot resume
can be added as `--pin` opt-in.

The `--force` opt-out gives users an escape hatch for "I know
what I'm doing — resume anyway and accept any breakage."

#### 3.1. Drift fields checked

- `agent_id` (the named entry in project tau.toml).
- `package.name`.
- `package.version`.
- `llm_backend`.

`resolved_commit` is recorded in the header but NOT enforced
(audit field; commits don't change within a single version).

#### 3.2. Trigger to revisit

- Real-world friction with strict mode (e.g., users always
  `--force`) → consider pin-mode default.
- Cross-version session migration (`tau session migrate <id>`) →
  future subcommand if there's demand.

### 4. Auto-save default with `--ephemeral` opt-out

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

#### 4.1. Privacy posture

Sessions persist:
- Full message text (user prompts, agent responses).
- Tool calls (the LLM's intent: "call fs-read with path=X").
- Tool results (file contents, command output, etc.).
- Tool errors (paths that were denied, etc.).

Sessions do NOT persist:
- Environment variables.
- API keys (LLM-backend credentials).
- Plugin process internals.
- The system prompt is recorded (it's part of the agent's
  identity, not a secret).

Users handling sensitive content should use `--ephemeral`. The
CLI prints a one-line reminder on session start so the user is
aware: `✓ Session: <prefix> (will be saved)` or
`⚠ Ephemeral session — not saved to disk`.

#### 4.2. File lifecycle

- Open: `SessionWriter::create(scope, id, header)` on `tau chat
  <agent>` startup. Writes the header line immediately. File
  handle held open for the duration of the REPL.
- Append: each turn appends N message lines + 1 turn_summary line.
- Close: `SessionWriter::close()` on REPL exit. Flushes; no
  metadata rewrite.
- Crash recovery: see §1.3.

#### 4.3. Trigger to revisit

- Per-session encryption (Phase 2+ ADR).
- Auto-prune by age or count → ADR-0013 amendment.
- Tool-result redaction in saved sessions
  (`--no-tool-results` flag) → minor flag addition.

### 5. `tau chat --resume` + `tau session` subcommand group

**Decision:** Resume invocation lives on `tau chat` (resuming IS
chatting). Session management lives in a new `tau session`
subcommand group (mirrors the existing `tau plugin` pattern).

**Rationale:**

Bundling listing/showing/deleting under `--list`/`--show`/`--delete`
flags on `tau chat` muddies the verb: `tau chat --list` does not
chat. Splitting along the verb boundary keeps each command's
purpose clear:

- `tau chat` = active sessions (start new, resume existing).
- `tau session` = archive management.

#### 5.1. CLI surface

```
tau chat <agent>                         # new session (auto-create file)
tau chat <agent> --resume <id>           # resume by id (or 8+ char prefix)
tau chat <agent> --resume <id> --force   # bypass drift check
tau chat <agent> --ephemeral             # in-memory only (no file)

tau session list [<agent>] [--global]   # list sessions
                  [--limit N | --all] [--json]

tau session show <id> [--global]        # render transcript
                  [--json]               # raw JSONL (re-emitted via Output::json)

tau session delete <id> [--global]      # confirmation prompt
                  [--force]              # skip prompt

tau session export <id>                 # convert transcript
                  [--format jsonl|md|json]   # default: jsonl
                  [--global]
```

#### 5.2. Slash command revision (REPL)

| Command | v0.1 behavior |
|---|---|
| `/exit` | unchanged (existing) |
| `/help` | unchanged (updated to drop `/clear`) |
| `/history` | unchanged |
| `/info` | **new** — print session id, file path, turn count, started_at, agent + package |
| `/clear` | **dropped from active surface** — prints deprecation message and continues. The user exits and re-runs for a fresh session. |

`/clear`'s old semantics ("drop in-memory history") are
incoherent with persistence: clearing in-memory leaves the file
intact, so resume would re-surface the cleared messages. Three
options were considered (no-op for the file, rotate to new
session, truncate file); option 3 (drop the command, instruct
user to /exit + re-run) is the simplest and avoids the semantic
murk. Future: a `/new` or `/branch` slash command could add
in-REPL session rotation if demand surfaces.

#### 5.3. Exit codes (per ADR-0007 §7)

| Command | Success | Failure |
|---|---|---|
| `tau chat --resume <id>` (drift, no `--force`) | n/a | 2 |
| `tau chat --resume <id>` (id not found) | n/a | 2 |
| `tau chat --resume <id>` (prefix ambiguous) | n/a | 2 |
| `tau session list` | 0 | 2 (scope error) |
| `tau session show <id>` | 0 | 2 (id not found / ambiguous) |
| `tau session delete <id>` | 0 | 2 (id not found) |
| `tau session export <id>` | 0 | 2 (id not found) |

#### 5.4. Trigger to revisit

- Cross-session search → `tau session search <query>` subcommand
  (likely SQLite migration).
- Session rename / alias names → `tau session rename <id> <name>`.
- Bulk operations → `tau session delete --all`, `tau session
  delete --older-than 30d`.

## Consequences

### Negative / new cost

- `tau-cli` gains a transitive dep on `humantime` (~30KB compiled).
  `humantime-serde` was already pulled in via `humantime`'s parent
  in `tau-pkg`'s lockfile usage; this just makes it a direct dep.
- `tau-cli` gains a `session` module (~1100 LOC across 4 files:
  id.rs, store.rs, render.rs, mod.rs). Code organization is clean:
  one module per logical responsibility.
- The chat REPL's turn handler grows by ~50 LOC for the auto-save
  side-effect. Both branches (streaming + batch) write to the
  session file per turn.
- `/clear`'s behavior change is a soft breaking change for
  scripts that drove it. The deprecation message guides users to
  the new workflow.

### Positive

- Sessions auto-save by default — matches industry idiom; users
  recover the conversation that just gave them the answer.
- Resume is strict by default — the user's mental model is
  preserved (continuation of the same agent), with `--force` as
  the deliberate escape hatch.
- Source-agnostic by design — future `PackageSource` variants
  reuse the resume primitive without redesign.
- The new `tau session` subcommand group enables tooling
  workflows: `tau session list --json | jq`, `tau session show
  <id> | less`, `tau session export <id> --format md > file.md`.
- No tau-runtime changes — runtime stays stateless across CLI
  invocations (NG6 preserved).

### Neutral / new obligations

- Future `tau session` extensions (search, rename, bulk
  operations) require their own ADRs (QG18). The CLI surface is
  locked at this ADR; any expansion needs a new ADR.
- The schema version is locked at 1; future bumps require ADR
  amendment + reader compatibility.

## Alternatives considered

### A. SQLite single DB

Rejected for v0.1. Wins on cross-session search (the headline
feature for simonw/llm and continue.dev). Costs: ~2MB rusqlite
dep, schema migration design, lock contention model. Tau v0.1
doesn't have a cross-session search requirement; defer to a
future ADR if/when it lands. JSONL is free to migrate to SQLite
later (walk + INSERT).

### B. Single JSON/YAML file per session (mods, aichat)

Rejected. Full rewrite per turn is O(history) — bad for sessions
that grow large. Crash safety is worse: a mid-write interrupt can
corrupt the entire file. JSONL's append-only model is strictly
better.

### C. Markdown append-only (aider)

Rejected. Markdown is human-inspectable but breaks structured
operations: tool_use args become unreadable text, no ToolResult /
ToolError variant distinction, no automated round-trip. JSONL
preserves the typed structure.

### D. Best-effort resume (silent drift)

Rejected. See decision 3. Silent drift breaks the user's mental
model. `--force` provides the deliberate opt-out for users who
explicitly want to ignore drift.

### E. Snapshot resume (pin package version at session creation)

Rejected. See decision 3. Over-engineered for v0.1. Doubles disk
usage. Future: `--pin` opt-in if reproducibility becomes a Tier
4+ requirement.

### F. Explicit save (`/save` slash command)

Rejected. See decision 4. "I forgot to save" is the dominant
footgun. Auto-save aligns with industry idiom (ChatGPT, Claude
Desktop, goose, gptme, simonw/llm).

### G. Flags on `tau chat` instead of `tau session` subcommand group

Rejected. See decision 5. `tau chat --list` does not chat; verb
muddying. The subcommand-group pattern (mirroring `tau plugin`)
keeps verbs clear.

### H. Keep `/clear`'s old semantics (no-op for the file)

Rejected. See §5.2. The in-memory clear with the file intact
creates a "phantom resume" surprise: the user clears the
conversation, exits, resumes, and the cleared messages reappear.
Dropping `/clear` from the active surface (with a deprecation
message) is the simplest fix.

## References

- Spec: `docs/superpowers/specs/2026-05-01-repl-persistence-design.md`
- Plan: `docs/superpowers/plans/2026-05-01-repl-persistence.md`
- ADR-0006 §16 — "REPL persistence" Phase 1+ deferral.
- ADR-0006 NG6 — "no persistent agent memory in core"; this ADR
  preserves the boundary by placing persistence at the CLI layer.
- ADR-0007 §11 — `/save`/`/load` slash command deferral; v0.1
  REPL stack (rustyline + termimad + in-memory `Vec<Message>`).
- ADR-0007 §7 — three-bucket exit code policy reused.
- ADR-0009 — typed-error policy; `SessionError` follows.
- ADR-0011 — JSON event-per-line streaming convention reused for
  `tau session list --json`.
- ADR-0012 — three-bucket exit codes for lifecycle commands;
  source-agnostic verify primitive precedent.
- `crates/tau-runtime/src/plugin_host/recording.rs` — JSONL
  writer precedent.
- `crates/tau-domain/src/id.rs` — `AgentInstanceId::new()` UUID v7
  precedent.
- `crates/tau-cli/src/session/` — the new module.
- `crates/tau-cli/src/cmd/chat.rs` — auto-save + resume turn
  handler.
- `crates/tau-cli/src/cmd/session/` — the new subcommand group.
- `crates/tau-pkg/src/scope.rs::Scope::state_path()` — per-scope
  state directory (`<scope>/.tau` for project, `~/.tau` for
  global).
