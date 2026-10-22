# mgbrowser v0.9.10 Experimental Preview

Bounded Window self-messaging on the existing browser engine. Local tests,
exact-commit CI, tagged packaging and public install/update/native verification
are separate required gates, recorded in the
[dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
The installer selects the latest actually published release.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Window messages

- `postMessage`, `message`/`messageerror` listeners and function-valued handler
  slots deliver genuine later native events through parent-issued tasks, not
  synchronous callbacks or Promise aliases. Initially only the actual current
  Window is addressable. Default `/`, wildcard `*` and normalized explicit
  target origins work; delivery rechecks the target. Sender origin comes from
  the trusted document, not aliases, a base URL or script-error exposure policy.
- The partial serializer supports primitives, ordinary objects and arrays,
  preserving UTF-16 strings, BigInt, NaN, negative zero, cycles, aliases, own
  enumerable string properties and sparse array length/holes. It takes a
  synchronous snapshot with depth-first getter order, before queuing the task.
  Source identities remain rooted during getters/GC, then release. Queued graphs
  use owned buffers and scalar edges, without reference-counted graph cycles.
- Symbols, functions, Proxy, Promise, weak collections, actual Window/DOM
  wrappers, detached buffers and non-isolated shared buffers throw DataCloneError.
  Other cloneable builtins, including Date, RegExp, Map/Set, boxed values,
  buffers/views and Error, explicitly throw NotSupportedError. Valid nonempty
  transfer lists are unsupported and detach nothing; invalid entries and
  duplicates still fail correctly. There is no global structuredClone API.
- Genuine native DOMException objects preserve Error identity, native stack
  support, UTF-16 name/message, legacy codes/constants and private receiver
  branding. Page replacement of the global constructor cannot forge host errors.
- Delivered events expose cloned data, sender origin, actual Window source,
  empty lastEventId and frozen empty ports. They are non-bubbling/noncancelable;
  handler false and preventDefault do not cancel them. Listeners share the
  actual event/data identity. Ordinary restoration failure dispatches messageerror;
  resource and internal-invariant failures stay terminal. Callback errors retain
  normal host diagnostics. Rejection privacy purging preserves queued messages.

See the [partial-domain contract](https://github.com/pierce403/mgbrowser/blob/v0.9.10/docs/WINDOW_MESSAGES.md).
Cross-context routing, MessagePort, Worker/BroadcastChannel, full structured
clone, MessageEvent/Event constructors and synthetic dispatch remain unsupported.

## Limits and standards scope

Messages share the existing 256 pending / 2,048 lifetime work admissions with
Promise jobs, timers and rejection notices. Cancellation never refunds lifetime
admission. Other unchanged limits include 32 cumulative listeners, 256 events,
1,000,000 cumulative opcodes, 1 MiB source / 4 MiB lifetime source admission,
32/64 MiB outstanding/cumulative requested System allocation, a 4 MiB DOM
allowance, one CPU second and 256 MiB address space. Sessions retain two total
active seconds, 300 seconds absolute lifetime, 64 transactions, 2 MiB request /
4 MiB response frames and 32 MiB aggregate wire. No syscall, filesystem/network
authority, protocol field, third-party dependency or source pin is added.

These are worker/session or per-Browser limits, not whole-workspace quotas,
GC live heap or RSS. Parser/native/regex/GC work still requires hard process
containment. Fatal VM roots can remain until actual Context teardown; the
browser drops terminal workers and embedders must discard Fatal/Closed realms.

The existing pinned static selection is 1,011 upstream flexbox reftests;
its last published result is 317 PASS, 632 FAIL and 62 UNSUPPORTED. Every commit
requires a fresh full gate and exact-source public report. Corpus, assertions,
expectations and scoring are unchanged; authored messaging controls are not WPT
passes. This is not JavaScript testharness support or complete Window readiness.

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256, installs without sudo to `~/.local/bin`, and
adds the desktop launcher and Mg icon. `MGBROWSER_INSTALL_DIR` overrides the
binary destination; `XDG_DATA_HOME` overrides launcher/icon/license data.
A DejaVu/Liberation font is required; `MGBROWSER_FONT` selects a custom font.
Checksums are not independent release signatures. Packages must fit older
updaters' unchanged 8 MiB compressed / 64 MiB unpacked bounds.

Installed builds check for updates. **Menu > About > Restart now** is optional;
it reopens committed tab URLs, not unsaved edits, cookies, history or pane layout.
`--no-auto-update` disables background checks. No background service is installed.

## Known limitations

- Modern-web compatibility is poor; Google search to first result is not working.
  Do not use this preview for banking, sensitive authenticated browsing or
  arbitrary hostile websites. The restricted JavaScript worker is not a sandbox
  for the browser as a whole.
- Linux x86_64 on X11/XWayland is the GUI target. Cookies are memory-only.
  There is no credential store, HTTP/2, proxy configuration or persistent session.
- Pure-Rust Boa scripting runs by default (`--disable-scripts` opts out).
  Bounded startup classic external scripts work, but async, integrity and
  crossorigin attributes are skipped with diagnostics. Modules, dynamic script
  injection, fetch/XHR, string timers, intervals and React/Vue browser apps remain
  unsupported. Startup lifecycle is still bundled in Init; no streaming parser,
  fully task-separated lifecycle or typed WPT observer is claimed.
- Full CSS is not implemented. Flex/grid/positioning/stacking and image/font
  resources are bounded subsets. CSS imports, transforms, rounded corners,
  complete line clamping and independent element scrolling remain incomplete.
  Google News has fidelity gaps, placeholders and destination fallback; search,
  account actions and personalization are not acceptance claims.
- Ordinary Playwright compatibility remains unfinished. Partial CDP and the
  native read-only Inspector are not full Chrome DevTools or a JS debugger.
- Source-error muting conservatively suppresses the whole realm after foreign,
  unknown or dynamically compiled sources. Error locations remain unknown.
  Rejection observation gaps stay explicit; cancellation cannot erase diagnostics.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old v0.1.0/v0.1.1 artifacts
remain MIT and all published tags/assets remain immutable.
