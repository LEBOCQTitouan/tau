# Escape-hatch registry

Each entry below names a place where tau core uses a structural escape
hatch (`Custom`, `InternalError`) instead of typed variants. Per
ADR-0002, every escape hatch must be documented here with rationale
and promotion trigger.

**PR rule:** any PR that introduces, promotes, or removes an escape
hatch updates this file in the same commit. The CI test
`crates/tau-domain/tests/escape_hatch_registry.rs` enforces this
mechanically.

## Active escape hatches

| Anchor | Location | Reason | Promotion trigger | Sub-project |
|---|---|---|---|---|
| <a id="capability-custom"></a>`capability-custom` | `Capability::Custom { name, params }` | Capability vocabulary not yet typed; tau-runtime hasn't determined which capabilities need typed variants beyond the v0.1 set (Filesystem/Network/Process/Agent). | When tau-runtime ships namespace enforcement for a new namespace (sub-project 4+), promote the namespace's verbs to typed variants. | 1 |
| <a id="messagepayload-custom"></a>`messagepayload-custom` | `MessagePayload::Custom { kind, body }` | Plugin-specific message kinds (MCP resources, skill-specific shapes) not yet enumerated. | When MCP plugin trait stabilizes (sub-project 2+), promote `mcp.*` shapes; same for skill-specific message kinds. | 1 |
| <a id="packagekind-custom"></a>`packagekind-custom` | `PackageKind::Custom { kind }` | All package kinds go through `Custom` at v0.1; no typed variants exist. | When tau-ports lands plugin traits for LLM/Tool/Storage/Sandbox (sub-project 2), consider promoting matching `PackageKind` variants. | 1 |
| <a id="failurekind-internalerror"></a>`failurekind-internalerror` | `FailureKind::InternalError` | Catch-all for failures not matching the v0.1 typed kinds (Crashed, BackendError, PolicyDenied, OutOfResources). tau-runtime hasn't yet emitted enough variety to identify recurring shapes. | When tau-runtime construction sites for `InternalError` exceed 3 distinct contexts, file an ADR proposing typed variants for the recurring shapes. | 1 |

## Promoted escape hatches

(none yet)

## Removed escape hatches

(none yet)
