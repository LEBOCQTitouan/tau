# Reference skill packages

This directory ships exemplary tau skill packages with the tau
source tree. Each one demonstrates a different capability axis so
the value of tau's skill system is concrete:

| Skill | Capability axis | Demonstrates |
|---|---|---|
| [critic](critic/) | none (pure prompt) | Anthropic-format roundtrip; capability-less skills |
| [fact-checker](fact-checker/) | `fs.read` | `${SKILL_DIR}` substitution; multi-file payload (`references/`) |
| [pr-reviewer](pr-reviewer/) | `process.spawn` | Sandbox-compatible process spawning (git + rg) |

## Install

From the tau repo root, after `cargo build --release`:

    ./target/release/tau install ./skills/critic
    ./target/release/tau install ./skills/fact-checker
    ./target/release/tau install ./skills/pr-reviewer

Verify with:

    ./target/release/tau skill list

## Use

Once installed, an agent can spawn a skill as a child agent if it
has the `skill.spawn` capability granting the skill's name:

    [[agents.reviewer.capabilities]]
    kind = "skill.spawn"
    allowed_skills = ["critic", "pr-reviewer"]

The agent then emits `skill.critic.spawn` or `skill.pr-reviewer.spawn`
as a tool call and tau spawns a child agent with the skill's
declared prompt + capabilities.

## Documentation

- [Tutorial: build your first skill](../docs/tutorials/build-your-first-skill.md)
- [How-to: install a skill](../docs/how-to/install-a-skill.md)
- [How-to: author a skill](../docs/how-to/author-a-skill.md)
- [How-to: export a skill](../docs/how-to/export-a-skill.md)
- [Reference: skill manifest schema](../docs/reference/skill-manifest-schema.md)
- [Explanation: two-layer skills](../docs/explanation/two-layer-skills.md)
