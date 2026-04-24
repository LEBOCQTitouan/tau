# tau

[![CI](https://github.com/LEBOCQTitouan/tau/actions/workflows/ci.yml/badge.svg)](https://github.com/LEBOCQTitouan/tau/actions/workflows/ci.yml)

> Tau installs and runs agents in the terminal — solo or orchestrated,
> globally or per-project — with skills, tools, MCP servers, LLM
> backends, and pipelines provided as installable packages.

**Status:** Phase 0 — bootstrap. Nothing to install yet. See
[`ROADMAP.md`](ROADMAP.md) for what is shipping next.

## What tau is

Tau is a minimal, terminal-native Rust runtime. Core does four things
(G1):

1. Installs packages.
2. Runs agents (solo or orchestrated).
3. Passes messages between entities.
4. Observes what happens.

Everything domain-specific — LLM backends, tools, pipelines, skills,
MCP servers, SDKs — is a package. Core ships empty (G11).

The full charter is in [`CONSTITUTION.md`](CONSTITUTION.md). Every
decision about what tau is and how it is built defers to that
document. A one-line summary of all 59 guidelines is in
[`GUIDELINES_CHEATSHEET.md`](GUIDELINES_CHEATSHEET.md).

## What tau is not

Tau is not an LLM, not a hosted service, not a package marketplace, not
a workflow engine, not an AI safety harness, not a credential manager.
See [`CONSTITUTION.md` §2](CONSTITUTION.md) for the full list of 12
non-goals.

## Building

Bootstrap-only at the moment:

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo run -p tau-cli  # prints "tau"
```

Requires Rust 1.91 or newer (MSRV).

## Documentation

Documentation follows [Diátaxis](https://diataxis.fr). See
[`docs/`](docs/) for the structure. Architecture Decision Records
(ADRs) live in [`docs/decisions/`](docs/decisions/).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Contributions must align with
the constitution; alignment is the bar, not LLM-vs-human provenance
(PG2).

## License

Dual-licensed under either of:

- Apache License 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([`LICENSE-MIT`](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Contributions submitted for inclusion are dual-licensed as above without
additional terms or conditions, per the Apache 2.0 §5 inbound=outbound
norm.
