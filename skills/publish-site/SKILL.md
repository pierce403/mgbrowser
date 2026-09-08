---
name: publish-site
description: Refresh the mgbrowser project page from feature status and daily logs, validate it, and verify authorized GitHub Pages publication at mgbrowser.org.
---

# Publish site

Standing user policy: feature work includes publishing a new binary release and
verifying that the public installer delivers it, not just pushing main. Use the
next appropriate version, update Cargo.toml/Cargo.lock and docs/RELEASE-vVERSION.md,
refresh README/site, and use the exact-commit gates below. Packaging, notes and
smoke checks must follow that version rather than a hardcoded first release.
Update the installer when its behavior needs to change; its latest-release URLs
already follow each newly published release. Test reinstalls as well as clean
installs. Do not move published tags or silently replace old release artifacts.

Read FEATURES.md and the latest work log. Update descriptive index.html content when direction changes, with no unsupported readiness claims. Compile tools/site.rs into tmp/site and run it to refresh the marked generated section; run tmp/site --check and git diff --check.

For an authorized publication, commit and push, then inspect the Pages workflow for the intended SHA. Check the repository Pages API for custom domain and certificate state, and confirm HTTPS enforcement. Fetch https://mgbrowser.org without bypassing TLS validation and compare the full HTML against index.html. HTTP success alone does not establish visual quality; inspect in a browser when available and state any QA limit.

Do not publish the repository root as the artifact: the workflow stages only index.html, CNAME, .nojekyll, install.sh and the explicitly listed identity assets. Never include logs, tmp or test artifacts. The deployment target is GitHub Pages, apex only. Domain DNS already pointed to Pages during initialization; recheck before any DNS changes. If certificate issuance is pending, report that state and retry within a bounded verification session rather than claiming success. Do not change nameservers or unrelated DNS records to fix Pages.

For a preview release, verify Rust CI and Pages on the exact main commit before
tagging. The v* workflow packages the locked Rust 1.91.1 binary and tests installation
before publishing a normal GitHub Release. Then verify assets/checksum, the public
installer command in an isolated prefix, version, worker selftest and desktop/icon
files. Preview publication does not complete F-007 or the Google compatibility goal.

After publication, run `python3 tools/public-release-smoke.py VERSION` from the
repo root. It verifies public bytes, the exact advertised curl command, current
version, replacement of an in-use executable, a fresh worker selftest, desktop/icons
and latest links. Existing browser processes must restart after installation;
do not claim that an old process can keep spawning workers after its binary is replaced.
This is a manually invoked release gate, not an unattended publication agent.
