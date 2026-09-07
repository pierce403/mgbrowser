---
name: browser-check
description: Validate mgbrowser transport, document, paint and native interaction changes with local fixtures, then verify specifically requested live browsing journeys without substituting fixture success for website compatibility.
---

# Browser check

Read affected FEATURES.md criteria and docs/RUNNING.md. Run cargo test --locked and the native dependency guard for relevant changes. Keep browser code Rust-only; native display servers and test infrastructure are separate from browser dependencies.

For input/navigation changes, build the binary/examples, start the loopback journey_server, and run the browser with its local URL, --smoke-search, --exit-after-smoke and an ignored tmp/ evidence directory. Inspect rendered frames, actual requested URLs and final exit status. Stop only the fixture service you started. CI uses Xvfb to reproduce this path.

The driver exercises application input handlers in a real window; state that distinction when independent desktop input was not performed. A successful local fixture proves that path only. For an authorized live-site journey, use the site's actual form fields/links and ordinary cookies/redirects; record the exact failing stage and response. Never count a placeholder, interstitial link, fabricated result, or another service as completing the requested site journey.

Keep live page/query artifacts in ignored tmp/ unless deliberately selected for publication without private data. Preserve deterministic local fixtures and component regressions in tests/. Update feature evidence and the daily log with both success and failure; keep the full goal open if any required live stage remains unverified.
