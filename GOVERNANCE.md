# Governance

## Model

Tau is a solo-maintainer project. The maintainer (Titouan Lebocq) makes
all final decisions about scope, design, and what merges. This is
recorded explicitly because it has implications for response times,
risk concentration, and the bar for accepting outside contributions.

Solo maintenance is **provisional**. When a second maintainer joins,
this file must be amended via an ADR that:

1. Names the second maintainer.
2. Defines the decision process for disagreement (e.g. "consensus, or
   the original maintainer breaks ties").
3. Defines the bus-factor mitigation (key rotation, signing key
   handoff, repo admin transfer plan).

Until that ADR exists, treat the bus factor as 1 — relevant for
downstream consumers planning long-term dependencies on tau.

## Decision rights

| Decision class | Who decides | How recorded |
|---|---|---|
| Bug fix, refactor within a crate, docs update | Maintainer or contributor | Commit message + PR (PG3) |
| New feature, public-API addition or break, protocol change, manifest change, plugin trait change, guideline change | Maintainer | ADR in `docs/decisions/` (QG18) |
| Release | Maintainer | CHANGELOG entry + git tag (QG21) |
| Security disclosure | Maintainer + reporter | GitHub Security Advisory (SECURITY.md) |

## Amending the constitution

Per [`CONSTITUTION.md` §4](CONSTITUTION.md), the constitution changes
only via ADRs. ADRs that propose guideline changes:

1. Explain what guideline is being added, modified, or removed.
2. Explain the situation that motivated the change.
3. State the replacement text explicitly.
4. Reference any PRs, issues, or retrospectives that contributed.

For a solo-maintainer project the maintainer decides; in the
overnight-delay spirit of QG22, guideline-changing ADRs wait at least
24 hours between drafting and merging, except for typo or formatting
corrections.

## Code of conduct enforcement

Reports go to the address in [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
The maintainer enforces. With one maintainer there is no escalation
path; if the report concerns the maintainer, escalate to GitHub Trust &
Safety (<https://support.github.com/contact/report-abuse>).

## License changes

Tau is dual-licensed MIT OR Apache-2.0. Relicensing requires consent
from every contributor whose code is still in the tree. Tau accepts
contributions only under inbound=outbound (Apache 2.0 §5), which
preserves this option for a future relicense ADR but does not pre-grant
it.

## Forks

Forks are welcome under either of the project licenses. The "tau"
trademark — to the extent one exists — is held by the maintainer; forks
should choose a different name to avoid downstream confusion.
