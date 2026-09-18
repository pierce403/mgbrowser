# Playwright acceptance

Status: **not compatible** with the pinned ordinary Playwright client. This is
an external-client acceptance scaffold, not another JavaScript backend, protocol
shim or passing CI gate. Existing Rust CDP journey success is a separate claim.

## Reproduce

Use Linux, Node.js 20 or newer, Xvfb and an actual packaged/installed mgbrowser:

```sh
npm --prefix tools/playwright ci --ignore-scripts --no-audit --no-fund
node tools/playwright/acceptance.mjs /absolute/path/to/mgbrowser
```

The separate package/lock pins `playwright-core` **1.58.2**, Apache-2.0. It adds
no Cargo dependency and downloads no browser. Do not run `playwright install`.
The checked-in client source is unmodified. The harness starts only the supplied
mgbrowser binary, an owned Xvfb, a temporary HOME/XDG profile and a loopback HTTP
fixture on an ephemeral port. It does not use port 7878 or an existing desktop
session. Automatic updates and page scripting are disabled.

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

This is a source-derived plan, not adopted behavior or completed acceptance.
The automation-script permission decision remains open. Existing scripting
opt-in is unchanged; the proposed first implementation requires both
`--remote-debugging-port=0` and `--enable-scripts`. A debugging port must not
silently enable scripting. All evaluation stays in the restricted worker.

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
   The untouched locator utility is 305,013 source bytes; current v2 later
   messages allow only 64 KiB and click/submit events. Introduce an explicitly
   bounded automation-message class rather than enlarge the existing event cap.
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
