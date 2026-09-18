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

The local v0.8.0 candidate accepts `/json/version/`. Repeating the exact client
now reaches WebSocket initialization through both connection forms, but both
still fail at the unimplemented `Target.setAutoAttach`. This fixes discovery,
not Playwright control. Evidence: `tmp/playwright-acceptance-etFn4Q`.

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

Primary references: Playwright documents
[connectOverCDP](https://playwright.dev/docs/api/class-browsertype#browser-type-connect-over-cdp)
as its lower-fidelity Chromium-only connection path. Mg compatibility would be
our independently tested contract, not upstream support for a non-Chromium
browser. The versioned upstream
[Chromium client](https://github.com/microsoft/playwright/tree/v1.58.2/packages/playwright-core/src/server/chromium)
and the installed pinned package are the implementation references.
