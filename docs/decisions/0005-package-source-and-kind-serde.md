# ADR-0005: Custom serde for `PackageSource` and `PackageKind`

**Status:** Proposed
**Date:** 2026-04-27
**Supersedes:** —

## Context

`PackageSource` and `PackageKind` are two of the most user-facing types in tau-domain: every hand-written `tau.toml` carries them. ADR-0002 ("Manifest format, capability evolution, escape-hatch policy") established the manifest format but left the per-field serde shape implicit — the v0.1 implementation derived `Serialize`/`Deserialize` for both types via `#[derive]`.

The derived shape produces awkward TOML for two reasons:

1. **`PackageSource::Git { location: GitLocation::Url(url::Url), rev }`** with derived serde produces nested tables because `url::Url`'s serde is itself nested. A user-written manifest needed:
   ```toml
   [source]
   [source.Git]
   rev = "v0.3.0"
   [source.Git.location]
   [source.Git.location.Url]
   "https" = "_"
   # ...
   ```
   This is so verbose that the existing `tests/manifest_roundtrip.rs` integration test side-stepped TOML entirely and exercised the round-trip via `serde_json`, with a comment calling the format "finicky" and the SAMPLE TOML constant marked as documentary-only.

2. **`PackageKind::Custom { kind: String }`** is a single-variant `#[non_exhaustive]` enum at v0.1. Derived serde produces `[kind.Custom] kind = "tool"` for a value that semantically reduces to the string `"tool"`.

Per QG18, "Changes to the package manifest format" require an ADR. The format users write is a function of the serde representation, so changing the representation is a manifest format change.

The natural form a user would write is:

```toml
source = "https://github.com/x/y.git#main"
kind = "tool"
```

Both types already have the necessary infrastructure for this:

- `PackageSource: FromStr + Display` round-trips through `<location>#<rev>` (proved by `proptest_package_source.rs`).
- `PackageKind` carries an inner `kind: String` that is the canonical text.

## Decision

We replace the derived `Serialize`/`Deserialize` on `PackageSource` and `PackageKind` with custom impls that round-trip through the string form. The change applies whenever the `serde` feature is enabled.

### 1. `PackageSource` — string form via `Display`/`FromStr`

`Serialize` uses `serializer.collect_str(self)` (formats via `Display`, no allocation). `Deserialize` uses a `Visitor` that delegates to `PackageSource::from_str`, converting parse failures via `serde::de::Error::custom`. The visitor implements `visit_str` and `visit_string`.

Wire form (TOML):
```toml
source = "https://github.com/x/y.git#main"
```

Wire form (JSON):
```json
"https://github.com/x/y.git#main"
```

`GitLocation`'s derived serde is removed at the same time. `GitLocation` is `pub` but no longer reachable through the manifest's serde path; if a future caller needs to serialize a `GitLocation` directly, the derive can be added back at that time.

### 2. `PackageKind` — inner-kind string form

`Serialize` matches the variants exhaustively (legal inside the defining crate even on `#[non_exhaustive]` enums) and emits the inner `kind` field as a plain string. At v0.1 the only variant is `Custom { kind }`. `Deserialize` uses a `Visitor` that rejects empty strings with a useful serde error and constructs `Custom { kind: <input> }`.

Wire form (TOML):
```toml
kind = "tool"
```

Wire form (JSON):
```json
"tool"
```

When typed `PackageKind` variants land later (e.g., `Tool { entrypoint: String }`), the custom serde will need to distinguish between a plain-string input (current path → `Custom { kind: <s> }`) and a richer table input (new typed variant). The chosen mechanism at that point will be a custom string-or-table deserializer with a tagged-discriminator inside the table form. This decision is deferred until a typed variant is concretely needed; the current single-variant case does not require pre-emptive complexity.

### 3. ADR-0002 amendment

ADR-0002 referred to "the manifest format" without specifying serde representations. This ADR refines that: the manifest format is canonically what users write in `tau.toml`, and ADR-0005 documents the serde shapes for two of its fields. Future per-field shape changes follow the same pattern (an ADR per field-or-type whose representation changes).

## Consequences

### Positive

- Hand-written `tau.toml` files are dramatically simpler — the `source` and `kind` fields each fit on one line.
- The `tests/manifest_roundtrip.rs` integration test now exercises an actual TOML round-trip via the `SAMPLE` constant (rather than treating it as documentary). The "finicky TOML" caveat is gone.
- The wire form is symmetric with `PackageSource::FromStr`/`Display` — there is one canonical text representation, used consistently in CLI input, manifests, lockfiles, and error messages.
- Lockfile diffs become readable: a `tau-lock.toml` records `source = "https://github.com/x/y.git#main"` instead of nested tables.

### Negative

- A small forward-compatibility burden when typed `PackageKind` variants land later. The mitigation is documented in §2: a string-or-table custom deserializer at that time. The burden is local to one type and one impl.
- The derived `Serialize`/`Deserialize` on `GitLocation` is removed. If a future caller needs to serialize a `GitLocation` directly (without going through `PackageSource`), the derive must be re-added. This is YAGNI for v0.1 — the only known consumer is the manifest format, and that path now goes through `PackageSource`'s custom impl.

### Neutral / new obligations

- Any future ADR proposing a new variant on `PackageSource` or `PackageKind` must specify how the variant interacts with the string-form serde. The natural extension for `PackageKind` is a string-or-table form (described in §2). For `PackageSource`, future variants would extend the `Display`/`FromStr` grammar — e.g. a `Path` variant could serialize as `"file:///abs/path"` or similar — preserving the single-string property.
- Proptest coverage (added in `tests/proptest_package_source_serde.rs`) becomes a regression gate: any future change to the custom impls must keep the proptest passing.
- Lockfile schema unaffected at the field-list level: `LockedPackage.source: PackageSource` stays the same type, only the serialization differs. Existing `schema_version = 1` covers both before and after this ADR (no on-disk lockfile shipped in the wild yet).

## Alternatives considered

### A. Keep derived serde, document the verbose form in user-facing docs

Rejected. The derived `[source.Git.location.Url] "https" = "_"` form is unworkable as a user-facing format — it leaks `url::Url`'s internal representation, requires users to know the Rust enum variant tags, and makes hand-written manifests impractical. The cost of writing custom serde impls (one per type, ~30 lines each) is far smaller than the cost imposed on every plugin author writing a manifest.

### B. Custom serde for `PackageSource` only, leave `PackageKind` derived

Rejected. `PackageKind`'s derived form (`[kind.Custom] kind = "tool"`) is also user-facing and equally awkward. The fix is symmetric — both types have a canonical inner-string form. Doing one but not the other would leave users writing a mix of natural and verbose forms in the same `tau.toml`, which is worse than either uniform alternative.

### C. Use `serde_with`'s `DisplayFromStr` attribute

Rejected for v0.1. Adding a `serde_with` workspace dependency for two impls is over-tooling. The explicit `Visitor` impls are ~30 lines each, easy to read, and make the validation behavior (empty-string rejection on `PackageKind`, `PackageSourceError` propagation on `PackageSource`) explicit. Reconsider if a third or fourth type needs the same treatment.

### D. Field-level `#[serde(with = "...")]` on `UncheckedManifest.source` / `.kind`

Rejected. The custom (de)serialization belongs to the type, not to a particular field that uses it. Putting it on the field would force every consumer of `PackageSource` (the lockfile, future serialization paths) to repeat the attribute — and would diverge if any consumer forgot.
