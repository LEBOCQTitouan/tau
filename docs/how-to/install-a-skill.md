# How to install a skill

## From a git URL

    $ tau install https://github.com/owner/some-skill

If the source has a `tau.toml`, tau installs it as a tau-native
skill. If the source has only `SKILL.md` (vanilla Anthropic Agent
Skill), tau auto-detects and synthesizes a `tau.toml` in-memory and
on disk. Lockfile records the provenance.

## From a local path

    $ tau install ./skills/critic

Local paths work like git URLs but skip the clone step. Useful when
developing a skill or shipping one with your project.

## From a `file://` URL

    $ tau install file:///path/to/skill

Same as a git URL but explicit. Useful for testing the git-clone code
path against local paths.

## Customize before installing (Anthropic format only)

    $ tau skill import https://github.com/owner/anthropic-skill \
        --output ./my-skill

Clones the source + writes a synthesized `tau.toml` next to the
SKILL.md. Edit `./my-skill/tau.toml` (e.g. add capabilities), then:

    $ tau install ./my-skill

## Verify the install

    $ tau skill list
    Name      Version  Source
    ──────────────────────────────────────
    critic    0.1.0    https://github.com/...

    $ tau skill show critic
    Name: critic
    Version: 0.1.0
    Description: Reviews drafts for clarity, completeness, and
                 rhetorical quality.
    Capabilities: (none)

## Uninstall

    $ tau uninstall critic
