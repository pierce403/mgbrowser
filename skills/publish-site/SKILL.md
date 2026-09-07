---
name: publish-site
description: Refresh the mgbrowser project page from feature status and daily logs, validate it, and verify authorized GitHub Pages publication at mgbrowser.org.
---

# Publish site

Read FEATURES.md and the latest work log. Update descriptive index.html content when direction changes, with no unsupported readiness claims. Compile tools/site.rs into tmp/site and run it to refresh the marked generated section; run tmp/site --check and git diff --check.

For an authorized publication, commit and push, then inspect the Pages workflow for the intended SHA. Check the repository Pages API for custom domain and certificate state, and confirm HTTPS enforcement. Fetch https://mgbrowser.org without bypassing TLS validation and compare the full HTML against index.html. HTTP success alone does not establish visual quality; inspect in a browser when available and state any QA limit.

Do not publish the repository root as the artifact: the workflow stages only index.html, CNAME and .nojekyll. The deployment target is GitHub Pages, apex only. Domain DNS already pointed to Pages during initialization; recheck before any DNS changes. If certificate issuance is pending, report that state and retry within a bounded verification session rather than claiming success. Do not change nameservers or unrelated DNS records to fix Pages.
