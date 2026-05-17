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

- [Sandbox platform support](sandbox-platform-support.md) — the kernel
  features required by tau's native sandbox adapter, the distros
  tested in CI, and the known limitations of the current v0.1
  enforcement.
- [Skill manifest schema](skill-manifest-schema.md) — complete schema
  for tau skill packages: `tau.toml` layout for `kind = "skill"`, the
  `[skill]` block, `SKILL.md` frontmatter, every capability shape, the
  `${SKILL_DIR}` substitution rules, and lockfile entries.

## See also

- [Architecture decisions](../decisions/README.md) — ADRs that pin
  protocol and schema decisions.
- [`CONSTITUTION.md`](../../CONSTITUTION.md) — guidelines (G*, NG*,
  QG*, PG*) referenced from many ADRs.
- [`ROADMAP.md`](../../ROADMAP.md) — what is shipping next and what is
  explicitly out of scope.
