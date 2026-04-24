# Security policy

## Reporting a vulnerability

Please report security vulnerabilities via **GitHub private security
advisories** rather than public issues:

<https://github.com/LEBOCQTitouan/tau/security/advisories/new>

Include:

- A description of the vulnerability and its impact.
- Steps to reproduce.
- Affected versions (commit SHA or tag).
- Any proposed mitigation.

## Response

This is a solo-maintainer project. Response is best-effort; expect
acknowledgement within a week. If the report is confirmed, the
maintainer will:

1. Work with you privately to develop a fix.
2. Prepare a coordinated disclosure timeline (typically 30–90 days
   depending on severity).
3. Issue a fix release.
4. Publish a GitHub Security Advisory; if severity warrants, request
   a CVE through GitHub.

## Scope

Security issues in tau core and the published `tau-runtime` crate are
in scope. Issues in third-party packages installed via `tau install`
should be reported to those packages' maintainers — tau does not
mediate disclosure for ecosystem packages (NG4, NG7).

Per the constitution, tau is not an AI safety harness (NG8). Reports
about agent output quality, alignment, or truthfulness are out of
scope; please direct them to the agent author or the LLM backend
provider.

## Supply chain

`cargo audit` and `cargo-deny` are scheduled for Phase 2 (QG16). Until
then, dependency vulnerabilities may be reported through this channel
even if they would normally be flagged by automation.
