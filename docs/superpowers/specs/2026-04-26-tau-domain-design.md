# Tau Domain (sub-project 1) — Design Spec

**Date:** 2026-04-26
**Sub-project:** `tau-domain` types (sub-project 1; first of the Phase-0 sub-projects following Bootstrap)
**Author:** Titouan Lebocq
**Status:** Approved for implementation planning

---

## 1. Scope & success criteria

### Scope

Land the public type surface of `tau-domain`: messages, agents, packages, capabilities, errors. tau-domain is the inside of the hexagon — pure data, no I/O, no async, no platform deps, no plugin trait definitions (those live in `tau-ports`). Every subsequent sub-project (2–5) consumes types from `tau-domain` without re-defining them.

### Done when

- `crates/tau-domain/` exposes the types listed in §3.
- `cargo build -p tau-domain --no-default-features` succeeds locally and in CI.
- `cargo build -p tau-domain --all-features` succeeds locally and in CI.
- `cargo clippy -p tau-domain --all-targets --all-features -- -D warnings` succeeds.
- `cargo fmt --all -- --check` succeeds.
- `cargo test -p tau-domain --all-targets --all-features` succeeds (proptest, doctests, integration, golden).
- `cargo test -p tau-domain --doc --all-features` succeeds — every public item has an example.
- ADR-0002 (manifest format + capability evolution rules + escape-hatch policy) is filed in `docs/decisions/` and accepted.
- `docs/explanation/escape-hatches.md` exists with seeded entries for every v0.1 escape hatch (`Capability::Custom`, `MessagePayload::Custom`, `PackageKind::Custom`, `FailureKind::InternalError`).
- `crates/tau-domain/tests/escape_hatch_registry.rs` passes — every escape-hatch variant in the workspace has a matching registry entry, and no registry entry is stale.
- `.github/pull_request_template.md` includes the escape-hatch checklist.
- The git log on `main` contains a clean per-sub-task series of Conventional Commits (one commit per task in the implementation plan).
- CI green on Linux + macOS (Windows non-blocking per G15) for both `--no-default-features` and `--all-features` build modes.

### Out of scope (explicit, deferred to later sub-projects or ADRs)

| Item | Owner / Trigger |
|---|---|
| `Option<PackageName>` for `AgentDefinition.llm_backend` | Sub-project 4+ if a non-LLM agent use case materializes. |
| Typed `Capability` variants beyond Filesystem/Network/Process/Agent | Land additively as tau-runtime starts enforcing (sub-project 4+). |
| Typed `HttpMethod` enum | `Vec<String>` at v0.1; ADR if normalization needed. |
| Typed `GitRev` (branch/tag/commit) | `Option<String>` at v0.1; tau-pkg disambiguates at clone time. |
| `PackageVersion` newtype around `semver::Version` | Re-export at v0.1; ADR if SemVer pre-release/build metadata normalization needed. |
| `AgentInstance` struct (runtime composition) | Owned by tau-runtime. |
| Skill references / MCP attachments on `AgentDefinition` | Go through `config: BTreeMap<String, Value>` at v0.1; typed when plugin trait stabilizes. |
| Resource limits on agents | Phase-1+ sandboxing concern (G12). |
| `AgentStatus` state-machine transitions | Runtime concern; not in tau-domain. |
| Custom envelope parser (serve-mode framing) | Lives in tau-runtime when serve mode lands. |
| Span / source-text tracking in error types | `MalformedUrl { reason: String }` carries upstream's text only. |
| Partial parsing / recovery, streaming deserialization | All-or-nothing; bounded inputs. |
| i18n of error `Display` text | Forever-out-of-scope in core. |
| Manifest schema versioning field | Tau-domain SemVer covers it; additive `schema_version: Option<u32>` if needed. |
| `[build]` section, lockfile types, optional/conditional deps, signing | Future ADRs. |
| Local-path / tarball / registry `PackageSource` variants | Additive `non_exhaustive` variants for later. |
| Top-level `DomainError` umbrella | Per-concern enums forever (rejected at design time). |
| `cargo audit` / `cargo-deny`, mutation testing, fuzz targets | QG16 defers to Phase 2; fuzz targets the IPC protocol, which is not tau-domain. |

---

## 2. Module layout & dependencies

```
crates/tau-domain/
├── Cargo.toml
└── src/
    ├── lib.rs              # crate-level docs, re-exports, lint attrs, feature gates
    ├── id.rs               # PackageName, AgentId, AgentInstanceId, MessageId
    ├── version.rs          # re-export of semver::Version, semver::VersionReq
    ├── value.rs            # Value enum + accessor helpers
    ├── message.rs          # Message envelope, Address, MessagePayload
    ├── agent.rs            # AgentDefinition, AgentStatus, FailureKind
    ├── package/
    │   ├── mod.rs          # re-exports for the package submodules
    │   ├── source.rs       # PackageSource, GitLocation + parsers
    │   ├── manifest.rs     # UncheckedManifest, PackageManifest, PackageDep, PackageId, PackageKind, kinds module
    │   └── capability.rs   # Capability, FsCapability, NetCapability, ProcessCapability, AgentCapability
    └── error.rs            # PackageNameError, AgentIdError, PackageSourceError, PackageKindError, PackageManifestError
```

### Cargo.toml additions

Workspace root (`Cargo.toml`) gains a `[workspace.dependencies]` block:

```toml
[workspace.dependencies]
thiserror = "2"
semver    = { version = "1", features = [] }   # serde feature toggled per-crate
uuid      = { version = "1", features = ["v7"] }
url       = "2"
serde     = { version = "1", features = ["derive"] }
proptest  = "1"
```

`crates/tau-domain/Cargo.toml`:

```toml
[dependencies]
thiserror = { workspace = true }
semver    = { workspace = true }
uuid      = { workspace = true }
url       = { workspace = true }
serde     = { workspace = true, optional = true }

[features]
default       = []
serde         = ["dep:serde", "uuid/serde", "semver/serde", "url/serde"]
test-fixtures = []

[dev-dependencies]
proptest    = { workspace = true }
serde_json  = "1"
toml        = "0.8"
```

Notes:
- **No `toml` dependency in main deps.** TOML deserialization is a tau-pkg concern; tau-domain owns the data type + serde derives + structural validation only. `toml` is in dev-deps for round-trip integration tests.
- **`serde` feature is off by default** per Q1; cascades to `uuid/serde`, `semver/serde`, `url/serde` so the wire format is consistent across the type tree.
- **`test-fixtures` feature** exposes a `pub mod fixtures` with construction helpers (see §5). Off by default; downstream crates opt in via `[dev-dependencies]`.
- **`proptest` is a dev-dep only.** Exposing `Arbitrary` impls behind a public feature is deferred — additive landing later doesn't break anyone.

---

## 3. Type-by-type design

### 3.1 IDs (`id.rs`)

```rust
pub struct PackageName(String);          // [a-z][a-z0-9-]{0,63}, validated at construction
pub struct AgentId(String);              // [a-z][a-z0-9-]{0,63}, validated at construction
pub struct AgentInstanceId(uuid::Uuid);  // UUID v7 (monotonic, sortable)
pub struct MessageId(uuid::Uuid);        // UUID v7
```

- All four implement `Debug + Clone + PartialEq + Eq + Hash + Display + FromStr`.
- `PackageName` and `AgentId` are validating newtypes; constructors return their respective error types (`PackageNameError`, `AgentIdError`).
- `AgentInstanceId` and `MessageId` expose `pub fn new() -> Self` (generates a fresh UUID v7) and `pub fn from_uuid(u: Uuid) -> Self`. They do NOT validate (UUID v7 has no application-level invariants).
- All four implement `Serialize` + `Deserialize` under the `serde` feature.

Grammar rationale: kebab-case ASCII only. No unicode → no homoglyph attacks on package names. 64-char cap chosen because npm's de facto cap is 214 and that's wide enough to hide manifest-injection attacks; tau picks a tighter limit.

### 3.2 `Value` (`value.rs`)

JSON-shaped value used by manifest capability params and tool args/results.

```rust
#[non_exhaustive]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),                      // images, file blobs
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}
```

Accessor helpers (cut consumer boilerplate):

```rust
impl Value {
    pub fn as_string(&self)  -> Option<&str>;
    pub fn as_integer(&self) -> Option<i64>;
    pub fn as_float(&self)   -> Option<f64>;
    pub fn as_bool(&self)    -> Option<bool>;
    pub fn as_bytes(&self)   -> Option<&[u8]>;
    pub fn as_array(&self)   -> Option<&[Value]>;
    pub fn as_object(&self)  -> Option<&BTreeMap<String, Value>>;
    pub fn is_null(&self)    -> bool;
}
```

`BTreeMap` (not `HashMap`) for deterministic iteration order — matters for golden tests and stable wire format.

### 3.3 Messages (`message.rs`)

```rust
pub struct Message {
    pub id: MessageId,
    pub sender: Address,
    pub recipient: Address,
    pub parent_id: Option<MessageId>,        // sole causality mechanism at v0.1
    pub created_at: SystemTime,
    pub headers: BTreeMap<String, String>,
    pub payload: MessagePayload,
}

#[non_exhaustive]
pub enum Address {
    Agent(AgentInstanceId),
    Tool(String),                            // tool-name; runtime owns name→plugin resolution
    User,
    System,
}

#[non_exhaustive]
pub enum MessagePayload {
    Text       { content: String },
    ToolCall   { args: Value },              // recipient Address says which tool
    ToolResult { body: Value },
    ToolError  { kind: String, message: String, details: Option<Value> },
    Lifecycle(AgentStatus),
    /// Plugin-specific message kind. See: [escape-hatches.md#messagepayload-custom](../explanation/escape-hatches.md#messagepayload-custom).
    Custom     { kind: String, body: Vec<u8> },
}
```

Design calls:
- **`Text` not `UserText`/`AgentText`** — `sender: Address` distinguishes origin; two near-identical variants would just be aliases.
- **No `tool: String` in `ToolCall`** — redundant with `recipient: Address::Tool(...)`.
- **`SystemTime` for `created_at`** — std-only, no pull-in of `chrono` or `time`. UUID v7 already embeds a timestamp; the explicit field is for observability tools that don't decode UUIDs.
- **`parent_id: Option<MessageId>`** is the only causality mechanism. No vector clocks, no thread/conversation IDs at v0.1. Reply chains traced via `parent_id` are sufficient for tau-observe and the solo runtime path.
- **`#[non_exhaustive]` on `Address` and `MessagePayload`** — additive variants are non-breaking.

### 3.4 Agents (`agent.rs`)

```rust
#[non_exhaustive]
pub struct AgentDefinition {
    pub id: AgentId,
    pub display_name: String,                // unvalidated free-form
    pub package: PackageId,
    pub llm_backend: PackageName,            // REQUIRED — see §6 ADR-0002 rationale
    pub system_prompt: Option<String>,
    pub config: BTreeMap<String, Value>,     // free-form per-agent config (validated by plugins)
}

#[non_exhaustive]
pub enum AgentStatus {
    Declared,                                // manifest seen, package not installed
    Installed,                               // package on disk, ready to instantiate
    Ready,                                   // instance created, idle
    Running,                                 // actively processing a message
    Stopped,                                 // intentionally halted
    #[non_exhaustive] Failed { kind: FailureKind, detail: Option<String> },
}

#[non_exhaustive]
pub enum FailureKind {
    Crashed,                                 // panic, signal, abort
    BackendError,                            // LLM backend returned error or unreachable
    PolicyDenied,                            // capability check rejected an operation
    OutOfResources,                          // memory cap, message-rate cap, timeout
    /// Catch-all for failures that don't match the named kinds.
    /// See: [escape-hatches.md#failurekind-internalerror](../explanation/escape-hatches.md#failurekind-internalerror).
    InternalError,
}
```

Construction:

```rust
impl AgentDefinition {
    pub fn new(id: AgentId, display_name: String, package: PackageId, llm_backend: PackageName) -> Self;
    pub fn with_system_prompt(self, prompt: String) -> Self;       // builder method
    pub fn with_config(self, config: BTreeMap<String, Value>) -> Self;
}
```

Infallible constructor — all field types are pre-validated. No `AgentDefinitionError` at v0.1.

Lifecycle transitions: documented in rustdoc as a state diagram (`Declared → Installed → Ready → Running ↔ Stopped`, with `Failed` reachable from any non-terminal state). NOT encoded as code per Q2/B; transitions live in tau-runtime.

### 3.5 Packages (`package/`)

#### 3.5.1 Source (`package/source.rs`)

```rust
#[non_exhaustive]
pub enum PackageSource {
    Git { location: GitLocation, rev: Option<String> },
}

#[non_exhaustive]
pub enum GitLocation {
    Url(url::Url),                           // https, http, ssh, git schemes
    Scp {
        user: Option<String>,
        host: String,
        path: String,
    },
}
```

Parsers:
- `impl FromStr for PackageSource` — splits on `#` for `<location>#<rev>`, delegates location to `GitLocation`.
- `impl FromStr for GitLocation` — tries `url::Url::parse` first, validates scheme is one of `https/http/ssh/git`; falls back to scp-style parser (hand-rolled, ~30 lines).
- `impl Display` for both — round-trips parsing.

Allowed schemes for `GitLocation::Url`: `https`, `http`, `ssh`, `git`. Anything else returns `PackageSourceError::UnsupportedScheme`.

#### 3.5.2 Manifest (`package/manifest.rs`)

**Typestate**: `UncheckedManifest` is the deserialization target; `PackageManifest` wraps a validated `UncheckedManifest`. Forces deserialize → `validate()` → use as the only path to a `PackageManifest`.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
// Serialize + Deserialize derived under the `serde` feature
pub struct UncheckedManifest {
    pub name: PackageName,
    pub version: semver::Version,
    pub description: String,
    pub authors: Vec<String>,
    pub license: Option<String>,             // SPDX expression as opaque text at v0.1
    pub source: PackageSource,
    pub kind: PackageKind,
    pub dependencies: Vec<PackageDep>,
    pub capabilities: Vec<Capability>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
// Serialize derived under `serde` (delegates to inner); NOT Deserialize
pub struct PackageManifest(UncheckedManifest);   // private inner; no Deref

impl UncheckedManifest {
    pub fn validate(self) -> Result<PackageManifest, PackageManifestError>;
}

impl PackageManifest {
    // Read-only accessors (one per field).
    pub fn name(&self)         -> &PackageName;
    pub fn version(&self)      -> &semver::Version;
    pub fn description(&self)  -> &str;
    pub fn authors(&self)      -> &[String];
    pub fn license(&self)      -> Option<&str>;
    pub fn source(&self)       -> &PackageSource;
    pub fn kind(&self)         -> &PackageKind;
    pub fn dependencies(&self) -> &[PackageDep];
    pub fn capabilities(&self) -> &[Capability];
}

impl From<PackageManifest> for UncheckedManifest;   // downgrade for mutation round-trip
```

Key points:
- `PackageManifest` does NOT implement `Deserialize`. Forcing the deserialize-then-validate path is the entire purpose of the typestate. Consumers write `let m: PackageManifest = toml::from_str::<UncheckedManifest>(s)?.validate()?;`.
- `PackageManifest` implements `Serialize` by delegating to the inner `UncheckedManifest` — wire format is symmetric.
- No `Deref<Target = UncheckedManifest>` — would erode the typestate guarantee.

Supporting types:

```rust
#[non_exhaustive]
pub struct PackageDep {
    pub name: PackageName,
    pub version_req: semver::VersionReq,
}

#[non_exhaustive]
pub struct PackageId {
    pub name: PackageName,
    pub version: semver::Version,
}

#[non_exhaustive]
pub enum PackageKind {
    /// Structural package kind at v0.1; typed variants land later.
    /// See: [escape-hatches.md#packagekind-custom](../explanation/escape-hatches.md#packagekind-custom).
    Custom { kind: String },
}

/// Canonical kind strings — recommended convention, not mandated.
pub mod kinds {
    pub const LLM_BACKEND: &str = "llm-backend";
    pub const TOOL: &str        = "tool";
    pub const SKILL: &str       = "skill";
    pub const PIPELINE: &str    = "pipeline";
    pub const MCP_SERVER: &str  = "mcp-server";
    pub const STORAGE: &str     = "storage";
    pub const SANDBOX: &str     = "sandbox";
}
```

#### 3.5.3 Capabilities (`package/capability.rs`)

Hierarchical typed shape (per Q4 + β refinement):

```rust
#[non_exhaustive]
pub enum Capability {
    Filesystem(FsCapability),
    Network(NetCapability),
    Process(ProcessCapability),
    Agent(AgentCapability),
    /// Plugin-specific capability not yet typed in core.
    /// See: [escape-hatches.md#capability-custom](../explanation/escape-hatches.md#capability-custom).
    Custom { name: String, params: BTreeMap<String, Value> },
}

#[non_exhaustive]
pub enum FsCapability {
    #[non_exhaustive] Read  { paths: Vec<String> },
    #[non_exhaustive] Write { paths: Vec<String>, max_bytes: Option<u64> },
    #[non_exhaustive] Exec  { paths: Vec<String> },
}

#[non_exhaustive]
pub enum NetCapability {
    #[non_exhaustive] Http { hosts: Vec<String>, methods: Vec<String> },
}

#[non_exhaustive]
pub enum ProcessCapability {
    #[non_exhaustive] Spawn { commands: Vec<String> },
}

#[non_exhaustive]
pub enum AgentCapability {
    #[non_exhaustive] Spawn { allowed_kinds: Vec<String> },
}
```

**Variant-level `#[non_exhaustive]`** — important: lets us add fields to existing variants (e.g., `FsCapability::Read { paths, recursive: bool }`) without breaking destructuring patterns, because consumers must already write `{ paths, .. }`.

**Wire format & canonicalization** (see ADR-0002 in §6):
- Manifest TOML form uses `kind = "fs.read"` + flat fields, NOT nested external-tagged JSON.
- A custom `Deserialize` impl for `Capability` maps the dot-namespaced `kind` to the variant tree.
- Round-trip property: deserialize → serialize produces canonical TOML/JSON form for each typed variant; `Custom { name, params }` round-trips as-is.

### 3.6 Errors (`error.rs`)

Per-concern enums, all `#[non_exhaustive]`, all derive `Debug + Error + Clone + PartialEq + Eq` (uniform per §5/A).

```rust
#[non_exhaustive]
pub enum PackageNameError { Empty, TooLong { max, got }, InvalidCharacter { ch, pos }, InvalidLeadingCharacter { ch } }

#[non_exhaustive]
pub enum AgentIdError     { /* identical body to PackageNameError */ }

#[non_exhaustive]
pub enum PackageSourceError { Empty, UnsupportedScheme { scheme }, MalformedUrl { reason }, MalformedScpAddress { reason }, EmptyRevision }

#[non_exhaustive]
pub enum PackageKindError { Empty }

#[non_exhaustive]
pub enum PackageManifestError {
    Name(#[from] PackageNameError),
    Source(#[from] PackageSourceError),
    Kind(#[from] PackageKindError),
    EmptyDescription,
    DependencyName { index: usize, source: PackageNameError },   // #[source], not #[from]
    CapabilityEmptyName { index: usize },
}
```

Notes:
- **All error enums derive `Debug + Error + Clone + PartialEq + Eq` uniformly.** All current variants contain only types that satisfy these traits (primitives, owned `String`, owned other-error types). Future variants must remain derivable or accept a pre-1.0 minor breaking change. Tests with free-form `String` fields (e.g., `MalformedUrl.reason`) should use `matches!(err, PackageManifestError::Source(PackageSourceError::MalformedUrl { .. }))` to avoid brittle wording comparisons; this is a discipline rule, not a derive concern.
- **`AgentDefinitionError`, `MessageParseError`, `CapabilityError`** — all deferred (no fallible v0.1 path produces them).
- **`semver::Error`** is re-exported, not wrapped.

---

## 4. Parser surface (proptest-covered)

Per Q6/B, four parsers earn dedicated proptests; the rest lean on serde + structural validation.

| Parser | Strategy | Round-trip property | Rejection property |
|---|---|---|---|
| `PackageSource` (incl. `GitLocation`) | proptest | `from_str(p.to_string()) == Ok(p)` for valid generated sources | malformed strategies → specific error variant |
| `PackageName` | proptest | valid kebab-case strings round-trip | invalid → matching variant (Empty / TooLong / InvalidCharacter / InvalidLeadingCharacter) |
| `AgentId` | proptest | same body as PackageName | same |
| `Message` envelope | proptest + serde_json | arbitrary `Message` round-trips through JSON | unknown payload variants → typed deserialization error |
| `Value` (recursive) | proptest, max depth 16 | nested values round-trip through JSON | — |
| `PackageManifest` (table-driven, NOT proptest) | hand-picked malformed manifests | — | each malformed input → specific `PackageManifestError` variant |

The manifest validation suite is intentionally **table-driven, not property-based** — see §5 (testing) for rationale.

### What is NOT a parser at v0.1

- `Version`, `VersionReq` — re-export `semver`'s parser.
- TOML manifest deserialization — owned by tau-pkg.
- `MessageId`, `AgentInstanceId` (UUIDs) — re-export `uuid`'s parser.
- `url::Url` — re-export the `url` crate's parser; only validate scheme-set on top.

---

## 5. Testing strategy

Per QG5 (four mandatory layers + proptest for parsers) and the §7 refinement.

### Layers

**Unit tests, inline in `src/*.rs`:**
- Per ID newtype: `PackageName::try_from` / `AgentId::try_from` cover all error variants by example.
- Per error enum: `Display` impls render readable text (one assertion per variant).
- `Capability` typed-vs-Custom dispatch fixture vector — exercise each pattern arm.
- `Value` accessor helpers — round-trip and shape assertions.
- `AgentStatus` Display + serde — one round-trip per variant.

**Integration tests, in `tests/`:**
- `manifest_roundtrip.rs` — TOML manifest → `UncheckedManifest::deserialize` → `validate()` → re-serialize → compare.
- `manifest_validation_table.rs` — table-driven, ~5–8 hand-picked malformed manifests, each asserting a specific `PackageManifestError` variant. Replaces the previously-considered `manifest_validation_consistency` proptest.
- `message_envelope_serde.rs` — Message/MessagePayload JSON round-trip across all variants.
- `package_source_grammar.rs` — table-driven cases (URL, scp, with/without rev, malformed) → expected outcome.

**Doc tests** (mandatory per QG9 + `#![deny(missing_docs)]`):
- Every public item has at least one example in rustdoc.
- Heaviest doc-test surface: ID constructors, `Value` accessors, `MessagePayload::Custom` example, `UncheckedManifest::validate` example.
- Pure Rust only (tau-domain has no I/O) — no `#[ignore]` needed.

**Property tests (`proptest`, dev-dep):**
- `package_source_roundtrip` — generated valid `PackageSource` round-trips via `Display` + `from_str`.
- `package_source_rejection` — malformed strategies map to specific `PackageSourceError` variants.
- `package_name_grammar` / `agent_id_grammar` — separate proptests despite identical grammar (clearer failure messages).
- `message_envelope_roundtrip` — `Message` ↔ JSON via serde_json.
- `value_roundtrip` — nested `Value` ↔ JSON, max depth 16.

### Wire-format golden tests (`tests/wire_format/`)

Files containing canonical serialized forms of each public type / variant:

```
tests/wire_format/
├── message_text.json
├── message_tool_call.json
├── message_tool_result.json
├── message_tool_error.json
├── message_lifecycle.json
├── message_custom.json
├── manifest_minimal.toml
├── manifest_with_capabilities.toml
├── package_source_https.txt
├── package_source_scp.txt
└── package_source_with_rev.txt
```

For each: read file → deserialize → re-serialize → byte-compare.

**Why:** G5 (stable message schema) + G6/QG12 (serve-mode IPC schema = public surface). A `derive(Serialize)` change (renamed field, switched tagging) is a wire-breaking change disguised as a type-level edit. Golden files turn that into a noisy PR-time test failure. ~10 small files; high signal, near-zero maintenance once stable.

### `--no-default-features` build check (CI)

One CI job: `cargo build -p tau-domain --no-default-features` (and optionally `cargo test` on the non-serde paths). Proves the off-by-default `serde` feature claim.

### Escape-hatch registry coverage test (`tests/escape_hatch_registry.rs`)

A workspace-level integration test that mechanically enforces the rule from ADR-0002 (§6 bullets 5 + 7): every escape-hatch variant in the source must have a corresponding entry in `docs/explanation/escape-hatches.md`, and every registry entry with status `active` must point at a real source variant.

Implementation:
1. Walk `crates/**/*.rs` (workspace-relative). Find every `enum` variant whose name is `Custom` or `InternalError` (the agreed escape-hatch naming convention).
2. For each variant, parse its rustdoc comment for a link of the form `escape-hatches.md#<anchor>`.
3. Read `docs/explanation/escape-hatches.md`. Parse the anchors (HTML `<a id="...">` tags inside the table).
4. Assert: every source variant has a rustdoc link with an anchor that matches a registry row.
5. Assert: every registry row with status `active` points to an anchor that resolves to a live source variant (catches stale entries).

Test failures are CI-blocking. Adds `walkdir` and `pulldown-cmark` (or a tiny hand-rolled parser) as dev-dependencies of tau-domain.

**Why this lives in tau-domain:** at v0.1, tau-domain is the only crate with escape hatches. As tau-ports / tau-runtime / tau-pkg add them, the test scans the whole workspace via `..` so it stays in tau-domain. If the workspace grows enough to warrant a dedicated tooling crate, migrate the test to `xtask/`.

### `test-fixtures` feature

```toml
[features]
test-fixtures = []
```

Exposes `pub mod fixtures` with construction helpers:

```rust
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures {
    pub fn any_package_name() -> PackageName;
    pub fn any_agent_id() -> AgentId;
    pub fn any_package_source() -> PackageSource;
    pub fn any_unchecked_manifest() -> UncheckedManifest;
    pub fn any_package_manifest() -> PackageManifest;
    pub fn any_agent_definition() -> AgentDefinition;
    pub fn any_message() -> Message;
}
```

Downstream crates depend via `tau-domain = { workspace = true, features = ["test-fixtures"] }` in `[dev-dependencies]`. Production builds don't pull the fixtures (zero cost).

### Out-of-scope at v0.1

- Numeric coverage thresholds (constitution's "every public behavior has a test" replaces them).
- `cargo-mutants` mutation testing (Phase 2+).
- Fuzz targets (QG5 mandates fuzz for the IPC protocol — that lives in tau-runtime).
- `compile_fail` doctests for the typestate guarantee (revisit if `PackageManifest` accidentally gets a `Deserialize` impl in code review).

---

## 6. ADR-0002 — manifest format & capability evolution

ADR-0002 is filed in `docs/decisions/0002-manifest-format.md` as part of this sub-project (NOT a separate sub-project).

### What it records

1. **Manifest field set at v0.1.** The fields enumerated in §3.5.2 — name, version, description, authors, license, source, kind, dependencies, capabilities. Adding fields is a non-breaking minor; removing or renaming is a breaking minor (pre-1.0) per QG11.
2. **Hierarchical Capability shape (β).** Filesystem / Network / Process / Agent / Custom at the top level; per-namespace verb enums underneath. Per-variant `#[non_exhaustive]` for additive field evolution.
3. **Canonicalization-at-deserialization commitment.** Manifest TOML uses flat dot-namespaced `kind = "fs.read"` form; a custom `Deserialize` impl for `Capability` maps it to the variant tree. New typed variants in v0.X auto-promote existing `Custom { name: "...", params }` manifests via the same canonicalization — plugin authors never have to update manifests when typed variants land.
4. **Dot-namespaced naming convention** as **recommended, not mandated**. The convention (`<domain>.<verb>`, e.g., `fs.read`, `net.http`) is documented in ADR-0002 and rustdoc; tau-domain validates only "non-empty" — plugin authors who want a non-conforming name (e.g., `myorg/special-cap`) can use `Custom`.
5. **Escape-hatch policy (typed-vs-Custom).** Prefer typed variants for known shapes; allow `Custom` / `InternalError` escape hatches with documented rationale. **Every escape hatch in tau core is tracked in `docs/explanation/escape-hatches.md`** with location, reason, promotion trigger, and status (`active` / `promoted` / `removed`). PRs that introduce, promote, or remove an escape hatch update the registry in the same commit. Each escape-hatch variant's rustdoc carries a link to its registry anchor. Applies uniformly to `Capability::Custom`, `MessagePayload::Custom`, `PackageKind::Custom`, `FailureKind::InternalError`, and any future escape hatches added in tau core.
6. **Required `llm_backend`.** Recorded with the rationale (Constitution Appendix C; G4) and the loosen-later-via-minor-bump escape if needed.
7. **Mechanical enforcement of the registry.** A CI-blocking integration test (`crates/tau-domain/tests/escape_hatch_registry.rs`, see §5) scans every `.rs` file in the workspace for variants named `Custom` or `InternalError`, parses their rustdoc for a link to the registry, and verifies each anchor exists in the registry file. Stale registry entries (rows whose anchor no longer maps to a live source variant) also fail the test. Combined with a PR template checkbox and the rustdoc convention, this enforces the policy in three layers: documentation (CONTRIBUTING.md + rustdoc), PR-time prompt (template), and CI gate (test).

### Status timeline

- ADR-0002 lands in the same PR (or PR series) as the tau-domain implementation.
- ADR-0002 is "Accepted" before Plan 2 closes (mirrors Plan 1's ADR-0001 timing).

---

## 7. Commit / sub-task strategy

The implementation plan derived from this spec follows the same one-commit-per-task pattern as Plan 1. Anticipated task ordering:

1. Workspace deps + crate dep update (touches `Cargo.toml` workspace + `crates/tau-domain/Cargo.toml`).
2. ID newtypes + their errors (`id.rs` + `error.rs` partial).
3. `Value` + accessors.
4. Re-export `semver`, `url`, `uuid` types from `lib.rs`.
5. `PackageSource` + `GitLocation` + parser + `PackageSourceError`.
6. `PackageDep`, `PackageId`, `PackageKind` + `kinds` module + `PackageKindError`.
7. `Capability` hierarchy (`Filesystem`/`Network`/`Process`/`Agent`/`Custom`) + custom `Deserialize` for canonicalization.
8. `UncheckedManifest` + `PackageManifest` typestate + `validate()` + `PackageManifestError`.
9. `AgentStatus` + `FailureKind` + `AgentDefinition` (basic).
10. `Message` envelope + `Address` + `MessagePayload`.
11. `test-fixtures` module.
12. Proptest suite (one commit per parser).
13. Integration tests (`manifest_roundtrip`, `manifest_validation_table`, `message_envelope_serde`, `package_source_grammar`).
14. Wire-format golden tests.
15. CI: add `--no-default-features` job.
16. Seed escape-hatch registry (`docs/explanation/escape-hatches.md`) with v0.1 entries (`Capability::Custom`, `MessagePayload::Custom`, `PackageKind::Custom`, `FailureKind::InternalError`).
17. Implement registry-coverage test (`crates/tau-domain/tests/escape_hatch_registry.rs`) + PR template (`.github/pull_request_template.md`) + CONTRIBUTING.md note on the escape-hatch policy.
18. ADR-0002 — manifest format & capability evolution (incorporates the escape-hatch policy + registry reference).
19. Final local verification + ADR sign-off.

The plan-writing skill (writing-plans) decomposes each into discrete, individually-committable steps.

---

## 8. Risks & rollbacks

| Risk | Mitigation |
|---|---|
| Wire format committed too early; serde derive change ships a wire break | Golden-file tests (§5) catch at PR review. ADR-0002 records canonicalization rules. |
| Capability typed-variant set is wrong (e.g., we typed `FsCapability::Read` but later need separate `FsCapability::ReadAttr`) | Variant-level `#[non_exhaustive]` permits additive field evolution. Wrong variants are pre-1.0 minor breaks per QG11. |
| `FailureKind::InternalError` becomes a grab-bag over time | ADR-0002's promotion rule: ≥2 distinct shapes → propose typed variant via ADR. tau-runtime telemetry watches for this. |
| Typestate (`Unchecked → Checked`) confuses plugin authors | Heavy doctests on `UncheckedManifest::validate` + a dedicated example in `docs/explanation/` after sub-project 1 ships. |
| `url::Url`'s scheme-set is too loose (e.g., `file://` accepted) | `GitLocation::Url` validates allowed schemes (`https/http/ssh/git`); rejects others with `PackageSourceError::UnsupportedScheme`. |
| `cargo build --no-default-features` fails because something accidentally hard-imports `serde` | CI job catches this on every PR. |
| `serde` feature changes at workspace level break a downstream crate | tau-domain's `serde` is its own feature; downstream crates explicitly opt in. Workspace-wide flips don't accidentally enable. |

Rollback: any single sub-task's commit is independently revertable. The plan's ordering (deps → IDs → values → individual modules) is dependency-bottom-up; reverting a higher-numbered task doesn't break lower ones.

---

## 9. Handoff to writing-plans

Inputs to the next stage:

- **This spec.**
- **`CONSTITUTION.md`** — guidelines G1–G17, NG1–NG12, QG1–QG25, PG1–PG5.
- **`ROADMAP.md` row 1** — sub-project 1 scope summary.
- **Plan 1** (`docs/superpowers/plans/2026-04-24-repo-bootstrap.md`) for committed format conventions.
- **ADR-0001** for the typed-error / forbid-unsafe / strict-clippy posture.

The plan should:
1. Decompose §7's task list into discrete steps (each individually committable).
2. Specify the test invocations after each step that prove the step lands cleanly.
3. Include the workspace-deps step as Task 1 (a workspace-level change that gates everything below).
4. End with: a "final verification" task (mirrors Plan 1 Task 16), an ADR-0002 task, and a QG22 overnight checkpoint.

After plan acceptance, hand off to `superpowers:executing-plans` (or `subagent-driven-development`) for implementation.
