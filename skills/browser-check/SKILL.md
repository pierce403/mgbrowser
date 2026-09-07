---
name: browser-check
description: Validate mgbrowser transport, document, paint and native interaction changes with local fixtures, then verify specifically requested live browsing journeys without substituting fixture success for website compatibility.
---

# Browser check

Read affected FEATURES.md criteria and docs/RUNNING.md. Run cargo test --locked and the native dependency guard for relevant changes. Keep browser code Rust-only; native display servers and test infrastructure are separate from browser dependencies.

For input/navigation changes, build the binary/examples, start the loopback journey_server, and run the browser with its local URL, --smoke-search, --exit-after-smoke and an ignored tmp/ evidence directory. Inspect rendered frames, actual requested URLs and final exit status. Stop only the fixture service you started. CI uses Xvfb to reproduce this path.

The driver exercises application input handlers in a real window; state that distinction when independent desktop input was not performed. A successful local fixture proves that path only. For an authorized live-site journey, use the site's actual form fields/links and ordinary cookies/redirects; record the exact failing stage and response. Never count a placeholder, interstitial link, fabricated result, or another service as completing the requested site journey.

For CDP changes, read docs/CDP.md and docs/cdp-protocol.json. Run the external Rust examples/cdp_journey.rs client against an owned native browser with --remote-debugging-port=0 and the loopback fixture server; docs/CDP.md gives commands and CI reproduces them under Xvfb. This proves public WebSocket behavior independently of App hooks. Verify schema/discovery, session and stale-node errors, actual form query/result destination, viewport PNG dimensions and rendered output. Keep local endpoint ports/process identities explicit and stop only test processes you started. Use the implemented protocol subset; an unsupported Runtime command is not authorization to substitute another browser engine.

For script changes, read docs/JAVASCRIPT.md. Keep live execution in the restricted worker; run the language/DOM tests and actual worker isolation selftest before an authorized live page. Use --enable-scripts with /script-redirect and /script-home on the local fixture server: the form must be created by real script execution, then usable through normal input and the external CDP client. Exercise /script-loop, verify a bounded error with readable content, and navigate onward in the same browser over CDP. CI reproduces these local checks. Source/projection rejection must retain the original no-script fallback and discard proposed navigation. Passing authored fixtures or syscall-denial probes is not full ECMAScript conformance or whole-browser sandbox assurance.

Keep live page/query artifacts in ignored tmp/ unless deliberately selected for publication without private data. Preserve deterministic local fixtures and component regressions in tests/. Update feature evidence and the daily log with both success and failure; keep the full goal open if any required live stage remains unverified.
