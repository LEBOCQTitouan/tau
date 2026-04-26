# ADR-0002: Manifest format, capability evolution, and escape-hatch policy

**Status:** Accepted
**Date:** 2026-04-26
**Supersedes:** —

## Context

tau-domain (sub-project 1) introduces the package manifest type
(`PackageManifest`), a hierarchical capability enum (`Capability`),
and four escape-hatch variants (`Capability::Custom`,
`MessagePayload::Custom`, `PackageKind::Custom`,
`FailureKind::InternalError`).

Per QG18, public-API additions to data shapes that downstream plugins
will deserialize require an ADR. This ADR records the v0.1 manifest
field set, the rules under which the `Capability` typed enum evolves,
and the policy governing all escape-hatch variants in tau core.

## Decision

### 1. Manifest field set (v0.1)

The v0.1 `UncheckedManifest` / `PackageManifest` carries:

- `name: PackageName`
- `version: semver::Version`
- `description: String` (non-empty; enforced by `validate()`)
- `authors: Vec<String>`
- `license: Option<String>` (SPDX expression as opaque text)
- `source: PackageSource`
- `kind: PackageKind`
- `dependencies: Vec<PackageDep>`
- `capabilities: Vec<Capability>`

Adding fields is a non-breaking minor (the struct is `#[non_exhaustive]`).
Removing or renaming is a breaking minor pre-1.0 per QG11.

### 2. Hierarchical Capability shape

`Capability` is a top-level `#[non_exhaustive]` enum with five variants:
`Filesystem(FsCapability)`, `Network(NetCapability)`,
`Process(ProcessCapability)`, `Agent(AgentCapability)`, and
`Custom { name, params }`. Each per-namespace sub-enum is also
`#[non_exhaustive]`, and each variant within (e.g. `FsCapability::Read`)
is `#[non_exhaustive]` so additive field evolution is non-breaking.

### 3. Canonicalization at deserialization

Manifest TOML uses the flat dot-namespaced form:

```toml
[[capabilities]]
kind = "fs.read"
paths = ["${PROJECT}/**"]
```

A custom `Deserialize` impl on `Capability` maps recognized `kind`
strings (`"fs.read"`, `"fs.write"`, `"net.http"`, `"process.spawn"`,
`"agent.spawn"`, etc.) onto the variant tree. Unknown `kind` values
fall through to `Capability::Custom { name, params }`. New typed
variants in v0.X auto-promote existing manifests via the same
canonicalization — plugin authors do not need to update manifests
when typed variants land.

A consequence of canonicalization-at-deserialization is that the
shadowed-name case round-trips into the typed variant: a
`Capability::Custom { name: "fs.read", params: ... }` constructed in
Rust serializes to `kind = "fs.read"` and re-deserializes as the
typed `Capability::Filesystem(FsCapability::Read { ... })`. This
is intentional and supports the v0.X auto-promotion path above.
Reserved typed param names (`paths`, `max_bytes`, `hosts`, `methods`,
`commands`, `allowed_kinds`) should therefore not be reused inside
`Capability::Custom.params` for unrelated meanings.

### 4. Naming convention

The dot-namespaced `<domain>.<verb>` convention (e.g. `fs.read`,
`net.http`) is **recommended, not mandated**. tau-domain validates
only "non-empty" on `Capability::Custom.name`. Plugin authors who
want a non-conforming name (e.g. `myorg/special-cap`) may use it via
`Custom`.

### 5. Escape-hatch policy

Prefer typed variants for known shapes; allow `Custom` /
`InternalError` escape hatches with documented rationale. **Every
escape hatch in tau core is tracked in
`docs/explanation/escape-hatches.md`** with location, reason,
promotion trigger, and status. PRs that introduce, promote, or remove
an escape hatch update the registry in the same commit. Each
escape-hatch variant's rustdoc carries a link to its registry anchor.

This applies uniformly to:
- `Capability::Custom`
- `MessagePayload::Custom`
- `PackageKind::Custom`
- `FailureKind::InternalError`
- any future escape hatches added in tau core.

### 6. Required `llm_backend`

`AgentDefinition.llm_backend` is `PackageName` (non-optional) at v0.1.
Rationale: Constitution Appendix C explicitly defines an agent as a
process with an LLM backend; G4 reinforces. If a non-LLM agent use
case materializes (sub-project 4+), `llm_backend` becomes
`Option<PackageName>` via a pre-1.0 minor breaking change.

### 7. Mechanical enforcement of the registry

A CI-blocking integration test
(`crates/tau-domain/tests/escape_hatch_registry.rs`) scans every
`.rs` file in the workspace for variants named `Custom` or
`InternalError`, parses their rustdoc for a link to the registry,
and verifies each anchor exists in `docs/explanation/escape-hatches.md`.
Stale registry entries also fail the test. Combined with the PR
template checkbox and the rustdoc convention, this enforces decision
5 in three layers: documentation (CONTRIBUTING.md + rustdoc), PR-time
prompt (template), and CI gate (test).

### 8. `Value::Bytes` wire format

`Value::Bytes(Vec<u8>)` (the message-payload byte-vector variant) is
serialized in human-readable formats (TOML, JSON) as a base64 string
prefixed with the reserved sentinel `@bytes:`. The prefix lets the
custom `Deserialize` distinguish a byte-payload string from an
ordinary `Value::String`. Consequently, `@bytes:` is a reserved
prefix on `Value::String` — strings starting with `@bytes:` are
not representable as `Value::String` and must use `Value::Bytes`.
Binary formats (e.g. bincode) carry bytes natively and ignore this
convention.

## Consequences

- The wire format for `Capability` is committed at v0.1: flat
  `kind = "<dot.namespaced>"` form with sibling field-shaped params.
  Deviating from this form is a breaking change.
- Plugin authors need to know the canonical names (`fs.read`, etc.)
  to produce manifests that map to typed variants; they don't *have*
  to use them — `Custom` always works.
- The escape-hatch registry is a living document; every PR that
  introduces or modifies an escape hatch touches it.
- The CI registry test depends on the rustdoc convention of linking
  to `escape-hatches.md#<anchor>`. Variants that violate this fail
  CI.
- Adding new typed `FailureKind` variants is non-breaking (the enum
  is `#[non_exhaustive]`); demoting `InternalError` to a typed kind
  is similarly additive.
- Required `llm_backend` rules out non-LLM agents at v0.1; we accept
  the migration cost if and when that case appears.

## Alternatives considered

- **String-only capability dispatch** (rejected at brainstorm). Forces
  every consumer to write `match name.as_str()` boilerplate; gives up
  type safety for no v0.1 benefit.
- **Flat `Capability` enum without namespace structure** (α; rejected
  in favor of β). Would have been more compact at v0.1 but β anticipates
  per-namespace enforcement layers in tau-runtime.
- **No escape-hatch registry**, just rustdoc warnings (rejected).
  Aspirational rules without mechanical enforcement decay quickly.
- **Top-level `DomainError` umbrella** (rejected at brainstorm Q5).
  Per-concern enums forever; consumers wanting "any tau-domain error"
  wrap their own.
- **`Option<PackageName>` for `llm_backend` from day one** (rejected).
  Adds permanent ceremony at every reader for a case that may not
  materialize; pre-1.0 SemVer makes the loosen-later path cheap.
