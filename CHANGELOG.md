# Changelog

All notable changes to tau are recorded here. Format:
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/). Tau
follows [Semantic Versioning](https://semver.org); pre-1.0, breaking
changes bump the minor (`0.X.Y` → `0.(X+1).0`) per QG11.

## [Unreleased]

### Added

- Initial repository bootstrap: empty Cargo workspace with eight crates
  (`tau-domain`, `tau-ports`, `tau-infra`, `tau-app`, `tau-pkg`,
  `tau-observe`, `tau-runtime`, `tau-cli`).
- `tau-cli` binary that prints `"tau"` and exits.
- Dual-license: MIT OR Apache-2.0.
- Governance files: README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT,
  GOVERNANCE, ROADMAP (QG10).
- Diátaxis documentation skeleton with ADR directory and MADR template.
- ADR-0001 recording bootstrap decisions.
- GitHub Actions CI: fmt, clippy, and test on Linux + macOS + Windows
  ×  stable + MSRV 1.91.
- Imported `CONSTITUTION.md` and `GUIDELINES_CHEATSHEET.md` as the
  project's source of truth.
- Versioned mdBook documentation site published to GitHub Pages.
  Auto-deploys on major-equivalent version tags (`v[0-9]+.0.0` or
  `v0.[0-9]+.0`). Force-publish escape hatches: `[publish-docs]`
  commit-message marker on `main` republishes `/latest/`; the
  `docs:publish` PR label publishes an ephemeral preview to
  `/preview/pr-NNN/`. See ADR-0027.

### Changed

- Nothing yet.

### Deprecated

- Nothing yet.

### Removed

- Nothing yet.

### Fixed

- Nothing yet.

### Security

- Nothing yet.

[Unreleased]: https://github.com/LEBOCQTitouan/tau/commits/main
