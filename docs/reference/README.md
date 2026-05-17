# Reference

Information-oriented documentation: precise, factual descriptions of
tau's interfaces, formats, and protocols.

Reference pages are *complete* and *neutral*. They don't teach and they
don't argue — they tell you exactly what a field, a flag, or a protocol
message contains. Reach for them when you already know what you're
looking for and want to confirm a fact without scrolling through prose.

Reference content split:

- **Authored reference** (this section) — schemas, protocols, and
  policies that need narrative framing and stable URLs.
- **Generated reference** — CLI usage (from `clap`) and config/schema
  JSON (from `schemars`) is produced by CI per QG8 and published
  alongside each release. The authored tree does not duplicate it.

## Pages

- [Package manifest schema](package-manifest-schema.md) — full
  schema for `tau.toml`: top-level fields, `[plugin]`, `[sandbox]`,
  every `[[capabilities]]` variant and its payload, validation rules,
  and reserved param names.
- [Skill manifest schema](skill-manifest-schema.md) — the
  `kind = "skill"` specifics that layer on top of the package
  manifest: the `[skill]` block, `SKILL.md` frontmatter,
  `${SKILL_DIR}` substitution, and lockfile entries.
- [Sandbox platform support](sandbox-platform-support.md) — the kernel
  features required by tau's native sandbox adapter, the distros
  tested in CI, and the known limitations of the current v0.1
  enforcement.

## See also

- [Architecture decisions](../decisions/README.md) — ADRs that pin
  protocol and schema decisions.
- [`CONSTITUTION.md`](../../CONSTITUTION.md) — guidelines (G*, NG*,
  QG*, PG*) referenced from many ADRs.
- [`ROADMAP.md`](../../ROADMAP.md) — what is shipping next and what is
  explicitly out of scope.
