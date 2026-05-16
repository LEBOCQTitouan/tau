# How to export a skill

## Why export?

`tau skill export` produces a directory in vanilla Anthropic Agent
Skills format — `SKILL.md` plus any bundled content files, no
`tau.toml`. Useful when:

- Sharing a skill with users who run claude-code or another
  Anthropic-format consumer.
- Submitting to a public skill repository.
- Distributing a skill without exposing tau-specific capability
  declarations.

## Basic export

    $ tau skill export critic --output ./out
    > Exported critic to ./out

The `./out/` directory now contains a vanilla Anthropic skill.

## Capability-bearing skills

If the skill declares capabilities, they're dropped from the export
(Anthropic format doesn't preserve them). You'll see a warning:

    $ tau skill export fact-checker --output ./out
    note: 1 capabilities dropped on Anthropic export (fs.read);
          Anthropic format does not preserve capability declarations

The export still succeeds (exit code 0). The dropped capability is
informational.

## Refuse-on-drop with `--strict`

If you want the export to fail rather than silently drop metadata:

    $ tau skill export fact-checker --output ./out --strict
    error: would drop metadata: ["fs.read"] (skill "fact-checker");
           remove --strict to proceed with a warning

Useful in CI to prevent accidental information loss when an
Anthropic-compatible export is required.

## Overwrite existing output

By default, `--output` refuses to overwrite an existing directory:

    $ tau skill export critic --output ./out
    error: output directory "./out" already exists; pass --force to overwrite

Add `--force` to overwrite:

    $ tau skill export critic --output ./out --force
    > Exported critic to ./out

## Roundtrip guarantee

For capability-less skills (`capabilities = []` + no `requires_skills`),
`tau skill export` produces a byte-identical SKILL.md to the original
source. Multi-file payloads (e.g., `references/` subdirs) are
preserved verbatim.

For capability-bearing skills, the export is one-way: re-importing
won't restore the dropped capabilities. Document any capability
declarations separately if you want round-trippable distribution.
