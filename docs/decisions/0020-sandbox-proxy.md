# ADR-0020: Sandbox proxy — replaces F's veth+nft per-host filtering

**Status:** Accepted
**Date:** 2026-05-07
**Deciders:** Titouan Lebocq
**Supersedes:** [ADR-0019 — Per-host network filter](0019-per-host-network-filter.md)

## Context

ADR-0019 shipped F's per-host network filter (veth + nftables + CAP_NET_ADMIN-in-parent). Field experience surfaced four problems:

1. Privileged-Docker requirement in CI made tests slow and brittle
2. The 4 strict_net_filter integration tests hung in privileged Docker (suspected cmd.output() / seccomp KillProcess interaction)
3. The 3 layer4_container HTTP plugin tests couldn't be un-`#[ignore]`'d because Docker networking ≠ veth IP
4. Production tau required root or CAP_NET_ADMIN — friction for deployers

Research (sub-project H) found that Anthropic's own sandbox-runtime uses a userspace proxy pattern that avoids all four pain points. This ADR adopts that pattern.

## Decision

Replace the `tau-sandbox-native::net_filter` module wholesale with a userspace HTTP-CONNECT proxy + small bridge binary. Both Native and Container adapters share the same proxy module via a new `tau-sandbox-proxy` crate.

Architecture: a tokio task in tau's parent address space accepts CONNECT requests on a temp Unix socket file. The strict-tier child runs in `unshare(CLONE_NEWUSER | CLONE_NEWNET)` — empty netns, no internet. The `tau-net-bridge` binary brings `lo` up, listens on `127.0.0.1:8443`, splices to the inherited Unix socket file. Plugin's HTTPS_PROXY env points at the bridge's TCP listener.

Pass-through CONNECT: proxy does NOT terminate TLS. SNI in the TLS ClientHello must match the CONNECT host (closes domain-fronting hole).

## Consequences

Positive:
- Zero kernel privileges in the parent (drops CAP_NET_ADMIN requirement)
- CI runs on stock ubuntu-latest (no privileged Docker)
- 7 `#[ignore]`'d sandbox tests become runnable (4 strict_net_filter replaced by strict_proxy + 3 layer4_container HTTP plugin tests un-ignored)
- ~640 LOC of F's machinery removed; net code reduction
- F's sync-pipe machinery in tau-ports also removed (~80 more LOC + trait field)
- New shared crate tau-sandbox-proxy enables future Container/Native parity work

Negative:
- Pass-through CONNECT proxy can't enforce HTTP method/path (matches today's enforcement; future iteration if richer capabilities land)
- Non-HTTP egress (raw TCP, UDP) no longer covered (no current plugin uses; future iteration if needed)
- Container adapter's deployment story now requires `tau-net-bridge` binary at a known path (resolved via TAU_NET_BRIDGE_PATH env var)

## References

- Spec: `docs/superpowers/specs/2026-05-07-sandbox-proxy-design.md`
- Plan: `docs/superpowers/plans/2026-05-07-sandbox-proxy.md`
- Production precedent: Anthropic sandbox-runtime (Oct 2025)
- ADR-0019 (superseded)
