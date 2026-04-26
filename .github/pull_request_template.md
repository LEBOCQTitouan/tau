## Summary

<short summary of the change>

## Test plan

- [ ] Local `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-features` pass.
- [ ] If CI behavior changes, the workflow file is updated and validated.

## Escape-hatch checklist

- [ ] This PR does not add, modify, or remove a `Custom` / `InternalError` escape hatch, OR
- [ ] `docs/explanation/escape-hatches.md` is updated with the corresponding entry (added / promoted / removed). The CI registry-coverage test (`crates/tau-domain/tests/escape_hatch_registry.rs`) enforces this.

## ADR check

- [ ] This PR does not require an ADR per QG18, OR
- [ ] An ADR has been filed in `docs/decisions/` and is referenced here.
