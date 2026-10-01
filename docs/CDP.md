# Browser control protocol

mgbrowser implements a Chrome DevTools Protocol (CDP) interface so tools can inspect and operate the same Rust browser that a person sees. The implementation is a bounded subset with stable targets for existing tabs. The long-term goal is full protocol support as the browser gains the corresponding behavior. Returning success for an unimplemented browser feature does not count as compatibility.

The initial contract is [cdp-protocol.json](cdp-protocol.json). Discovery advertises version `1.3` alongside the mgbrowser product name; this does not claim complete Chrome 1.3, tip-of-tree, Playwright, Puppeteer, or Chrome DevTools frontend compatibility. Production opt-in page scripts use the restricted [Boa page execution lane](BOA.md), but expose no remote execution contexts: `Runtime.evaluate` still returns method-not-found (`-32601`). The [original JavaScript evaluator](JAVASCRIPT.md) remains a historical test baseline. There is no Runtime domain in the advertised subset.

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

The endpoint grants control of this browser session to local clients. The transport checks loopback Host values; an Origin must be absent or the same loopback HTTP origin and port. Other web origins are rejected. It limits clients, message sizes, pending commands, and output queues; an oversized or stalled client is disconnected. It is not a remote hosting service or an authentication mechanism for other local users.

## Initial subset

The initial local CDP journey passed on 2026-09-07: the external WebSocket client loaded the fixture, entered a Unicode search, submitted the hidden form field, rejected the old document's node ID, clicked the first local result, loaded its destination, and decoded a 1100×683 PNG. A separate browser connection attached, inspected the same page through a flattened session, and detached. The client reported `CDP_JOURNEY_OK`. This verifies that local workflow; it does not establish full protocol or third-party client compatibility.

The same external journey also passes with opt-in scripts at `/script-home` and
`/script-dynamic`. The latter creates its real form through the original Function
constructor and direct JavaScript eval inside the restricted worker. CDP reads
and operates the resulting DOM; it does not evaluate code itself. Recovery after
the local `/script-loop` fuel error is separately checked before onward navigation.

| Domain | Commands |
| --- | --- |
| Browser | `getVersion` |
| Target | `getTargets`, `getTargetInfo`, `attachToTarget`, `detachFromTarget` |
| Page | `enable`, `disable`, `navigate`, `reload`, `getFrameTree`, `getLayoutMetrics`, `captureScreenshot` |
| DOM | `enable`, `disable`, `getDocument`, `querySelector`, `querySelectorAll`, `getAttributes`, `getBoxModel`, `focus` |
| Input | `dispatchMouseEvent`, `dispatchKeyEvent`, `insertText` |

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
operations. This adds no commands, remote execution context or Runtime evaluation.

Enabled Page sessions receive `frameStartedLoading`, `frameNavigated`, `domContentEventFired`, `loadEventFired`, and `frameStoppedLoading` as the corresponding navigation work occurs. Transport failures omit the two content/load completion events. DOM sessions receive `documentUpdated` when their node references become stale. Attach/detach events describe actual session changes. Navigation errors stay distinguishable from successful content delivery. Event timestamps use monotonic seconds; load events cannot establish that unsupported scripts or images completed.

Unknown methods return `-32601`; invalid or unsupported parameters return `-32602`. Invalidated handles and unavailable page operations return a protocol error with a useful explanation. Commands with no return fields still reply with an empty `result` object. A response must contain either `result` or `error`, never both.

## Acceptance and continued development

The multi-target regressions use independent local WebSocket connections and
two real Browser values with equal navigation generations and DOM epochs. They
verify discovery, direct/attached routing, DOM/input isolation, reordered host
enumeration, target-specific events and replies, close/detach cancellation, and
rejected ID reuse. They do not establish ordinary Playwright attachment or
browser-context/Runtime support.

Build `workspace_cdp_smoke`, then run
`bash tools/workspace-cdp-smoke.sh /absolute/path/to/mgbrowser` against the
packaged and public-installed binary. It uses native tab creation, detachment
and closure alongside actual WebSocket discovery/attachment/input to check
retained edits, stable routes and closed-target rejection on an owned display.

Ordinary Playwright 1.58.2 currently fails during attachment, before any locator
or page operation. See [the actual pinned baseline and acceptance command](PLAYWRIGHT.md).
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

The client has a 30-second deadline and prints `CDP_JOURNEY_OK` only after its assertions pass. It also exercises a separate flattened browser session and checks that `Runtime.evaluate` is explicitly unsupported. The fixture server and browser remain open until stopped by the operator. Local evidence from the initial run is in ignored `tmp/cdp-client.log`, `tmp/cdp-browser.log`, and `tmp/cdp-journey.png`.

Continued acceptance must cover the full advertised contract: discovery, direct and attached sessions, request/response IDs, event gating, stale node/session rejection, supported parameters, limits, selectors against the real tree, input focus/text/submission, coordinates after scrolling, and PNG dimensions. Live Google search and first-result navigation remain separate product acceptance; a CDP connection or successful fixture cannot prove those work.

The full-protocol direction is staged around real browser capabilities:

1. Freeze the advertised subset and add independent protocol/client fixtures. Validate every advertised method, optional parameter, result, event, and error path. Pin a reviewed upstream schema revision for future changes; tip-of-tree changes are not automatically adopted.
2. Add real frame/target lifecycles, navigation history, cancellation, Network request/response/error events, resource bodies, and more accurate layout/DOM inspection as their browser implementations mature.
3. Expand the Boa-backed Rust execution facade and web bindings, then expose Runtime execution contexts, object handles, evaluation, exceptions, and Debugger behavior. Internal retained page realms now support bounded later activation, but expose no remote Runtime context. Execution and isolation tests precede compatibility claims.
4. Connect actual style/compositing and platform features to CSS, DOMSnapshot, Accessibility, Emulation, storage, workers, profiling, tracing, and the remaining relevant domains. Maintain an explicit inventory of unsupported platform-specific features.
5. Run pinned external clients and DevTools frontend versions against documented workflows. Claim compatibility per client/version/workflow only after observed success; retain the full protocol as the longer-term goal.

Official definitions live in the [ChromeDevTools protocol repository](https://github.com/ChromeDevTools/devtools-protocol), with [generated browser schema](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json). This project deliberately publishes its own narrower schema until the underlying capabilities exist and pass their gates.
