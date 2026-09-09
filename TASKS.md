# Work queue

Current task: **T-010 / F-013, component extraction and v0.2.0**. User approved
Butane (JS), Sparkle (HTML/rendering), Chassis (services/optional UX) and mg-browser
(platform host), including push to main. Verify independent embedding, preserve
worker isolation and native/CDP journeys, then finish the versioned release and
public-installer gates. Drop-in compatibility and ThermiteOS implementation are
future tasks, not part of this extraction.

This queue is ordered; planning entries do not imply implementation has begun.

Completed release follow-up: v0.1.1 is published for the HTTP warning and
punctuation changes; installer/site, clean install and v0.1.0 upgrade are verified.
Release commit: 0c72c8b898f66a8e3e0ad20c8e73eaa4dc165676. Standing
user policy: future user-facing features include a new binary release and public
installer verification before handoff; docs-only changes do not need a release.
No new browser compatibility iteration is included.

2026-09-08 follow-up: user authorizes project-text em-dash replacement with colons,
plain HTTP with a red title/address strip, and Ctrl+L location access. Keep this
small UI/navigation change separate from deferred compatibility work and the
immutable v0.1.0 release.

## Release mode : supersedes the historical queue below

**Completed 2026-09-08:** v0.1.0 Experimental Preview is publicly released at
https://github.com/pierce403/mgbrowser/releases/tag/v0.1.0 from
392867f5f059cc34162360b5a63c4f16b62d6fcc. Exact-commit Rust CI and Pages, tagged
release workflow, public checksum installer, installed version/worker selftest
and desktop/icons all passed. Stop here; deferred engineering is not automatically
reactivated. This closeout changes documentation only, not the release tag/binary.

1. **T-009 / F-012: Complete : shipped v0.1.0 Experimental Preview.** Freeze engine and
   compatibility at 4b9a5f74b09f4e3092f26d5c61d6b8a04e22a4da. Package Linux x86_64
   X11/XWayland binary, MIT/license inventory, installer, Mg identity and launcher.
   Get exact-commit Rust CI and Pages green, tag v0.1.0, publish the GitHub release,
   verify public assets/checksum and the exact curl install command, then stop.
2. **After v0.1 only:** Google/JavaScript/storage work, external scripts, broader
   CDP, CSS/images, autoresearch evaluator and expanded corpus remain deferred.
   Do not resume them as part of release work. Formal F-007 MVP gates are unchanged.

The following is historical engineering context, not the active task ordering.

Latest completed local increment: bounded Object.create descriptors under
docs/OBJECT_CREATE.md. Fresh data/accessor properties, typed keys, flags and
original receivers pass independent language/resource tests and a real-worker
form. Compact storage preserves ordinary Property64 and Bootstrap26,880. Eight
public allocator cases and seven private groups verify actual allocation,
admission-before-callback and moved payloads; the public example is a CI guard,
not the general research executor. The existing 23-case AST probe is unchanged.
One subsequent Google attempt submits its real form but still renders no results.
This response has no unsupported descriptor error; its first failure is a later
AST admission request. Next: independently measure remaining retained AST/source
ownership before adopting another bounded storage change. Do not raise the cap.
Function.prototype callability remains separate, unadopted work.
Exact publication evidence is recorded separately in the daily log.

Retained restricted page realms and real later click/submit events now pass the
independent local contract in docs/PAGE_SESSIONS.md. The current aggregate passes
1,193 debug tests, 1,080 selected release checks and 25 native/25 external CDP journeys
(excluding repeated child test summaries).
State, cancellation,
versioned edits, validated defaults and cumulative limits work without replay.
The latest live Google attempt submits through that session but still yields no
results. The full Google goal and remote Runtime/CDP compatibility remain open;
exact publication evidence is recorded in the daily log.

Latest steering : **T-008 / F-011:** Initial CDP automation subset implemented, published and verified locally and in GitHub CI. Use the external CDP client for browser checks. Full protocol support remains the long-term contract, expanding with actual Network, Runtime/Debugger, frames, CSS and other browser capabilities; no stub-success compatibility.

1. **T-001 / F-001, F-002:** Publish foundation and verify GitHub Pages, exact deployed content, custom TLS, and HTTPS enforcement. Complete; evidence in the 2026-09-07 log.
2. **T-007 / F-010, F-008:** Active user goal: open our browser, browse to Google, search, click the first result and attempt the destination. Bounded descriptors, static operator storage, core intrinsics and retained events are locally verified. Latest homepage HTTP 200 exposes its actual form and two later activations complete; search HTTP 200 still renders zero items/forms. Three scripts complete with two errors: Ast 377,733 is rejected after 4,135,770 accepted against 4,194,304, then the next script repeats the latched failure. No first result/destination or new required live stage completed. Changing responses are not a controlled comparison. Next: independently measure remaining AST/source ownership and specify any candidate before changing storage. Function.prototype callability, parent-brokered external scripts, broader events/timers and remote Runtime contexts need separate designs. Do not blindly raise limits, port challenge logic, substitute an existing engine or fabricate results.
3. **T-003 / F-003, F-004:** Initial native HTML-flow browser and local form→result→destination path verified. Expand keyboard editing, cancellation, document layout and navigation acceptance; full feature gates remain open.
4. **T-002 / F-003:** Choose project license and expand the initial local fixtures into the planned 20-case corpus. Linux X11/XWayland selected as current runnable target using Rust protocol code, without native font/codec/crypto backends.
5. **T-004 / F-006:** Use the deterministic local journey as an initial baseline; implement the general Rust evaluator and report schema. Do not start unattended research before limits and failure classification work.
6. **T-005 / F-005:** Add CSS flow and actual Rust-decoded page images in individually tested contributions.
7. **T-006 / F-006, F-007:** Candidate comparison, outside-contributor reproduction, release packaging and MVP acceptance.

For each task record scope, relevant feature IDs, hypothesis where applicable, acceptance evidence and unresolved blockers in the dated log. Split tasks before implementation if they cannot be reviewed as a bounded change.
