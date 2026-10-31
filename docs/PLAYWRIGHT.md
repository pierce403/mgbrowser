# Playwright acceptance

Status: **compatible** with the pinned ordinary Playwright client (`playwright-core`
**1.58.2**) as of `v0.9.29` across both `chromium.connectOverCDP` (HTTP discovery
and direct browser WebSocket) and `chromium.launch` (`--remote-debugging-pipe`
and `--headless`). Unsupported CDP methods continue to fail explicitly.

## Reproduce

Use Linux, Node.js 20 or newer, Xvfb and an actual packaged/installed mgbrowser:

```sh
npm --prefix tools/playwright ci --ignore-scripts --no-audit --no-fund
node tools/playwright/configuration.test.mjs
node tools/playwright/acceptance.mjs /absolute/path/to/mgbrowser
MGBROWSER_BIN=/absolute/path/to/mgbrowser node --test tools/playwright/launch.test.mjs
```

The separate package/lock pins `playwright-core` **1.58.2**, Apache-2.0. It adds
no Cargo dependency and downloads no browser. Do not run `playwright install`.
The checked-in client source is unmodified. The harness starts only the supplied
mgbrowser binary, an owned Xvfb, a temporary HOME/XDG profile and a loopback HTTP
fixture on an ephemeral port. It does not use port 7878 or an existing desktop
session. Automatic updates and page scripting are disabled.
The launch configuration passes `--disable-scripts` explicitly: production
scripting has defaulted on since v0.9.4. The summary records the actual arguments
from that same configuration; a remembered default is not evidence of permission.

Each run prints its ignored `tmp/playwright-acceptance-*` directory. It retains
native startup logs, HTTP and WebSocket client results, `pw:protocol` traffic,
fixture requests and screenshots if that stage is reached. Cleanup stops only
processes owned by the harness. Artifacts contain only the authored local fixture.
The public input is the executable path, not a live website or remote endpoint.

Both the HTTP CDP base and the emitted browser WebSocket are passed separately
to unmodified `chromium.connectOverCDP`. A successful client must then assert:

1. Existing default context/page discovery and creation of a distinct page.
2. Ordinary navigation, HTTP 200 and the actual document title.
3. Locator fill/click, a real Unicode form request and resulting document text.
4. A real nonempty viewport PNG screenshot.
5. The deliberately missing stylesheet's HTTP 404 through Playwright response
   events, and an explicit error for an unknown CDP method.
6. Closing only the newly created page, without retargeting or navigating the
   original page.

Console, page-error and request-failure events are retained when delivered, but
this scripts-disabled fixture does **not** establish JavaScript error reporting.
That requires a separate engine-enabled acceptance path after real remote
execution contexts exist. A 404 is a delivered HTTP response, not necessarily a
`requestfailed` event.

The command exits nonzero when any required step fails. There are no `xfail`
rules, accepted unsupported-method responses or client patches that turn this
baseline green. It is intentionally not added to the currently passing CI suite.
The pure configuration regression runs separately in CI; it
neither launches a browser nor changes the failing compatibility assertions.

## Verified v0.9.29 compatibility: 2026-10-31

`v0.9.29` implements real target auto-attachment (`Target.setAutoAttach` with
`flatten: true` and `waitForDebuggerOnStart`), isolated utility worlds
(`Page.createIsolatedWorld`, utility-world `Runtime.evaluate` and
`Runtime.callFunctionOn` with `awaitPromise` and DOM element marshaling even
when page scripts are disabled via `--disable-scripts`), `Page` lifecycle events,
`DOM`/`CSS`/`Network`/`Input`/`Emulation`, `Browser.setDownloadBehavior`
(`downloadWillBegin` / `downloadProgress`), and Chromium FD `3`/`4`
`--remote-debugging-pipe` + `--headless` virtual framebuffer rendering.

With unmodified `playwright-core` `1.58.2`:
- `node tools/playwright/acceptance.mjs /path/to/mgbrowser` exits `0`
  (`http: PASS`, `websocket: PASS`, `PLAYWRIGHT_ACCEPTANCE_OK`), verifying
  default context/page discovery, `newPage()`, HTTP `200` navigation and title,
  locator `fill`/`click` with Unicode form submission, nonempty PNG screenshot,
  stylesheet HTTP `404` response event, explicit error on unknown CDP method,
  and isolated page closure without disturbing the original page.
- `MGBROWSER_BIN=/path/to/mgbrowser node --test tools/playwright/launch.test.mjs`
  exits `0`, verifying `chromium.launch` over `--remote-debugging-pipe` in
  `--headless` mode with locators, screenshot capture, and real download stream
  completion.

## Observed public v0.9.21 baseline: 2026-10-26

The latest verified public executable comes from immutable release commit
`321e1e87e47e76102d5cc3a4d010acc24d3eff72`, SHA-256
`5889e5a5e8e3c6be23154d2ead091f20ce7ab69a95a1e1f8f8d9b13d1b372dbb`.
Node24.19.0 and the unchanged1.58.2 client use the frozen explicit
`--disable-scripts` configuration. Both HTTP and browser-WebSocket connections
fail at `Target.setAutoAttach`; both logs also contain the parallel
`Browser.setDownloadBehavior` failure. Each result has no completed steps or
events. Exactly one native startup `GET /` returns200; this is not a Playwright
navigation/title/locator pass.

Actual client exit1 is retained, with normal completion and confirmed owned-
group cleanup, in1.729s. The outer evidence owner exits0 only because it verifies
that failure, not because compatibility passes. Its unchanged producer limits
are165s/2MiB output,512 artifact entries/8MiB per file/64MiB total. It terminates
the inherited owned group and reaps its direct Node leader, not every descendant
individually. Xvfb's display-collision warnings do not prevent native startup.

Evidence: `tmp/playwright-acceptance-B2MX9t`; bounded receipt
`tmp/oct26-playwright-v0921-baseline-a/receipt.json`, SHA-256
`094e9a0835322d582122329d9a6e145847534591dd5c5a6813420693cc5b7eaf`.
The owner pins the executable, Node, configuration/lock and relevant installed
client modules; its SHA-256 is
`68d1d41c980bd6816e6b939edf2a3175a474fd25a8134fecc48bde6d28f54034`.
An earlier owner startup exited before its terminal output could be retained,
with no output directory or client result: its32MiB input-hash limit could not
admit the125,989,464-byte configured Node executable. Only that exact pinned host
input's hash admission became128MiB; no browser or produced-artifact limit changed.

## Observed public v0.9.19 baseline: 2026-10-23

The fresh probe uses Node 24.19.0, unchanged playwright-core 1.58.2 and the verified
public v0.9.19 binary from release commit
`9179426360904f01ab3b4394f7699c7ba4c70da9`, SHA-256
`242819ede1aac0710490bf76a94cf00256193f3dbd68aad58e295ee30b2f5378`.
The corrected harness explicitly disables scripting and automatic updates.
Both configuration regressions pass; the actual client journey **fails**:

- HTTP discovery and direct browser WebSocket reach `Browser.getVersion`.
- Both fail during connection at `Target.setAutoAttach` with `-32601`.
- Both raw logs also retain the parallel `Browser.setDownloadBehavior` error.
- Each result has `passed:false`, no completed steps and no client events.
  Exactly one startup fixture `GET /` returns 200. It is not a Playwright
  navigation, title, locator, screenshot or error-event pass.

Evidence: `tmp/playwright-acceptance-AtoPCC`; bounded outer receipt
`tmp/oct23-playwright-v0919-baseline-a/receipt.json`, SHA-256
`6a1ea9cc3bd4d037f5b261b7a7e910218e0e05dba2fccb86c2d0ec559b8ddb5e`.
The harness exits 1 normally in 1.355s. An independent owner enforces 165s, 2 MiB
outer output and polled artifact bounds of 512 entries, 8 MiB/file, 64 MiB total.
It terminates the owned inherited process group before reaping its direct
leader, even after normal nonzero exit. This is not an OS disk quota or proof
of individually reaping every descendant. Xvfb's retained display-collision
warnings do not prevent the owned native window loading the fixture.

Before this correction, the harness reported `scriptsEnabled:false` but omitted
`--disable-scripts`. Do not use that old launch configuration to claim scripting
was disabled on a default-on release. The older observations below retain their
original release scope; the corrected19 run is historical, superseded above.

## Observed public v0.7.2 baseline: 2026-09-17

Tested with Node.js 24.19.0 and the verified public-installed v0.7.2 executable,
SHA-256 `675d2df4708620ab8f98d581ce0bf8c66835cc91376f9a17b6af9a2e3dc64aab`.
The owned native window loaded the local fixture with HTTP 200, title
`Mg Playwright fixture` and one real form. No remote page script was executed.

| Connection | Actual first result |
| --- | --- |
| HTTP base | Playwright requests `/json/version/`, including the trailing slash. Mg returns HTTP 404. |
| Browser WebSocket | Connection and `Browser.getVersion` succeed. `Target.setAutoAttach` returns `-32601`, so connection initialization fails. |
| Parallel WebSocket initialization request | `Browser.setDownloadBehavior` also returns `-32601`; raw traffic records both errors. |

Nothing beyond connection initialization ran: pages, navigation through
Playwright, title, locators, screenshot and client error-event assertions remain
unverified. The fixture's initial load was browser startup, not a successful
Playwright navigation. Local evidence is `tmp/playwright-acceptance-L3e9or`.

The public-installed v0.8.0 release at commit
`8b84604c83541beade431cfbedaa2dc449878011` was retested on 2026-09-17 with the
same unmodified pinned client and scripts disabled. Binary SHA-256:
`fa69c06f4770e92ee9f13486550790583ded69005d16c35bab3efc2573690ba4`.
The executable was `tmp/public-release-install-lj3rw4lr/.local/bin/mgbrowser`.
Both HTTP discovery (including `/json/version/`) and direct WebSocket reach
`Browser.getVersion`, then fail at `Target.setAutoAttach` with `-32601`.
The parallel `Browser.setDownloadBehavior` also returns `-32601`. The harness
exits 1: no title, locator or later acceptance step ran. This fixes discovery,
not Playwright control. Public-binary evidence: `tmp/playwright-acceptance-oqK1ep`.

Public v0.9.0 was retested on 2026-09-18 from tag commit
`7b5fecac59c167442a330e2a4822b153073fc69b`, binary SHA-256
`5a9f9035ce0c5007a85158b9e24ee67e2a8743148e0f4f08922c27a71cc379fe`.
The same unmodified1.58.2 client exits1: HTTP and WebSocket initialization both
fail on `Target.setAutoAttach`; `Browser.setDownloadBehavior` also returns
`-32601`. No title, locator, navigation or screenshot acceptance step completed.
Evidence: `tmp/playwright-acceptance-EyB4vi`, `tmp/news-public-playwright.log`.
The reading-layout release does not change this automation status.

## Required integration, not stub successes

The pinned client's installed `lib/server/chromium/chromium.js` constructs the
trailing-slash discovery URL. `crBrowser.js` requests real target auto-attachment
and download policy before exposing a browser. Its `crPage.js` then initializes
page/frame lifecycle, Runtime contexts, logging, Network events, isolated worlds
and browser defaults. These later requirements are **source-derived**, not
claims that the baseline got past its observed first failures.

Locator and title operations are not simple aliases for Mg's current DOM CDP
subset. `crExecutionContext.js` uses `Runtime.evaluate`, `Runtime.callFunctionOn`,
remote object IDs, property access and object release for the client's injected
utility scripts. Real integration needs:

- Correct target/session/frame lifecycle tied to actual tabs and windows.
- Restricted-worker remote contexts, handle lifetime and exception transport,
  with explicit policy for automation execution while page scripting is off.
- Sufficient real DOM/layout/actionability behavior for the client's utilities.
- Navigation/lifecycle and Network/Log/exception events backed by real work.
- Honest implementations or explicit rejection of download/emulation requests.

Do not add method-name stubs, fabricate execution results, run live scripts in
the browser parent, loosen worker isolation, or claim compatibility after fixing
only discovery and auto-attachment. Add support in bounded increments and rerun
this exact client/workflow before changing its status.

## Proposed implementation sequence

This is a source-derived sequence, not completed client acceptance.
v0.9.20 ships the [separately frozen Runtime contract](RUNTIME_AUTOMATION.md).
Its focused typed-realm, parent/broker, actual child, native/CDP and local
Rust/JSPLAN controls pass. Both complete score lanes, normal publication and
public install/update acceptance pass separately in the
[public closeout](../memory/logs/2026-10-25.md#v0920-public-release-closeout).
Native target mutations ship as v0.9.21 under
[TARGET_LIFECYCLE.md](TARGET_LIFECYCLE.md), with public acceptance recorded in the
2026-10-26 log. Do not repeat that completed implementation.
Neither increment completes the client journey. Runtime's permission
policy requires remote debugging **and effective
page scripting enabled**, whether by the current default or `--enable-scripts`.
`--disable-scripts` must remain effective: debugging alone never starts,
restarts or grants execution to a realm. All evaluation stays in the restricted
worker. The current scripts-disabled client fixture remains unchanged; it will
need a separately reviewed automation permission/world implementation before
ordinary Playwright utilities can execute there.

### Next meaningful increment

Require both unmodified ordinary connection paths, real existing-page utility-
world `document.title`, and actual initialized `about:blank` creation/closure
without changing page A. Autoattachment alone is a prerequisite, not this
increment's completion boundary. [AUTOMATION_STARTUP.md](AUTOMATION_STARTUP.md)
freezes its native startup/session contract before implementation.

The following gates remain required, not stub acknowledgements:

- Distinct named globals share one actual DOM and one restricted worker's
  cumulative policy. Remote debugging grants reviewed client execution, never
  permission to run inline/external page sources under `--disable-scripts`.
  Automation-only initialization must preserve the accepted arena, `noscript`
  semantics and real readiness. Existing `BoaPageRealm::start` reparses/runs
  page sources and is not that initialization mode. A blank document currently
  intentionally has no worker/context; do not fabricate one on the parent.
  Owner/root/group issuance and one pending transaction stay document-wide,
  not multiplied per world. Adding a second Engine or worker is not world
  isolation under the current shared profile.
- Title uses the10,122-byte stock utility,10,257 bytes in its ordinary wrapper,
  followed by a call requesting `awaitPromise:true,userGesture:true`. Both
  semantic flags currently reject. The stock utility returns a primitive title
  directly; accepting await-on-a-non-Promise need not implement waiting for a
  timer-settled Promise. Carry the option into the worker and inspect the actual
  result, leaving unsupported real Promise waiting an explicit error. Define and
  test genuine activation before accepting userGesture; do not ignore it or
  manufacture a click. Generic worker execution must supply the title.
- Implement actual Log/Network subscriptions, lifecycle delivery, future named-
  world registration/creation, focus and media/default behavior requested during
  page initialization. Default attachment uses `noDefaultViewport:true`; do not
  unnecessarily pull window-resizing work into this boundary.
  The named-world request includes the upstream-spelled
  `grantUniveralAccess:true`: its cross-origin authority requires an explicit
  decision, not an ignored default. Browser-root parameterless `getTargetInfo`
  currently rejects zero/multiple pages; genuine browser-target metadata is a
  further decision before claiming connection with those rosters.
- Implement finite parent-owned `allowAndName` download behavior with real files,
  safe paths, actual progress/terminal events, cancellation/failure and teardown.
  The stock default requests it even though the current form fixture downloads
  nothing. Add a genuine local download witness; connection success alone cannot
  prove download support. Keep unsupported options explicit errors.

Freeze an explicit permission/world/download design and additive positive and
negative controls before those production changes. This documentation adopts
no new message size, CPU/memory/time allowance or filesystem/syscall authority.
The full unchanged acceptance must still run and record its actual remaining
failure; do not turn this partial boundary into F-024 completion.

### First Runtime increment: implementation acceptance contract

The preparatory documentation commit968c654 implemented no Runtime commands.
Before source adoption, [RUNTIME_AUTOMATION.md](RUNTIME_AUTOMATION.md) froze typed
request/reply/value forms, numeric handle/argument/property/serialization
admissions and authored negative controls. Preserve that contract during current
integration. Do not infer correctness from client success or enlarge an old bound
to fit an injected utility. Current source and public v0.9.20 use strict
modern-v8; the recorded public v0.9.19 baseline had no Runtime implementation.
Full client criteria remain unchecked.

- Expose only an existing accepted, usable main-world worker context. Identity
  includes target, worker session, navigation generation and world, plus client/
  flattened-session authority. A DOM projection revision is not a new realm.
  Native tab moves preserve it; navigation, terminal failure and closure destroy
  it. No replacement worker or second engine merely to satisfy a command.
  A final transaction can stop the child despite a Ready reply: never publish
  usable handles/context authority after that worker has stopped.
- Evaluate supplied source generically with guarded `evaluate_raw`; compile
  and invoke actual functions with guarded `call_raw`. Root actual returned
  objects in bounded traced worker storage. Preserve receiver/argument identity;
  never substitute a parent title/selector or use page-overridable `.call`.
  Property inspection must have an explicit descriptor/proxy policy, not invoke
  getters merely to produce a description or pretend unsupported objects empty.
- Freeze lossless primitive/UTF-16/nonfinite/negative-zero/BigInt representation,
  by-value cycles/depth/size handling and release/group semantics before coding.
  The current CDP JSON codec cannot represent lone-surrogate strings: reject
  unsupported values explicitly unless a separately reviewed codec supports
  them, never silently replace them with lossy text or literal escape characters.
  Forged, foreign, stale and already-released IDs need deliberate tests. Revoking
  client authority and releasing worker roots must stay bounded on disconnect,
  including during execution or after transaction exhaustion; no cleanup queue
  with unlimited work or hidden roots.
- Return genuine caught exceptions without calling page `window.onerror` merely
  because a Runtime evaluation threw. Distinguish ordinary preceding effects
  from fatal rejection. Do not reuse the page's uncaught-error adapter blindly.
  Freeze the injected-source privacy policy before execution: a destination
  context is not proof of fetched same-origin source provenance. Preserve the
  sticky conservative error/rejection exposure rules and trusted observer inputs.
  Only after identity/resource/projection validation and the existing exactly-one
  active-time charge may results, handles and DOM effects become visible.
- Use the existing single pending/validation slot shared with activations/tasks.
  Carry real bounded native control edits and version acknowledgements where
  needed, preserving newer typing. Runtime evaluation is not a click/submit and
  grants no user activation or fabricated lifecycle/default action.
- Preserve strict modern-v8/legacy-v1 separation and the typed automation
  envelope. Keep 64 transactions, 64 KiB later frames, 4 MiB replies,
  32 MiB lifetime wire, 2s active/300s absolute lifetime and all source/opcode/
  System/DOM/process/work limits. Existing Event/TaskPump cannot acquire script
  payloads. New admission must account for handles, conversion and cleanup too.
- Explicitly reject unsupported promise awaiting, utility-world creation,
  user-gesture and other semantic options in this first synchronous increment.
  One guarded microtask checkpoint does not implement waiting for timer-settled
  promises or a separate world. No extra source, work, syscall or parent authority.

Acceptance must span Butane/Sparkle retained state, forced-GC roots and genuine
exceptions; actual restricted-child frames/resources/terminal cleanup; parent
single-flight/stale/edit/charge/publication rejection; direct and flattened CDP
cross-client/context/navigation lifetime; packaged and public native behavior.
Update only assertions whose formerly unsupported command is genuinely adopted;
retain unknown-method negatives using an actually unsupported method. The
user-facing increment requires its normal versioned release. Full F-011/F-016/
F-024 acceptance, the unchanged client journey and all other features stay open.

### Subsequent integration

1. **Worker foundation, then connection and title on an existing HTTP page.**
   Add typed automation transactions through `scripts.rs`, `page_session.rs`
   and the versioned worker envelope, with guarded execution in `boa.rs` and
   `modern.rs`. Worker-owned rooted handles need context/session/generation
   scoping, bounded properties/serialization, real exceptions, promise handling,
   explicit release and teardown. Main and utility worlds need distinct globals
   sharing the actual DOM and cumulative budgets, not duplicate documents.
   Navigation and worker failure destroy their contexts and reject pending work.
   Playwright's title operation executes its 10,122-byte stock utility source,
   then calls it through `Runtime.callFunctionOn` to read `document.title`.
   Do not recognize that expression and substitute a parent-side title result.

   Implement real auto-attachment, stable context/target/session identity,
   startup waiting/resume, context events and isolated-world registration in
   `cdp_browser.rs`. Empty startup source still registers a world for future
   documents. Enable subscriptions must report actual lifecycle/resource/errors.
   The pinned client also requests `Browser.setDownloadBehavior` with
   `behavior:"allowAndName"`, a download directory and `eventsEnabled:true`.
   That is not a no-op when downloads are absent: implement bounded parent-owned
   download behavior or leave connection failing explicitly. Version 1.58.2's
   public connection options have no `noDefaults` or `acceptDownloads` escape.

2. **Ordinary locator fill/click on the existing authored form fixture.**
   The untouched locator utility is 305,013 source bytes. Current automation
   source/operation admissions are16KiB/32KiB, modern-v8 later frames allow
   only64KiB, and incoming CDP commands allow256KiB: it exceeds all four
   before JSON/wrapper overhead.
   Full locator support needs a separate explicit transport/resource-policy
   review, not fragmentation, repeated Init or a silent Event/TaskPump cap change.
   A dedicated bounded automation-message class alone cannot solve the CDP cap.
   Preserve cumulative source, memory, work, transaction, wire and time bounds;
   execute the exact utility in an owned worker fixture to measure feasibility.
   Its constructor already needs MutationObserver and option-object capture
   listeners. Selection/actionability additionally needs real DOM prototypes,
   relationships, selectors, focus/selection, computed style, hit testing,
   rectangles, animation-frame progress and trusted event/default handling.
   Geometry must describe the rendered document at a checked revision, and
   worker focus/control edits must synchronize with Chassis. Add object-backed
   DOM geometry/scroll commands and truthful support for the client's additional
   navigation/input parameters. Do not force clicks, bypass utilities or fake
   stable frames. Test disabled, obscured, detached and stale elements too.

3. **Complete the current unmodified acceptance journey.**
   Host-backed page creation/closure must preserve other targets. Emit actual
   Network requests, responses and completion/failure events correlated with
   frame/loader identity and `Page.lifecycleEvent` load milestones. Verify the
   Unicode form request, result text, screenshot, CSS 404 and surviving page.
   Restricted-worker success alone is not client or native-routing acceptance.

An already-resumed `Runtime.runIfWaitingForDebugger` can return unchanged state.
Default-reset requests can do so only when their requested behavior is actually
the current behavior. Auto-attachment, isolated worlds, focus, RAF, observers,
download allowance and event subscriptions are not blanket no-ops. Unsupported
parameters remain explicit errors; measured budget failures remain open gates,
not permission to renew budgets, expand isolation or patch the pinned client.

Primary references: Playwright documents
[connectOverCDP](https://playwright.dev/docs/api/class-browsertype#browser-type-connect-over-cdp)
as its lower-fidelity Chromium-only connection path. Mg compatibility would be
our independently tested contract, not upstream support for a non-Chromium
browser. The versioned upstream
[Chromium client](https://github.com/microsoft/playwright/tree/v1.58.2/packages/playwright-core/src/server/chromium)
and the installed pinned package are the implementation references.
