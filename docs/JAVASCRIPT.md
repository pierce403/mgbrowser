# Original Rust JavaScript engine

The Google search journey remains the end-to-end goal. Its observed no-JavaScript
response makes real script execution necessary; no alternate engine, fabricated
results, browser impersonation, or Google-specific response rewriting is a substitute.

## Implemented research subset

Scripting is disabled by default. `--enable-scripts` opts into the original Rust
lexer, parser, tree-walking evaluator and DOM bridge in `src/js/` and
`src/js_browser.rs`. The browser executes it only through the restricted
Linux x86_64 worker in `src/script_worker.rs`. No existing parser, JavaScript
runtime, browser engine, or external browser executes page code for us.

The language is a small, non-strict, ES5-like subset, not ECMAScript conformance:

- Expressions include literals, UTF-16 strings, arrays with holes, object
  properties, `this`, member access, calls, `new`, unary/binary operators,
  assignment/update, conditional and sequence expressions.
- Statements include `var`, functions/closures and IIFEs, `return`, `if`,
  `while`/`do`/ordinary `for`, `break`/`continue`, `throw`, and
  `try`/`catch`/`finally`. Function-scoped declarations are hoisted. Tests exercise
  precedence, left-to-right side effects, short circuits, calls and exceptions.
- Basic prototypes and selected Object, Array, String, Number, Boolean, Function,
  Error and Math operations exist. Examples include `call`/`apply`,
  `Object.create` without descriptors, `Object.keys`, array push/pop/slice/join,
  string indexing/slicing and numeric conversion. This is not complete builtin
  coverage: exposed but unimplemented methods throw explicit unsupported errors.

Unsupported syntax rejects the complete script with a byte-offset diagnostic,
not a successfully parsed prefix. Current exclusions include strict directives,
non-ASCII identifiers, `let`/`const`, arrows, classes, modules, templates, regular
expressions, labels, `switch`, `for-in`, object accessors and block-level function
declarations. Dynamic `eval` and the Function constructor are unsupported. There
is no garbage collector, Promise implementation, module loader or general event
loop. URI helpers, several Array methods and String.split are exposed but not
implemented. The source is authoritative for individual builtin coverage.

Known approximations remain: `arguments` is an unmapped snapshot rather than
non-strict parameter aliasing; property descriptors and host coercion are partial;
number formatting/rounding is not fully specification-compatible. Strings retain
UTF-16 code units inside the evaluator, including lone surrogates; the UTF-8 DOM
and display boundary uses replacement characters for unpaired surrogates, and
lone-surrogate property names are rejected. `Math.random` is deterministic research
output, never cryptographic randomness. Passing the local tests is not a claim
that supported-looking real-world programs always evaluate correctly.

## Document execution and DOM capabilities

The worker parses a complete HTML snapshot, then runs eligible classic inline
scripts in source order in one shared realm for that document. This is not the
HTML parser-blocking script model. External `src` scripts and modules report
unsupported errors; inert data scripts are not executed. Dynamically inserted
scripts do not execute. Realm state ends when the document snapshot is returned.

The bridge reads and changes the actual retained DOM tree. It supports connected
element lookup, the documented selector subset, create/append/remove operations,
attributes, ordinary `textContent`/`innerHTML`, title, basic form properties,
base-relative URL getters and limited inline style properties. Collections are
snapshots, not live DOM collections; `innerText` is text extraction, not computed
layout. The renderer still lacks a full CSS implementation. Unsupported mutation
targets, cycles, and excessive depths produce errors instead of pretending to
update the page. Replacing script/style text is unsupported.

`DOMContentLoaded` and `load` callbacks run once after inline scripts, with
`readyState` progressing through loading, interactive and complete. Registered
callbacks run in registration order within each phase; `window.onload` follows
the load listeners. This is a small startup callback mechanism, not DOM event
propagation. Keyboard/mouse events, timers, persistent event handlers, fetch/XHR,
storage and script cookie access are not implemented. Console methods currently
discard their arguments rather than providing a DevTools console.

`location.href`, `location.assign` and `location.replace` can propose
credential-free HTTP(S) navigation. The parent validates the URL and performs the
request with the existing TLS/session stack. These are currently the same
automatic-navigation path, not distinct full history semantics. Script navigation
and HTML refresh share a four-hop limit. No script receives network handles.

Script exceptions may leave earlier DOM changes intact. A serializable snapshot
can be applied with explicit partial-execution errors; successful-script counts
do not imply that the whole page worked. Scripting mode suppresses `noscript`
only when that snapshot is accepted. Disabled scripting, source rejection,
serialization rejection, or worker failure retains the original document with
its ordinary `noscript` behavior. A rejected reply has `applied=false` and no
script navigation. This prevents an invalid worker result from hiding the fallback
document or navigating the parent.

## Process boundary and limits

For each document, the parent starts a fresh copy of the executable with an empty
environment and bounded JSON pipes. Before reading page input, the worker closes
inherited descriptors above 2, installs resource limits and `no_new_privs`, and
loads an x86_64 seccomp filter. After setup it denies new filesystem, network and
process access, executable memory, and changes to the confinement controls.
Reads are restricted to stdin; writes are restricted to stdout/stderr. No
filesystem, process, socket or arbitrary native-call API exists in the language.
The Rust `libc` crate supplies OS declarations, not an alternate runtime/backend.

| Boundary | Current cap |
| --- | --- |
| Script document / URL | 1 MiB HTML; 16 KiB URL |
| Each parsed script | 100,000 tokens and AST nodes; nesting/AST depth 128 |
| Document startup | 32 eligible scripts, including unsupported external/module entries; 32 listeners |
| Evaluator realm | 1,000,000 fuel; 4 MiB cumulative logical allocation; 10,000 objects, functions or environments; call depth 64; array/argument length 10,000 |
| DOM bridge | 50,000 nodes; depth 256; 4 MiB cumulative logical allocation; 1,024 snapshot collections |
| Serialized DOM / diagnostics | 2 MiB HTML; at most 64 reported errors |
| Worker protocol | 2 MiB request JSON; 4 MiB response JSON |
| Worker OS / parent deadline | 256 MiB address space; 1 CPU second; 2-second wall deadline including transfer/startup |

Logical allocation accounting is conservative and cumulative, not allocator RSS.
The OS address-space cap is independent. Evaluator fuel/allocation/call exhaustion
is uncatchable and latched for the realm. The parent bounds pipe traffic, kills
and reaps timed-out/oversized workers, and reports an explicit error.

Isolation is implemented only on Linux x86_64 and requires the kernel controls
above, including `close_range`. Unsupported platforms or failed setup refuse
execution; there is no in-process live-code fallback. The worker is research
containment, not proof of production hardening. HTML parsing, rendering and the
parent network process are not sandboxed by it, and this browser remains
unsuitable for sensitive accounts or arbitrary hostile browsing.

## Evidence and next gates

On 2026-09-07, local language/DOM and worker tests passed. The native Xvfb journey
followed `/script-redirect` to `/script-home`, where an inline script creates all
form controls, then submitted the Unicode query and hidden field and clicked the
local result to `/destination`. The independent Rust CDP journey also completed
that script-built form path after an infinite-loop fixture produced a readable
fuel error and the browser recovered. Captured native/CDP frames were inspected.
These are explicitly authored localhost fixtures, not Google replicas.

The real Google attempt with `--enable-scripts` still failed: its homepage returned
HTTP 200 with partial script errors; the actual form submitted successfully, but
the HTTP 200 page titled “Google Search” produced no rendered result items or
forms. The first observed homepage parser failure involved an unsupported
character/non-ASCII identifier at byte 3277; the search response first failed on
an unsupported labeled statement at byte 463. The journey exited 2. These are
observed failures in changing public responses, not an exhaustive diagnosis or
a promise that implementing those two constructs will make Google work. No
site-specific rewriting or challenge logic was added. The requested first-result
journey remains open; see the dated log for commands and artifacts.

Next work is general language/builtin correctness, a pinned independent
conformance corpus, parent-brokered external script loading, persistent realms,
real event dispatch/timers and broader DOM support. CDP Runtime/Debugger remain
unsupported until backed by actual realm and remote-object lifecycles;
`Runtime.evaluate` still returns an unsupported-method error.

Language references are [ECMAScript 5.1](https://262.ecma-international.org/5.1/),
the [current ECMAScript specification](https://tc39.es/ecma262/) and the
[HTML script processing model](https://html.spec.whatwg.org/multipage/scripting.html).
Imported conformance corpora need a recorded revision and license. Extend against
independent local cases, not one site's source; keep private browsing/script
artifacts in ignored `tmp/`, and use only explicit local fixtures in CI.
