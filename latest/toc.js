// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><a href="index.html">Introduction</a></li><li class="chapter-item expanded affix "><li class="part-title">Project</li><li class="chapter-item expanded "><a href="../ROADMAP.html">Roadmap</a></li><li class="chapter-item expanded "><a href="../CHANGELOG.html">Changelog</a></li><li class="chapter-item expanded "><a href="../CONSTITUTION.html">Constitution</a></li><li class="chapter-item expanded "><a href="../GUIDELINES_CHEATSHEET.html">Guidelines cheatsheet</a></li><li class="chapter-item expanded "><a href="../GOVERNANCE.html">Governance</a></li><li class="chapter-item expanded "><a href="../CONTRIBUTING.html">Contributing</a></li><li class="chapter-item expanded "><a href="../SECURITY.html">Security policy</a></li><li class="chapter-item expanded "><a href="../CODE_OF_CONDUCT.html">Code of conduct</a></li><li class="chapter-item expanded affix "><li class="part-title">Tutorials</li><li class="chapter-item expanded "><a href="tutorials/index.html">Overview</a></li><li class="chapter-item expanded "><a href="tutorials/build-your-first-skill.html">Build your first skill</a></li><li class="chapter-item expanded affix "><li class="part-title">How-to</li><li class="chapter-item expanded "><a href="how-to/index.html">Overview</a></li><li class="chapter-item expanded "><a href="how-to/install-a-skill.html">Install a skill</a></li><li class="chapter-item expanded "><a href="how-to/author-a-skill.html">Author a skill</a></li><li class="chapter-item expanded "><a href="how-to/export-a-skill.html">Export a skill</a></li><li class="chapter-item expanded affix "><li class="part-title">Reference</li><li class="chapter-item expanded "><a href="reference/index.html">Overview</a></li><li class="chapter-item expanded "><a href="reference/sandbox-platform-support.html">Sandbox platform support</a></li><li class="chapter-item expanded "><a href="reference/skill-manifest-schema.html">Skill manifest schema</a></li><li class="chapter-item expanded affix "><li class="part-title">Explanation</li><li class="chapter-item expanded "><a href="explanation/index.html">Overview</a></li><li class="chapter-item expanded "><a href="explanation/escape-hatches.html">Escape hatches</a></li><li class="chapter-item expanded "><a href="explanation/tau-as-language.html">tau as language</a></li><li class="chapter-item expanded "><a href="explanation/two-layer-skills.html">Two-layer skills</a></li><li class="chapter-item expanded affix "><li class="part-title">Architecture decisions</li><li class="chapter-item expanded "><a href="decisions/index.html">Index</a></li><li class="chapter-item expanded "><a href="decisions/template.html">ADR template</a></li><li class="chapter-item expanded "><a href="decisions/0001-bootstrap.html">ADR-0001 — Bootstrap</a></li><li class="chapter-item expanded "><a href="decisions/0002-manifest-format.html">ADR-0002 — Manifest format</a></li><li class="chapter-item expanded "><a href="decisions/0003-tau-ports.html">ADR-0003 — tau-ports</a></li><li class="chapter-item expanded "><a href="decisions/0004-tau-pkg.html">ADR-0004 — tau-pkg</a></li><li class="chapter-item expanded "><a href="decisions/0005-package-source-and-kind-serde.html">ADR-0005 — Package source and kind serde</a></li><li class="chapter-item expanded "><a href="decisions/0006-tau-runtime.html">ADR-0006 — tau-runtime</a></li><li class="chapter-item expanded "><a href="decisions/0007-tau-cli.html">ADR-0007 — tau-cli</a></li><li class="chapter-item expanded "><a href="decisions/0008-plugin-loading.html">ADR-0008 — Plugin loading</a></li><li class="chapter-item expanded "><a href="decisions/0009-llm-error-typing-and-conformance.html">ADR-0009 — LLM error typing &amp; conformance</a></li><li class="chapter-item expanded "><a href="decisions/0010-tool-args-schema-validation.html">ADR-0010 — Tool-args schema validation</a></li><li class="chapter-item expanded "><a href="decisions/0011-streaming-llm-responses.html">ADR-0011 — Streaming LLM responses</a></li><li class="chapter-item expanded "><a href="decisions/0012-tau-lifecycle-commands.html">ADR-0012 — tau lifecycle commands</a></li><li class="chapter-item expanded "><a href="decisions/0013-repl-persistence.html">ADR-0013 — REPL persistence</a></li><li class="chapter-item expanded "><a href="decisions/0014-sandboxing.html">ADR-0014 — Sandboxing</a></li><li class="chapter-item expanded "><a href="decisions/0015-sandbox-activation.html">ADR-0015 — Sandbox activation</a></li><li class="chapter-item expanded "><a href="decisions/0016-plugin-compat-verification.html">ADR-0016 — Plugin compat verification</a></li><li class="chapter-item expanded "><a href="decisions/0017-e2e-landlock-and-driver.html">ADR-0017 — E2E landlock and driver</a></li><li class="chapter-item expanded "><a href="decisions/0018-ci-optimization.html">ADR-0018 — CI optimization</a></li><li class="chapter-item expanded "><a href="decisions/0019-per-host-network-filter.html">ADR-0019 — Per-host network filter</a></li><li class="chapter-item expanded "><a href="decisions/0020-sandbox-proxy.html">ADR-0020 — Sandbox proxy</a></li><li class="chapter-item expanded "><a href="decisions/0021-per-plugin-images.html">ADR-0021 — Per-plugin images</a></li><li class="chapter-item expanded "><a href="decisions/0022-sandbox-darwin.html">ADR-0022 — Sandbox (darwin)</a></li><li class="chapter-item expanded "><a href="decisions/0022-tau-workflow.html">ADR-0022 — tau-workflow</a></li><li class="chapter-item expanded "><a href="decisions/0023-sandbox-windows-scaffold.html">ADR-0023 — Sandbox windows scaffold</a></li><li class="chapter-item expanded "><a href="decisions/0024-multi-agent-orchestration.html">ADR-0024 — Multi-agent orchestration</a></li><li class="chapter-item expanded "><a href="decisions/0025-skills-foundation.html">ADR-0025 — Skills foundation</a></li><li class="chapter-item expanded "><a href="decisions/0026-skills-install-pipeline.html">ADR-0026 — Skills install pipeline</a></li><li class="chapter-item expanded "><a href="decisions/0027-skills-discovery.html">ADR-0027 — Skills discovery</a></li><li class="chapter-item expanded "><a href="decisions/0028-docs-deployment.html">ADR-0028 — Docs deployment</a></li><li class="chapter-item expanded "><a href="decisions/0028-skills-runtime-invocation.html">ADR-0028 — Skills runtime invocation</a></li><li class="chapter-item expanded "><a href="decisions/0029-skills-anthropic-interop.html">ADR-0029 — Skills Anthropic interop</a></li><li class="chapter-item expanded "><a href="decisions/0030-skills-reference-packages.html">ADR-0030 — Skills reference packages</a></li><li class="chapter-item expanded affix "><li class="part-title">Project artifacts</li><li class="chapter-item expanded "><a href="dev-environment.html">Dev environment</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0].split("?")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
