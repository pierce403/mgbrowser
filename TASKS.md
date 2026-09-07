# Work queue

This queue is ordered; planning entries do not imply implementation has begun.

Latest steering — **T-008 / F-011:** Initial CDP automation subset implemented, published and verified locally and in GitHub CI. Use the external CDP client for browser checks. Full protocol support remains the long-term contract, expanding with actual Network, Runtime/Debugger, frames, CSS and other browser capabilities; no stub-success compatibility.

1. **T-001 / F-001, F-002:** Publish foundation and verify GitHub Pages, exact deployed content, custom TLS, and HTTPS enforcement. Complete; evidence in the 2026-09-07 log.
2. **T-007 / F-010, F-008:** Active user goal: open our browser, browse to Google, search, click the first result and attempt the destination. Original JS/DOM, labeled control flow, URI helpers and bounded Function/eval now pass local worker/native/CDP dynamically created-form journeys. Real Google still provides no result links; current search diagnostics report identifier-escape/unsupported-character lexer failures and missing setTimeout. Next: expand general lexer/language semantics against independent fixtures; design parent-brokered external scripts, persistent realms and actual events/timers. Re-run the real journey in bounded checkpoints. Do not port challenge logic, substitute an existing engine or fabricate results.
3. **T-003 / F-003, F-004:** Initial native HTML-flow browser and local form→result→destination path verified. Expand keyboard editing, cancellation, document layout and navigation acceptance; full feature gates remain open.
4. **T-002 / F-003:** Choose project license and expand the initial local fixtures into the planned 20-case corpus. Linux X11/XWayland selected as current runnable target using Rust protocol code, without native font/codec/crypto backends.
5. **T-004 / F-006:** Use the deterministic local journey as an initial baseline; implement the general Rust evaluator and report schema. Do not start unattended research before limits and failure classification work.
6. **T-005 / F-005:** Add CSS flow and actual Rust-decoded page images in individually tested contributions.
7. **T-006 / F-006, F-007:** Candidate comparison, outside-contributor reproduction, release packaging and MVP acceptance.

For each task record scope, relevant feature IDs, hypothesis where applicable, acceptance evidence and unresolved blockers in the dated log. Split tasks before implementation if they cannot be reviewed as a bounded change.
