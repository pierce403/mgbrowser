# Browser control protocol

mgbrowser implements a Chrome DevTools Protocol (CDP) interface so tools can inspect and operate the same Rust browser that a person sees. The implementation is a bounded subset with stable targets for existing tabs. The long-term goal is full protocol support as the browser gains the corresponding behavior. Returning success for an unimplemented browser feature does not count as compatibility.

The contract is [cdp-protocol.json](cdp-protocol.json). Discovery advertises version `1.3` alongside the mgbrowser product name; this does not claim complete Chrome 1.3, tip-of-tree, Playwright, Puppeteer, or Chrome DevTools frontend compatibility. Page scripts use the restricted [Boa page execution lane](BOA.md), enabled by default since v0.9.4 with `--disable-scripts` to opt out. v0.9.20 ships the synchronous [worker-backed Runtime subset](RUNTIME_AUTOMATION.md), with separate packaged/public native acceptance in the [public closeout](../memory/logs/2026-10-25.md#v0920-public-release-closeout). The historical public v0.9.19 baseline did not implement Runtime. The [original JavaScript evaluator](JAVASCRIPT.md) remains a historical test baseline, without automation authority.

## Connection and scope

Remote debugging is opt-in. The transport binds to `127.0.0.1`, with an explicitly selected port or port zero for an available port. Discovery exposes `/json/version` (also `/json/version/`), `/json/list` (also `/json`), and `/json/protocol`. The browser endpoint is `/devtools/browser/browser-1`; `/json/list` supplies each live tab's permanent `/devtools/page/page-N` endpoint. The original tab remains `page-1`. These discovery conventions follow the [official CDP endpoint documentation](https://chromedevtools.github.io/devtools-protocol/).

```sh
cargo run --locked -- --remote-debugging-port 9222 https://www.google.com/
```

This starts the existing native X11/XWayland browser; the debugging endpoint does not provide a headless mode. Port `0` selects an available port and prints the browser WebSocket URL to stderr.

The Rust command client accepts a command object or an array that shares one connection:

```sh
cargo run --locked --example cdp_command -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  '[{"method":"Browser.getVersion"},{"method":"Page.captureScreenshot"}]' \
  --screenshot tmp/page.png
```

It assigns request IDs, prints JSON response/event lines, fails on protocol errors,
and bounds the batch to 30 seconds. With `--screenshot`, it saves and validates the
last PNG capture and prints metadata instead of its base64 contents. For input or
link work, obtain current IDs with DOM commands; never reuse IDs across navigation.
Keep mouse press/release in one batch. This simple page client does not manage
flattened sessions; the journey regression below verifies those separately.

A page WebSocket talks directly to its permanent page target, never the currently focused tab. A browser WebSocket attaches to a discovered target with `Target.attachToTarget` and `flatten: true`; subsequent page commands include the returned `sessionId` at the top level. Responses and page events retain that session identifier. Domain enablement, pending mouse presses, and navigation replies belong to each connection/session and its target. Protocol detachment invalidates that session without closing the visible page. See the [Target domain](https://chromedevtools.github.io/devtools-protocol/tot/Target/).

Moving a tab between panes/windows preserves its target and sessions. Closing it
fails pending navigation replies, emits `Target.detachedFromTarget` for attached
browser sessions, and closes its direct page sockets. A replacement tab gets a
new ID. Inactive tabs remain addressable. `Target.getTargetInfo` without a target
uses the direct/attached page; on an unattached browser socket it requires an
explicit `targetId` when zero or multiple pages exist. No automatic attach,
target creation/closure/activation, or target-discovery event subscription is
implemented: poll the real target list and attach explicitly.

Embedding hosts call `BrowserCdp::tick_pages(&mut [(&str, &mut Browser)])` with
all live tabs, not just visible panes. It returns an error before processing
commands if IDs are duplicate, recycled, out of allocation order, noncanonical
`page-N`, or exceed 16 live targets. IDs are monotonically allocated and must
remain bound to the same Browser payload. The original `tick(&mut Browser)`
wrapper retains the single `page-1` embedding contract. Transport client, queue,
message, and per-client attached-session limits are unchanged.
Hosts must enable debugging explicitly on each Browser. Before dropping a
BrowserCdp while retaining its Browsers, call `set_debugging(false)` on them.
This retires only exact workers that accepted automation, not ordinary pages
or replacement sessions. The native host does so on protocol failure; normal
Browser teardown already cancels and reaps its workers.

The endpoint grants control of this browser session to local clients. The transport checks loopback Host values; an Origin must be absent or the same loopback HTTP origin and port. Other web origins are rejected. It limits clients, message sizes, pending commands, and output queues; an oversized or stalled client is disconnected. It is not a remote hosting service or an authentication mechanism for other local users.

## Initial subset

The initial local CDP journey passed on 2026-09-07: the external WebSocket client loaded the fixture, entered a Unicode search, submitted the hidden form field, rejected the old document's node ID, clicked the first local result, loaded its destination, and decoded a 1100×683 PNG. A separate browser connection attached, inspected the same page through a flattened session, and detached. The client reported `CDP_JOURNEY_OK`. This verifies that local workflow; it does not establish full protocol or third-party client compatibility.

The same external journey also passes with opt-in scripts at `/script-home` and
`/script-dynamic`. The latter creates its real form through the original Function
constructor and direct JavaScript eval inside the restricted worker. CDP reads
and operates the resulting DOM without requiring Runtime. Recovery after
the local `/script-loop` fuel error is separately checked before onward navigation.

| Domain | Commands |
| --- | --- |
| Browser | `getVersion` |
| Target | `getTargets`, `getTargetInfo`, `attachToTarget`, `detachFromTarget` |
| Page | `enable`, `disable`, `navigate`, `reload`, `getFrameTree`, `getLayoutMetrics`, `captureScreenshot` |
| DOM | `enable`, `disable`, `getDocument`, `querySelector`, `querySelectorAll`, `getAttributes`, `getBoxModel`, `focus` |
| Input | `dispatchMouseEvent`, `dispatchKeyEvent`, `insertText` |
| Runtime | `enable`, `disable`, `evaluate`, `callFunctionOn`, `getProperties`, `releaseObject`, `releaseObjectGroup` |

`Browser.getVersion` identifies mgbrowser and reports `jsVersion: "mgbrowser-js/0.1-experimental"`. This identifies the bundled partial interpreter, not whether script execution is enabled for a document. Protocol fields have their ordinary [Browser-domain meanings](https://chromedevtools.github.io/devtools-protocol/tot/Browser/#method-getVersion).

Each page target uses one top-level frame, real navigation, the actual parser tree, existing editable controls, and the software raster. It does not introduce a second HTML renderer or a JavaScript interpretation shortcut. There are no subframes, worker targets, browser contexts, emulation, full CSS inspection, touch gestures, or downloads. `Page.navigate` requires an absolute HTTP(S) URL, accepts only that target's `frame-N`, and delays its reply until completion. Transport failures and superseded navigations return `errorText`; HTTP error responses still represent delivered documents. `Page.reload` acknowledges initiation, with completion reported through enabled events. Frame metadata separates the fragment-free `url` from optional `urlFragment`, which includes its leading `#`.

`DOM.getDocument` reads the bounded parser arena and implicitly enables DOM events. The default depth is one; `0..256` and `-1` are accepted, with `-1` requesting the available subtree. The serialized root is limited to 4 MiB. Node IDs expire on document replacement; node operations accept `nodeId`, without backend-ID or Runtime-object alternatives. Attributes use alternating names and values. The selector subset supports tag/universal selectors, IDs, classes, attribute presence/equality, compounds, and whitespace descendants. Other selector syntax returns an error. A missing `querySelector` match returns zero. The wire objects follow the [DOM domain](https://chromedevtools.github.io/devtools-protocol/tot/DOM/).

`getBoxModel` returns the union of a node's own or descendant flow boxes. Its content, padding, border, and margin quads are currently equal. The bounds must intersect the viewport but are not clipped to it; wholly offscreen and non-rendered nodes return errors. Geometry collection is capped at 100,000 rectangles; reaching that cap makes box-model queries return an explicit error without a partial model. A query also stops with an explicit error after four million traversal steps. This geometry is useful for existing flow content and does not imply a full CSS box model.

Page coordinates exclude the native toolbar and status strip and remain logical
CSS pixels regardless of display size. At scale `s`, a protocol point `(x, y)`
maps to native canvas `(x*s, (y+64)*s)` with integer raster rounding for the
single-page window. A visible tab strip/pane origin adds its native host offset;
CDP coordinates remain target-local and never include it. The logical page height excludes 64 toolbar and 29 status
pixels since v0.7.1's compact toolbar. Screenshots crop that
viewport in **physical** pixels and encode PNG, with the unchanged 7 MiB encoded
output limit. CSS layout/visual viewport fields stay logical; deprecated device-
pixel fields scale with the output. Visual `scale` and `zoom` remain 1: there is
no separate pinch or page-only zoom. Other image formats, `clip` and
`captureBeyondViewport: true` return errors. Either `fromSurface` value uses the
same software raster. [Page reference](https://chromedevtools.github.io/devtools-protocol/tot/Page/).

Input acts on page controls. Use `DOM.focus` before `Input.insertText`, or a visible box and a mouse press/release. Mouse coordinates must be inside the viewport. Press/release require `button: "left"`; a release within five pixels of that session's press activates once. Only single clicks, unmodified mouse events, and vertical wheel scrolling are supported. Mouse movement does not implement hover or dragging. Coordinates and event parameters follow the [Input reference](https://chromedevtools.github.io/devtools-protocol/tot/Input/).

A scale or viewport-size change cancels a pending mouse press; release cannot
activate a different target after reflow. This does not fabricate document events
or invalidate DOM IDs. Native size shortcuts are not added to CDP's key subset.

Keyboard modifiers support Ctrl=2 and Shift=8; Ctrl+A is the sole control shortcut. Named keys are Enter, Backspace, Tab, PageUp/Down, ArrowUp/Down, Shift, and Control. Windows virtual-key fallback covers Enter=13, Backspace=8, and Tab=9. The `code`, `nativeVirtualKeyCode`, `unmodifiedText`, and `autoRepeat` fields are accepted as validated metadata and do not supply alternative key translation. Use printable `char` text or `insertText` for Unicode input into focused controls. Key release does not edit, and printable raw key-down leaves insertion to a following character event. No clipboard, IME composition, or command-array editing support is provided.

Text commands preflight the resulting editable value, which may hold at most 8191 UTF-8 bytes. A selected value is replaced rather than counted toward retained text. Invalid control characters and excess size return an error without modifying either text or selection. This limit applies to the resulting value, not a separate 16 KiB insertion allowance.

With `--enable-scripts`, real activations now reach retained click/submit handlers
under [the page-session contract](PAGE_SESSIONS.md). Input acknowledgment means the
activation was queued, not that its asynchronous handler/default has finished.
One pending activation is allowed; immediate admission failures (including busy,
known expired or failed sessions) return -32000. A queued activation can still
fail during worker framing, execution or reply validation; that closes the
session without publishing a projection or allowing an unanswered default.
There is no asynchronous Input-completion event in this subset. Editing can
continue while a reply is pending; version acknowledgments
cannot erase newer typing. An accepted event snapshot invalidates all CDP node IDs
and emits only DOM.documentUpdated, not a page load. Requery before further DOM
operations. That retained-event increment itself added no Runtime commands;
the separate source integration is described below.

## Synchronous worker-backed Runtime

This subset operates only an existing accepted main world after LoadComplete,
with debugging and scripting enabled. It cannot enable scripts, start a new
worker, renew allowances or enter the trusted Window-observer/legacy lanes.
Busy/loading/expired/failed realms reject explicitly. Direct page and flattened
routes share the actual realm but have separate owners; handles from another
connection/session/context cannot be used. Context identity survives projections
and native tab moves, not navigation, closure or terminal failure.

`evaluate` executes arbitrary admitted source. `callFunctionOn` compiles a real
function and preserves receiver/argument identity through native guarded calls.
Omit both objectId/executionContextId to select the realm global. Source is at
most16KiB; calls accept at most32 arguments/16KiB encoded arguments. Object groups
are at most64 UTF-8 bytes, with32 live groups. See the full numeric admissions in
[RUNTIME_AUTOMATION.md](RUNTIME_AUTOMATION.md). Existing worker/wire/System/CPU/
source/opcode/task bounds still apply, including64 total transactions and the
300-second absolute lifetime. No reserved automation allowance or restart exists.

Top-level undefined/null/booleans/strings, numeric special values and bounded
BigInts are exact. Returned objects are actual traced worker roots, not DOM node
IDs. By-value output admits bounded ordinary own enumerable data trees/dense
arrays, without getters, proxies, toJSON or coercion. Cycles, holes, symbol keys,
unsupported exotics and compound negative zero reject, never silently normalize.
The current external JSON codec cannot represent unpaired UTF-16 surrogates;
reject them before staged root publication, without lossy replacement.

`getProperties` requires `ownProperties:true` and returns complete own descriptors
in native key order, at most64. It does not invoke accessors/proxies or run a
checkpoint. Accessor functions are real handles. Explicit release drops roots,
never cumulative accounting; unknown/foreign IDs reject and same-owner duplicate
release is idempotent. Disable only stops notifications: release, detach or
disconnect for cleanup. Revoked owners remain capacity-charged until one ordinary
single-flight cleanup is accepted. If existing allowance prevents cleanup, that
same worker is canceled; a replacement is never canceled for an old context.

Parent validation and exactly one active charge precede DOM/result publication.
Control edits preserve newer typing. Ordinary exceptions/representation rejection
retain real preceding effects. Terminal final-sequence results may return data,
never surviving handles; their result/rejection precedes context destruction.
Evaluate/call perform one outer checkpoint after success or ordinary exception;
property/release/cleanup commands run no JavaScript or checkpoint.

Caught exceptions use bounded native diagnostics without reading script-controlled
name/message/stack properties and do not dispatch page onerror. Our explicitly
narrowed ExceptionDetails omits unknown source positions and stacks, unlike the
full upstream required shape. If the thrown value cannot be represented, both
the exception member and command result are absent: not fabricated undefined.
This divergence is intentional and does not imply ordinary client compatibility.

Promise awaiting, utility worlds, user activation, previews, silent execution,
command-line helpers, side-effect analysis and REPL mode are unsupported. Their
semantic flags accept only false; unknown parameters reject. There is no DOM
resolveNode/requestNode bridge, Debugger domain or exception-event stream.
Enabling Runtime acknowledges before reporting existing actual contexts; context
destruction precedes replacement creation. Full Playwright remains a separate
unmodified-client gate, including its still-failing attachment initialization.

Enabled Page sessions receive `frameStartedLoading`, `frameNavigated`, `domContentEventFired`, `loadEventFired`, and `frameStoppedLoading` as the corresponding navigation work occurs. Transport failures omit the two content/load completion events. DOM sessions receive `documentUpdated` when their node references become stale. Attach/detach events describe actual session changes. Navigation errors stay distinguishable from successful content delivery. Event timestamps use monotonic seconds; load events cannot establish that unsupported scripts or images completed.

Since v0.9.11, queued snapshot lifecycle separates these notifications: document
commit does not settle Page.navigate, DCL/load follow accepted worker milestones,
and loading stops on load or explicit failure/abort. An aborted terminal prefix
does not fabricate either missing milestone; later projections emit only DOM
invalidation. Disabled-script parsing/resource completion and the explicit legacy
test lane remain parent-owned. This is queued snapshot lifecycle, not a general
resource-loading barrier, Runtime support or Playwright compatibility. Full
release/public acceptance remains recorded separately in the dated log.

The explicit legacy v1 test lane preserves the old delivered-document contract:
a validated fatal script reply retains its Failed/Script initial outcome and
readable fallback, but does not make successful HTTP delivery a navigation error.
Its parent-owned parse/load events do not claim modern worker milestones.
Infrastructure, transport, supersession and modern terminal-prefix failures
remain distinguishable through `errorText`.

Unknown methods return `-32601`; invalid or unsupported parameters return `-32602`. Invalidated handles and unavailable page operations return a protocol error with a useful explanation. Commands with no return fields still reply with an empty `result` object. A response must contain either `result` or `error`, never both.

## Acceptance and continued development

The multi-target regressions use independent local WebSocket connections and
two real Browser values with equal navigation generations and DOM epochs. They
verify discovery, direct/attached routing, DOM/input isolation, reordered host
enumeration, target-specific events and replies, close/detach cancellation, and
rejected ID reuse. They do not establish ordinary Playwright attachment or
browser contexts or worker-backed Runtime behavior. v0.9.20's separate core and
tab-move native/CDP proofs pass on packaged and public-installed binaries. They
verify real worker closure/property roots and release, exception effects, newer
page form edits, actual paint/navigation and retained roots across native tab
detach/redock/closure. All fourteen actual images were inspected. This authored
client evidence does not satisfy the ordinary Playwright criteria.

Build `workspace_cdp_smoke`, then run
`bash tools/workspace-cdp-smoke.sh /absolute/path/to/mgbrowser` against the
packaged and public-installed binary. It uses native tab creation, detachment
and closure alongside actual WebSocket discovery/attachment/input to check
retained edits, stable routes and closed-target rejection on an owned display.

The last recorded ordinary Playwright 1.58.2 probe, on public v0.9.19, fails
during attachment before any locator or page operation. See
[the actual pinned baseline and acceptance command](PLAYWRIGHT.md).
The Rust client below is not a substitute for that independent-client gate.

Reproduce the local journey in three terminals. Start the fixture service:

```sh
cargo run --locked --example journey_server
```

Open the native browser on that fixture with debugging enabled:

```sh
cargo run --locked -- --remote-debugging-port 9222 http://127.0.0.1:7878/
```

Run the separate client through the public page WebSocket:

```sh
cargo run --locked --example cdp_journey -- ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/ tmp/cdp-journey.png
```

The client has a 30-second deadline and prints `CDP_JOURNEY_OK` only after its assertions pass. It also exercises a separate flattened browser session and checks an unknown Runtime method returns `-32601`. The initial v0.1-era client used Runtime.evaluate for that negative before its implementation; the separate Runtime journey must prove the new real behavior. The fixture server and browser remain open until stopped by the operator. Local evidence from the initial run is in ignored `tmp/cdp-client.log`, `tmp/cdp-browser.log`, and `tmp/cdp-journey.png`.

Continued acceptance must cover the full advertised contract: discovery, direct and attached sessions, request/response IDs, event gating, stale node/session rejection, supported parameters, limits, selectors against the real tree, input focus/text/submission, coordinates after scrolling, and PNG dimensions. Live Google search and first-result navigation remain separate product acceptance; a CDP connection or successful fixture cannot prove those work.

The full-protocol direction is staged around real browser capabilities:

1. Freeze the advertised subset and add independent protocol/client fixtures. Validate every advertised method, optional parameter, result, event, and error path. Pin a reviewed upstream schema revision for future changes; tip-of-tree changes are not automatically adopted.
2. Add real frame/target lifecycles, navigation history, cancellation, Network request/response/error events, resource bodies, and more accurate layout/DOM inspection as their browser implementations mature.
3. Build on the released v0.9.20 synchronous worker-backed Runtime foundation; implement broader execution/Debugger behavior only under separately reviewed contracts. Promise awaiting and utility worlds are not part of this subset. Execution, isolation and actual client tests precede compatibility claims.
4. Connect actual style/compositing and platform features to CSS, DOMSnapshot, Accessibility, Emulation, storage, workers, profiling, tracing, and the remaining relevant domains. Maintain an explicit inventory of unsupported platform-specific features.
5. Run pinned external clients and DevTools frontend versions against documented workflows. Claim compatibility per client/version/workflow only after observed success; retain the full protocol as the longer-term goal.

Official definitions live in the [ChromeDevTools protocol repository](https://github.com/ChromeDevTools/devtools-protocol), with [generated browser schema](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json). This project deliberately publishes its own narrower schema until the underlying capabilities exist and pass their gates.
