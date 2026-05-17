# How to author a skill

## Minimal skill (pure prompt)

Directory layout:

    my-skill/
    ├── SKILL.md
    └── tau.toml

`SKILL.md`:

    ---
    name: my-skill
    description: One-line description of what this skill does.
    ---

    [System prompt body — Markdown.]

`tau.toml`:

    name = "my-skill"
    version = "0.1.0"
    description = "One-line description of what this skill does."
    authors = ["you"]
    source = "https://github.com/you/my-skill.git"
    kind = "skill"
    dependencies = []
    capabilities = []

    [skill]

The `name` fields in SKILL.md frontmatter and tau.toml MUST match.

## Adding capabilities

Capabilities give your skill access to things beyond the LLM (files,
network, processes). Declare them in `tau.toml`:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

    [[capabilities]]
    kind = "net.http"
    hosts = ["api.example.com"]
    methods = ["GET"]

    [[capabilities]]
    kind = "process.spawn"
    commands = ["git", "rg"]

`${SKILL_DIR}` resolves at runtime to the skill's installed path. Use
it for paths that live inside your skill's directory.

For the full set of capability kinds, see
[Reference: skill manifest schema](../reference/skill-manifest-schema.md).

## Bundling reference files

Anything in your skill directory ships with the package on install.
Example:

    fact-checker/
    ├── SKILL.md
    ├── tau.toml
    └── references/
        ├── style-guide.md
        └── common-claims.md

In `tau.toml`, grant fs.read access to the bundled directory:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

In `SKILL.md`, reference the files by their relative path:

    Use the bundled references at `references/` to validate claims.

The runtime substitutes `${SKILL_DIR}` with the actual install path
when spawning, so the skill agent reads the right files.

## Declaring sub-skill dependencies

If your skill is meant to be invoked alongside another skill, declare
it:

    [skill]

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

This is advisory in tau v1 — the runtime doesn't auto-spawn
sub-skills. It documents the relationship for users who want to
install the dependencies together.

## Versioning

tau uses semver in `tau.toml`'s `version` field. Bump it when you
publish a new version:

- **Patch** (0.1.0 → 0.1.1): SKILL.md text fixes, doc updates.
- **Minor** (0.1.0 → 0.2.0): new capabilities, new bundled files.
- **Major** (0.1.0 → 1.0.0): breaking changes to the skill's
  contract (e.g., it now requires capabilities it didn't before).

## Testing your skill

Install your skill into a tempdir scope:

    $ mkdir /tmp/test-scope && cd /tmp/test-scope
    $ mkdir .tau && echo 'schema_version = 3' > .tau/config.toml
    $ tau install /path/to/my-skill
    > Installed my-skill@0.1.0

Then invoke it (depending on how your agent is configured):

    $ tau skill show my-skill --body --raw
