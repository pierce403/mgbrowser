# Work queue

2026-09-16 complete: **T-014 / F-017 shipped as v0.5.0**, release commit
`8c457e04a8ce00dd8cf81802da35a054d3bad8ec`. Menu > Settings offers persistent
System/Light/Dark controls with live Linux desktop preference following.
Rust CI 35150643020, JSPLAN CI 35150642873, Pages 35150642969 and release
workflow 35152075966 pass on that exact commit. Public checksum, exact curl
install/reinstall, worker/session tests, desktop/icons and actual v0.4.1
self-update pass. Native public-binary tests cover System changes, explicit
overrides, restart persistence, both portal APIs and unavailable-portal fallback.
Light/Dark screenshots are visually inspected. Page colors, HTTP red warnings,
optional chrome and engine behavior are preserved. Public archive: 7,879,494
bytes, within the unchanged 8 MiB updater cap. See the dated log for receipts.
No release gates remain. Stop here: further engineering requires a new request;
Google and the remaining JSPLAN roadmap stay deferred.

2026-09-16 complete: **T-013 / F-016 bounded Boa integration shipped as v0.4.1**,
release commit `02b413f27e8ddce9b6408887317f08cd9b9b86b7`. The opt-in production
path uses Boa for inline scripts, bounded Promise checkpoints and retained DOM
events. See [the explicit process profile](docs/BOA.md). Rust CI 35139133042,
JSPLAN CI 35139133037, Pages 35139133092 and release workflow 35140430106 are
green on that exact commit. Public archive/checksum, exact curl install/reinstall,
version/About identity, worker/session selftests, real Boa fixture execution and
desktop/icons pass. Two installed-binary native and two external CDP journeys
pass, with retained-event requests, zero traps and readable fatal-loop recovery.
Actual v0.3.0 and v0.4.0 binaries update to v0.4.1, then pass a no-op update check.
The public archive is 7,240,140 bytes compressed and 23,746,560 unpacked, within
the unchanged 8 MiB updater cap. The size-optimized, symbol-stripped release
preserves engine source, dependencies, resource limits and panic behavior.
v0.4.0's tag/assets remain immutable; its oversize-package failure and the full
receipts remain in the dated log. No release gates remain. Stop here: further
engineering requires a new request. Google optimization remains deferred.
No dual production engine or fallback.
User clarified publication goes directly to main; PRs are not the default for
user-directed work. See the standing instruction in AGENTS.md.

Deferred JSPLAN roadmap; all later engineering requires a new request:

1. **Remaining P1/P2:** comprehensive cooperative work/heap control, expanded
   language/lifecycle acceptance and a broader application profile. Process-v1
   supplies enforceable final containment, not complete native/parser/regex/GC
   work accounting or full adoption-gate completion.
2. **Remaining P3/P4:** external script/module loading, general tasks/timers and
   broader DOM integration, then pinned
   React/Vue applications with real input and long-session acceptance.
3. **P5/P6:** Measured interpreter optimizations, then optional baseline JIT with
   an explicit executable-memory/isolation design and evidence it pays off.
4. **P7/P8:** Named V8 API consumer, then selective optimizing JIT, wider apps,
   architectures, tooling and Wasm as separately scoped work.

F-016, the full P1 gate and P2-P8 exit criteria remain incomplete. The selected Boa
backend does not establish React/Vue browser applications or V8 compatibility.
The initial research comparison remains reproducible in `experiments/jsplan`;
its [historical results](docs/jsplan/RESULTS.md) are not rewritten as full conformance.

2026-09-16 complete: **T-011 / F-014 shipped as v0.3.0**, release commit
e3ac7a7b873eb080baf0fa9be61b343b06cbbcb9. Rust Stylo, bounded same-origin CSS/images
and generic table/inline layout are accepted at 1024/1280 desktop widths. All
normal CI gates, including the language/resource assertions and 26 native/26 CDP
journeys, passed locally and remotely. Exact-commit Pages/release, public checksum,
fresh install/reinstall, worker, desktop/icons, real v0.2.1 self-update and fresh
public-binary HN navigation passed. Stop this request here. No further feature
work or Google/JavaScript optimization is authorized by this completed task.

2026-09-16 completed: **T-012 / F-015**, diagnosed HN styling, added verified
automatic self-updates and About compile identity, published v0.2.1. Diagnosis:
linked CSS loading/cascade/table layout remain absent. The HN implementation goal
below stays planned; this request does not silently expand into that engine work.
v0.2.1 includes the unpublished v0.2.0 extraction, closing T-010 / F-013 too.
Release commit 830c6ca4ede1ccd5c23d3b73ebfd89aca06729ef: exact-SHA Rust CI,
Pages, tagged release, public install and native public updater upgrade all passed.
See the 2026-09-16 log. Stop this request here; HN engine work remains planned.

Completed user-directed goal: **T-011 / F-014: Hacker News desktop rendering**.
See docs/HACKER_NEWS.md for the bounded scope and completed acceptance evidence.
Google/JS optimization remains deferred. This does not complete the formal MVP.
The conversation tracker still needs the user to cancel its unfinished Google
goal before the new goal can be activated there.

Historical task: **T-010 / F-013, now shipped in v0.2.1**. User approved
Butane (JS), Sparkle (HTML/rendering), Chassis (services/optional UX) and mg-browser
(platform host), including push to main. Verify independent embedding, preserve
worker isolation and native/CDP journeys, then finish the versioned release and
public-installer gates. Drop-in compatibility and ThermiteOS implementation are
future tasks, not part of this extraction.

Historical 2026-09-09 checkpoint, superseded by the release above:
source was published at a444cada85ec80e8ac0df6858b63f4f980e4c3b8. Local component,
package/installer and all 26 native/26 CDP journeys passed. Release tagging needs
an authenticated git/tag-capable path: shell Git has no credentials, and the
current connector exposes branch/file operations but no tag/release operation.
Do not claim v0.2.0 is publicly installed until its release workflow and public
smoke check pass. The site explicitly identifies the binary as pending.

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
