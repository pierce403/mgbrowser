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
  `while`/`do`/ordinary `for`, `for-in`, `switch`, `break`/`continue`, `throw`, and
  `try`/`catch`/`finally`, labeled statements and scoped labeled break/continue.
  Function-scoped declarations are hoisted. Tests exercise
  precedence, left-to-right side effects, short circuits, calls and exceptions.
- Basic prototypes and selected Object, Array, String, Number, Boolean, Function,
  Error and Math operations exist. Examples include `call`/`apply`,
  `Object.create` without descriptors, `Object.keys`, array push/pop/slice/join,
  string indexing/slicing, numeric conversion, and the four URI encoding/decoding
  functions. URI conversion preserves UTF-16/UTF-8 semantics and distinguishes
  complete-URI reserved separators from component data. This is not complete builtin
  coverage: exposed but unimplemented methods throw explicit unsupported errors.
- Direct intrinsic `eval` reads the caller's lexical bindings and `this`; aliases,
  member calls and other indirect forms evaluate in global scope. The Function
  constructor compiles separately parsed parameter/body fragments and closes over
  global scope. Neither adds native code execution or a JIT. Non-string eval
  arguments return unchanged; malformed source is a catchable SyntaxError, while
  parser/resource exhaustion remains fatal. Dynamic source containing unpaired
  UTF-16 surrogates is explicitly rejected, not rewritten with replacement characters.
- Original UTF-16 regular expressions support literals, the RegExp constructor,
  `exec`/`test`/`toString`, and String `match`/`search`/`replace`/`split`, including
  capture groups, replacement callbacks and stateful global matching. See below
  for the supported grammar and deliberate limitations.

Unsupported syntax rejects the complete script with a byte-offset diagnostic,
not a successfully parsed prefix. Current exclusions include strict directives,
non-ASCII identifiers, `let`/`const`, arrows, classes, modules, templates,
`for-of`, `with`, `debugger`, object accessors and block-level function
declarations. There is no garbage collector, Promise implementation, module loader or general event
loop. Several Array methods are exposed but not
implemented. The source is authoritative for individual builtin coverage.

Known approximations remain: `arguments` is an unmapped snapshot rather than
non-strict parameter aliasing; property descriptors and host coercion are partial;
number formatting/rounding is not fully specification-compatible. Strings retain
UTF-16 code units inside the evaluator, including lone surrogates; the UTF-8 DOM
and display boundary uses replacement characters for unpaired surrogates, and
lone-surrogate property names are rejected. Error object names/messages are
available, but Error prototype and `instanceof` fidelity remain partial.
`Math.random` is deterministic research
output, never cryptographic randomness. Passing the local tests is not a claim
that supported-looking real-world programs always evaluate correctly.

## Iteration and switch

`for-in` supports a single `var` binding, including an ES5-style initializer, or
an assignable identifier/member reference. The initializer runs before the RHS;
the RHS is evaluated once and the assignment reference is evaluated for each
visited key. Null and undefined produce no iterations. Other primitives are
boxed, with string indices referring to UTF-16 code units. Host-object enumeration,
including DOM handles, is explicitly unsupported.

Enumeration snapshots first-visible owner/name pairs across the prototype chain,
including non-enumerable names that shadow inherited properties. A name is never
visited twice. Within each owner, virtual string/array indices precede stored
insertion-ordered properties; nearer owners precede their prototypes. Before each
visit, the runtime checks that the same owner still exposes an enumerable
property. Deletion or a new nearer shadow skips that candidate; newly added names
wait for a later enumeration. Deleting and readding a property on the same owner
before its visit may visit the replacement. This deterministic research policy
is not a claim of modern property-order or complete descriptor conformance.

`switch` evaluates its discriminant once, checks selectors in source order using
strict equality, and selects default only when no selector matches. Execution
falls through subsequent clause bodies without reevaluating their selectors.
Breaks, loop continues, labels, returns and exceptions preserve their scoped
completion behavior. A switch permits an unlabeled break but does not create a
continue target; function boundaries reset both scopes. Variable declarations in
unvisited branches still hoist. Duplicate defaults, invalid for-in targets and
multiple for-in declarations reject the entire script before effects. Clause
containers count toward parser AST limits, including empty clauses; both new
statement forms participate in dynamic-compilation allocation accounting.

Own-property inspection and enumeration share the virtual metadata rules for
boxed strings, functions and supported native builtins. String indices/length
remain readonly and nonconfigurable, including inherited access. Overwriting a
native builtin preserves its nonenumerability; deletion followed by readdition
creates an ordinary enumerable property. Broader descriptor fidelity is partial.

## Regular expressions

`src/js/regexp.rs` is our own compiler and matcher, not a regex dependency or
another language engine. Supported patterns include literals, dot, character
classes/ranges, builtin classes, anchors/boundaries, alternation, capturing and
noncapturing groups, greedy/lazy quantifiers, backreferences and positive/negative
lookahead. Only `g`, `i` and `m` flags are accepted. Lookbehind, named captures,
Unicode property escapes, legacy octal and quantified assertions are unsupported.
The parser selects the regex lexical goal from grammar context, preserving
division, comments and automatic-semicolon rules. Malformed literals reject the
whole script before effects; invalid constructor patterns throw SyntaxError.

Indices and captures use UTF-16 code units, including lone surrogates. Case
matching uses Rust's Unicode uppercase tables with ES5-style no-expansion and
no-non-ASCII-to-ASCII-fold rules, not a pinned historical Unicode version. RegExp
construction, metadata and String builtin dispatch are ES5-shaped; modern Symbol
protocols and custom `exec` dispatch from String methods are not implemented.
Global String match/replace deliberately advance an empty match by one code unit
at its actual position, avoiding the historical ES5 duplicate-empty-match quirk.
`exec` itself does not advance empty matches. Search/split ignore and preserve
`lastIndex`; replacement callbacks run after the bounded match set is collected.
These choices and the documented resource limits are not full ES5 conformance.

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
| Each parsed script | 1 MiB source; 100,000 tokens and AST nodes; nesting/AST depth 128; Function fragments share these caps |
| Document startup | 32 eligible scripts, including unsupported external/module entries; 32 listeners |
| Evaluator realm | 1,000,000 fuel; 4 MiB cumulative logical allocation; 10,000 objects, functions or environments; call depth 64; array/argument length 10,000 |
| For-in enumeration | Prototype depth 64; candidate snapshots, including non-enumerable shadows, charge shared realm fuel and cumulative allocation; no separate candidate-count cap |
| URI conversion | 1,048,576 UTF-16 units each input/output, with exact encoded-size preflight; allocations also charge the realm budget |
| Regex compilation | 16,384 pattern units; 8,192 nodes; 64 captures; nesting 64; 128 bracket classes; 2 MiB compiled storage; 2,000,000 compile steps; numeric quantifiers at most 1,000,000 |
| Regex matching | 1,048,576 input units; caller's remaining realm fuel; 16,384 tasks; 4,096 pending states; 4 MiB cumulative state-work accounting per find; lookahead depth 16 |
| DOM bridge | 50,000 nodes; depth 256; 4 MiB cumulative logical allocation; 1,024 snapshot collections |
| Serialized DOM / diagnostics | 2 MiB HTML; at most 64 reported errors |
| Worker protocol | 2 MiB request JSON; 4 MiB response JSON |
| Worker OS / parent deadline | 256 MiB address space; 1 CPU second; 2-second wall deadline including transfer/startup |

Parser nesting counts guarded grammar/helper frames, not just visible braces.
The for/switch helpers charge this existing budget to bound their retained Rust
stack frames. Object.keys/getOwnPropertyNames additionally obey the ordinary
10,000-element result-array cap; for-in does not construct such a result array.

Logical allocation accounting is conservative and cumulative, not allocator RSS.
Dynamic compilation charges source conversion and fixed attempt overhead even on
syntax failure, and additionally charges successfully parsed ASTs. Temporary
parser allocations, including partial ASTs discarded on failure, are bounded by
the per-parse limits but are not individually charged as cumulative heap usage.
The OS address-space cap is independent. Evaluator fuel/allocation/call exhaustion
is uncatchable and latched for the realm. The parent bounds pipe traffic, kills
and reaps timed-out/oversized workers, and reports an explicit error.

Regex compile attempts charge a pattern-sized realm reservation, including
syntax failures; successful compiled storage receives an additional charge when
larger than that reservation. Compiler temporaries and matcher state copies have
the independent per-operation bounds above, not exact cumulative realm/RSS
accounting. Returned captures and String output also charge the realm. Match fuel
uses the caller's remaining budget without resetting it; any regex resource error
is fatal and latches the realm, never a successful non-match. Matching uses
explicit task/backtrack stacks and bounded lookahead submachines.

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

The subsequent language increment added labeled control flow, URI builtins and
bounded dynamic compilation. Independent tests cover direct/indirect scope,
coercion ordering, isolated Function grammar fragments, early errors, cross-script
binding attributes and repeated invalid compilation exhausting the allocation
budget. `/script-dynamic` has no static controls: its Function/eval-created form
completed Unicode submission and result/destination navigation through both native
handlers and the external CDP client. The form and destination frames were inspected.

The regular-expression increment passes 18 independent integration groups plus
parser/matcher/runtime regressions. `/script-regexp` creates every form control
using actual capture, lastIndex, replacement and splitting results. Its real
restricted-worker test and native/CDP form-to-destination journeys pass; rendered
form and destination frames were inspected. Another actual worker test verifies
that an invalid literal prevents all prefix DOM effects while a later valid
script still runs. No site-specific patterns or challenge code were inputs.

The for-in/switch increment passed the full local suite of 218 tests on
2026-09-07, including eight actual-worker tests. All three CI journey steps were
also reproduced locally. `/script-iteration` creates its form by enumerating own
and inherited fields and selecting control types with switch; native and external
CDP journeys submitted the Unicode query and hidden field and reached the local
destination. Form and destination screenshots were inspected. Authored cases
cover mutation/shadowing, boxed values, hoisting, scope, completions, early errors
and cumulative resource limits. Publication/remote CI evidence is recorded
separately in the daily log.

The subsequent single Google attempt with `--enable-scripts` still failed. The
HTTP 200 homepage retained 26 rendered items and one form, with two completed
scripts and eight errors, including unsupported submit/onload/onclick behavior,
non-callable values and allocation exhaustion. The actual form submitted, but
the HTTP 200 page titled “Google Search” had no rendered items or forms: two
scripts completed and three errors reported the parser nesting limit. No result
or destination was reached. These are observations of a changing public response,
not an exhaustive diagnosis or a promise that fixing one diagnostic will make
Google work. No site-specific rewriting or challenge logic was added. The
requested first-result journey remains open; see the dated log for evidence.

Next work starts with independently authored nesting cases and a stack/resource-safe
parser architecture; do not blindly increase limits or port site challenge code.
General language/builtin correctness, a pinned independent conformance corpus,
parent-brokered external scripts, persistent realms, real DOM event dispatch/timers
and broader DOM support remain open. CDP Runtime/Debugger remain
unsupported until backed by actual realm and remote-object lifecycles;
`Runtime.evaluate` still returns an unsupported-method error.

Language references are [ECMAScript 5.1](https://262.ecma-international.org/5.1/),
the [current ECMAScript specification](https://tc39.es/ecma262/) and the
[HTML script processing model](https://html.spec.whatwg.org/multipage/scripting.html).
Imported conformance corpora need a recorded revision and license. Extend against
independent local cases, not one site's source; keep private browsing/script
artifacts in ignored `tmp/`, and use only explicit local fixtures in CI.
