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
| Expression parser state | Per Parser: 1,536 live continuation instructions, including the active instruction; 3,200,000 cumulative dispatch steps |
| Document startup | 32 eligible scripts, including unsupported external/module entries; 32 listeners |
| Evaluator realm | 1,000,000 fuel; 4 MiB cumulative logical allocation; 10,000 objects, functions or environments; call depth 64; array/argument length 10,000 |
| Evaluator retained entries | 128 active expression entries; 384 combined expression, statement and call entries, shared across re-entry |
| For-in enumeration | Prototype depth 64; candidate snapshots, including non-enumerable shadows, charge shared realm fuel and cumulative allocation; no separate candidate-count cap |
| URI conversion | 1,048,576 UTF-16 units each input/output, with exact encoded-size preflight; allocations also charge the realm budget |
| Regex compilation | 16,384 pattern units; 8,192 nodes; 64 captures; nesting 64; 128 bracket classes; 2 MiB compiled storage; 2,000,000 compile steps; numeric quantifiers at most 1,000,000 |
| Regex matching | 1,048,576 input units; caller's remaining realm fuel; 16,384 tasks; 4,096 pending states; 4 MiB cumulative state-work accounting per find; lookahead depth 16 |
| DOM bridge | 50,000 nodes; depth 256; 4 MiB cumulative logical allocation; 1,024 snapshot collections |
| Serialized DOM / diagnostics | 2 MiB HTML; at most 64 reported errors |
| Worker protocol | 2 MiB request JSON; 4 MiB response JSON |
| Worker OS / parent deadline | 256 MiB address space; 1 CPU second; 2-second wall deadline including transfer/startup |

Expression parsing now uses heap-backed continuations rather than recursive
precedence-helper descent. Its shared structural guard counts recursive grammar
operands/containers, not each pass-through helper; 64 grouping pairs are supported
without adding AST wrappers. Guarded statement/function and for/switch helper
frames still share the same nesting budget, so the limit is not just a count of
visible braces. Object.keys/getOwnPropertyNames additionally obey the ordinary
10,000-element result-array cap; for-in does not construct such a result array.

Logical allocation accounting is conservative and cumulative, not allocator RSS.
Dynamic compilation charges source conversion and fixed attempt overhead even on
syntax failure, and additionally charges successfully parsed ASTs. Temporary
parser allocations, including partial ASTs discarded on failure, are bounded by
the per-parse limits but are not individually charged as cumulative heap usage.
The OS address-space cap is independent. Evaluator fuel/allocation/depth exhaustion
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

### Compact AST and retained-storage boundary

The independently measured storage change boxes only ordinary For test/update
expressions and the ForIn binding, reducing the x86_64 statement layout from
144 to 80 bytes without adding language nodes or logical depth. Successful AST
admission counts root/block/case vector capacity, expression/tuple capacity
including holes, boxed children and separately owned String/UTF-16 buffers.
Inline children already occupy their containing slots and contribute only owned
descendant storage. Shared function slices pay their length-based payload and
two-word reference-count control allowance once per parse, not per closure.

The accounting contract adds an explicit 16-byte logical overhead to each
allocated block (including empty Rc slices); zero-capacity vectors/strings have
no allocation. Arithmetic saturates. This is conservative retained requested
storage accounting, not allocator RSS or a proof of native allocator overhead.
Source/attempt and real-copy charges, cumulative failure latching, all caps and
parser semantics remain unchanged. Sparse holes and spare capacity must receive
higher charges than the old per-present-node policy. Parser temporaries and
failed partial trees remain independently bounded as documented above.

Before implementation, the unchanged authored 730-byte `/script-ast` HTML
fixture generated 19,998 harmless statements and 19 form-building statements.
The actual restricted worker rejected 4,492,773 AST bytes after 864,771 accepted
bytes, leaving readable fallback and no controls. Its bytes are frozen for this
increment. It now completes one script without errors at 2,481,036 accepted bytes
(Ast 1,615,150), creating the actual query/hidden/submit controls. Native and
external CDP journeys submit the Unicode query and reach the local destination;
both rendered frames were inspected. These runs perform different amounts of
work, so accepted totals are not an equal-work memory comparison.

An independent Rust allocator wrapper measures retained allocation requests after
parsing the same ten authored inputs. Every new AST charge exactly matches the
measured bytes plus the stated 16-byte allowance per live allocation block.
For the unchanged 20,000-statement retained function, requested storage falls
from 2,880,612 to 1,600,356 bytes, with 1,600,420 charged. A 10,000-hole uncalled
array instead rises from the old incorrect 388-byte charge to 918,020, covering
917,940 retained bytes and capacity for 16,384 slots. Dense 4,096→4,097 arrays
also pay their full backing-capacity increase. These are x86_64 requested-storage
measurements, excluding parser temporaries and allocator/RSS overhead.

Ten private storage groups cover all AST variants, capacity, boxes, empty shared
slices, alignment and saturation. Nine independent integration groups verify a
20,000-statement function remains callable, capacity transitions, loop execution,
shared closure identity/lifetime, repeated ordinary/eval/Function parsing, early
AST rejection and fatal latching. New parser groups preserve exact node/depth
counts and clone/drop at depth 128 without enlarging native stacks. A real-worker
sparse negative control rejects a 917,689-byte AST request after 3,993,182 accepted
bytes, retaining fallback without catch/finally/later-script/navigation effects.

Three old assertions required reviewed accounting updates: the 24,000-statement
input now fits, so the prefix/hoist rejection case uses a 40,000-statement tree;
five sparse-literal evaluations fit while the original six-evaluation input is
retained as a fatal regression; and the shared-factory worker asserts actual
10,000 statement-slot storage instead of the obsolete two-MiB node weight.
Dense literals, six native Array constructions and all semantic/failure checks
remain. All 368 debug tests, 258 selected release checks and all three exact CI
journey steps pass locally (11 native and 11 external CDP destinations).
Live/publication evidence follows in the dated log.

### Sole Function parameter-source ownership

The narrow ownership change moves the already-owned UTF-16 buffer when a
Function constructor has exactly one parameter fragment after all ToString
conversions and body removal. No joining is needed in that case. Zero/multiple
fragments retain their current preflighted joining policy, including commas.
Real source creation, UTF-8 conversion, ingress, parse attempts and every limit
remain charged; coercion completes in the same order before grammar validation.

Independent tests verify pointer/capacity preservation for the sole buffer, unchanged
zero/multiple-fragment charges, independent ASCII-padding slopes of Runtime two
and Source one bytes/unit (formerly two/three), a 900,000-unit callable case,
separate parameter/body grammar, unpaired UTF-16 source rejection and fatal cumulative limits.
The authored `/script-sources` form uses a generated 749,925-unit whitespace
fragment. Before the change its worker rejected Source 749,925 after 3,664,599
accepted bytes. At that increment, the unchanged fixture created and used real controls below
4 MiB: 2,935,365 accepted bytes, one completed script and no errors. Native and
external CDP journeys submit its real query/hidden field and reach the local
destination; native query and CDP destination frames were inspected. These runs
complete different amounts of work and are not an RSS comparison.

That increment passed 344 debug tests, 234 selected release checks and all three
exact CI journey steps locally on 2026-09-07. Twelve independent source groups and three new
private ownership/preflight groups cover the contract; 21 actual-worker groups
include a multi-fragment control that still pays for real joining and rejects
UTF-8 allocation without body/catch/finally/later-script effects. Parser source
limits still reject excessive source even when heap admission fits. No existing
tests, caps, dependencies, worker capabilities or CDP commands changed.
Publication evidence is recorded separately in the daily log.

### Parameter-copy binding ownership

The bounded ownership change removes only the payload surcharge when a
fresh, charged formal-parameter copy moves into its local binding. A private
copy-and-bind helper performs the real copy itself before passing ownership
to a non-global local-storage helper. Ordinary `define`, global/property ingress,
caught host-error strings, metadata, identifier reads and all limits retain their
charges. No generic already-paid flag or public admission bypass is allowed.

Independent tests verify public-invoke formal/no-formal string slopes of
4/2 bytes per additional UTF-16 unit, a 900,000-unit formal argument below 4 MiB,
distinct parameter/original pointers, preserved duplicate/missing parameters and
unmapped arguments, unchanged ingress/catch/real-copy charges and fatal latching.
The authored `/script-bindings` fixture creates controls only after binding a
749,925-unit generated string. Its baseline worker rejects a 1,499,850-byte
Runtime move charge after 3,678,037 accepted bytes. At that increment, the unchanged fixture
completed with 3,681,962 accepted bytes, one script and no errors; its real query
and hidden field submit through native and external CDP input to the local result
and destination. The old run stopped before creating controls, so these totals
are not equal-work or RSS measurements.

That increment passed 327 debug tests, 217 selected release checks and all three
exact CI journey steps locally on 2026-09-07. This includes ten independent binding groups,
three new private copy/move/preflight groups and 19 actual-worker groups. A larger
worker input still fails on the required actual parameter clone and prevents
body/catch/finally/later-script effects while preserving readable fallback.
Native query and CDP destination frames were inspected. The earlier array
increment's formal slope of six is historical: it is now four, with no-formal
two unchanged. Other bindings/property transfers and AST policy were unchanged
in that parameter-copy increment.
See the daily log for separate exact-SHA remote acceptance.

### Prepaid array and argument ownership

Array construction now uses a private array builder: reserve each
64-byte logical slot before allocating/growing its value vector, then consume
that builder when creating the object. Known-size producers prepay their exact
count; growing result arrays prepay bounded geometric capacity (at most 10,000
slots), retaining unused credits without refunds. This preserves amortized growth
instead of reallocating for every result element. Adoption charges the existing
128-byte object metadata, not the already-paid slots or moved string payloads again.
Only audited producers may transfer owned payloads without creating new copies;
their existing creation/copy/ingress charges must remain. Array literals,
numeric/element constructors, slice, key results, regex results, split and
user-function argument snapshots retain their individual
payload and phase accounting. No bare vector may bypass slot admission.

User-function parameters still receive their existing independent copies and
bindings before the original owned argument values move into the unmapped
arguments snapshot. A script call's input vector and its separate snapshot vector
remain charged; public `invoke` admits incoming payloads and separately pays the
new snapshot storage. Holes versus undefined, identity, extra/missing/duplicate
parameters, the parameter named arguments, callee and post-return lifetime are
preserved.
Host ingress, property/binding transfers, real reads/clones, regex reservations,
all realm/parser/worker caps and first-failure latching are unchanged. This is
construction/adoption accounting; later array-mutation charges remain unchanged.
Credits describe requested logical slots, not exact allocator capacity or RSS,
and this is not a new garbage collector.

The array increment passed all 312 debug tests and 202 selected release tests on 2026-09-07, including
17 independent array groups, five private ownership/preflight/growth groups and
17 actual-worker groups. Pointer checks verify that adoption moves the slot vector
and string buffers, and that parameters retain an independent copy. Authored
public-invoke no-formal/formal snapshots then retained charges of 2/6 bytes per added UTF-16
unit; slice still pays for its real string copies. Growing result arrays reserve
at most 15 times through 10,000 slots, with rejection before failed growth.

The `/script-arrays` fixture retains six independent 10,000-slot arrays before
creating any controls. Its baseline failed with 3,883,859 accepted bytes and a
rejected 640,000-byte Runtime charge. At that increment, the complete worker accepted 3,891,459
bytes, with one completed script and no errors; a seventh maximum array still
fails cumulatively without catch/finally/later-script effects. These are logical
charges, not a peak-memory comparison: the old run stopped before doing all work.
Native and external CDP journeys submit its real query/hidden field and reach the
local destination. All three exact CI journey steps pass locally; native query
and CDP destination frames were inspected. Publication evidence is in the log.
AST representation and broader property-transfer policies remain separate proposals.

### Allocation diagnostics and shared function storage

`Runtime::allocation_report()` returns a fixed-size host-readable snapshot:
the unchanged 4 MiB limit, accepted total, exclusive phase totals (bootstrap,
source, successful AST, function code, runtime storage/copies, regex compilation
and regex results), and the first rejected charge. Rejection records its phase,
accepted total and requested bytes without changing successful totals. Later
scripts retain that first failure rather than relabeling the latched error.
The report contains no source text, identifiers, URLs, timestamps or growing
history. This is conservative cumulative accounting, not measured allocator RSS.

Phases classify explicit charge sites, not inclusive operation costs. Shared
object/property metadata remains Runtime even inside function/regex operations;
callbacks use their own charge sites, not the caller's phase. The accepted phase
sum equals the accepted total. The parent rejects invalid reports before applying
the worker's projection. Reports are optional: rejection before realm creation
has none, while rejected DOM serialization can retain a snapshot. Absence is not
zero allocation. `SCRIPT_ALLOCATION` prints only for validated, applied replies.
No script-visible host API, worker capability or CDP command was added.

Function parameter/body slices now use `Rc` ownership from the parsed AST,
instead of deep-copying immutable code for each function instance. Closures keep
fresh identity, captured environments, properties and prototypes. Each separate
parse still pays for its entire tree, including shared-slice metadata, before
effects; per-instance function metadata remains charged. Independently parsed
trees are not interned. Native dispatch avoids first-argument copies only where
unused; audited string operations return already-prepaid buffers without charging
them again. True clones, host ingress, binding/property storage, parser attempts
and all existing limits remain charged. Generic array adoption, property/binding
transfers and regex reservation policies are unchanged.

On 2026-09-07 all 288 debug tests and 178 selected release tests passed on default
stacks, including 24 independent allocation groups and 15 actual-worker groups.
These cover phase sums/monotonicity, first-failure latching, malformed reports,
large ASTs/functions, repeated compilation, closures, pointer/lifetime behavior,
UTF-16/coercion order, true-copy charges and fatal-limit enforcement.

The authored `/script-allocation` factory contains 9,999 harmless statements and
creates five real DOM controls. Before sharing, its worker rejected a 2,240,584-byte
function-code charge; after sharing, the complete run accepts 3,745,970 bytes,
including 137 bytes of per-instance function-code metadata. The unchanged fixture
passes actual-worker, native-window and external CDP form/result/destination
journeys. All three exact CI journey steps also pass locally; native query and
CDP destination frames were inspected. Publication evidence is in the daily log.
These local results do not complete the live Google journey.

### Bounded expression-state increment

`src/js/syntax/expressions.rs` implements the expression grammar with explicit
continuation/operator state and one result register. Each pending operand is an
already depth-checked AST. Storage grows on demand, with at most `12 × 128 = 1,536`
live instructions shared across all expression machines in one Parser. This
includes the popped instruction while a nested function body is parsed; a
function cannot reset the outer machine's counters. The allowance derives from
the grammar's forwarding stages, structural entry/leave, container return and
active-instruction bookkeeping, not an increased source or nesting cap.

Dispatch work is independently capped at `32 × 100,000 = 3,200,000` transitions
per Parser. Success and error paths restore the invocation's pending-state and
nesting counters, but never refund work already performed. Parser nesting, AST
depth and AST-node exhaustion now retain their resource-error classification even
when the lexer has already cached a malformed-token diagnostic. All parser-owned
resource errors remain fatal through eval and Function construction; ordinary
syntax errors remain catchable and never return a successfully parsed prefix.

That expression-machine increment preserved the AST/API, 128 structural/AST-depth limits, source/token/node caps, function and
statement guards, grammar-directed regex/division, NoIn contexts,
precedence/associativity and ASI. Function parameter/body fragments
remain separately parsed. Statement/function parsing and AST
cloning/evaluation/disposal still use guarded recursive code: this is not a fully
iterative engine or a measurement of native stack bytes. No thread-stack size was
increased, and no website script/challenge source was an implementation input.

On 2026-09-07, the final debug suite passed all 251 tests: 107 library, 15 binary,
16 control-flow, 12 DOM, 21 dynamic, 22 expression, 24 iteration, 18 regex,
12 actual-worker and four example/client tests. The library includes all 30 parser
groups, including shared-state cleanup and nested-function limits. The exact
release command `cargo test --locked --release --lib --test js_expressions --test script_worker`
passed all 141 selected tests (107 library, 22 expression and 12 worker).
Formatting, build and the native-dependency guard passed as well.

These default-stack checks cover 64 grouping levels in ordinary code,
direct/indirect eval and Function bodies, mixed expression forms, lexical goals,
early rejection and cumulative limits. Actual workers verify the authored
`/script-expressions` factory, malformed grouping without prefix DOM effects,
excessive-depth latching and recursion errors that cannot enter catch/finally or
execute a later script.

Independent review exposed evaluator stack overflows with pending unary
expressions and deeply nested switch/for-in helpers. The evaluator now caps
active expression entries at 128 and combined expression/statement/call entries
at 384 across function and native re-entry, alongside the unchanged 64-call cap.
Mechanical helper splits isolate variable/labeled statements, for-in preparation
and assignment, switch selection and eight expression dispatch arms, reducing
temporary native frames retained through recursive execution. No cap or thread
stack was increased.

All 11 heavy default-stack forms pass, including 48 nested switches with 24 unary
operators and 96 nested if-statements with 24 unary operators. Existing 64-call
regressions still pass. Entry counters return to zero after successful execution,
catchable exceptions and fatal errors; exhaustion bypasses catch/finally and
latches the realm against later execution. These are verified logical guards,
not native-stack byte accounting or a production stack-safety claim.

The native and external CDP `/script-expressions` journeys passed: both submitted
the real Unicode query and hidden field, clicked the local result and reached
the HTTP 200 destination. All three exact CI journey steps passed locally,
including existing scripted forms and loop-error recovery. Native query and CDP
destination frames were inspected and readable. See [RUNNING.md](RUNNING.md) for
the authored fixture commands and the daily log for remote publication evidence.

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

The post-iteration Google checkpoint, before the expression-parser increment,
used `--enable-scripts` and still failed. The
HTTP 200 homepage retained 26 rendered items and one form, with two completed
scripts and eight errors, including unsupported submit/onload/onclick behavior,
non-callable values and allocation exhaustion. The actual form submitted, but
the HTTP 200 page titled “Google Search” had no rendered items or forms: two
scripts completed and three errors reported the parser nesting limit. No result
or destination was reached. These are observations of a changing public response,
not an exhaustive diagnosis or a promise that fixing one diagnostic will make
Google work. No site-specific rewriting or challenge logic was added. The
requested first-result journey remains open; see the dated log for evidence.

The post-expression checkpoint completed the real Google form submission with
verified TLS and ordinary cookies. Search returned HTTP 200, title “Google
Search”, two completed scripts and three allocation-budget errors, with no
rendered items or forms. The earlier parser-nesting diagnostic was absent in
this response, but the inspected frame was still blank and the journey exited
2 without a result or destination. Local success does not complete the live goal.

After allocation diagnostics and sharing, the actual Google homepage has no
rejected allocation (2,468,375 accepted bytes), with three completed scripts and
seven unsupported-behavior errors. The real form still submits normally. Search
returns HTTP 200 but rejects a 2,575,110-byte AST charge after 2,390,987 accepted
bytes; three script errors repeat the same first failure. Its inspected frame has
no items/forms, and the journey exits 2 without a result or destination.

Next work audits remaining AST representation and runtime ownership/copy costs
using independently authored fixtures. The counters identify an admission gap,
not permission to remove real charges or raise limits. No site challenge code is
an implementation input.

The post-array checkpoint still returns no Google result links. Homepage HTTP 200
retains its real form, three completed scripts/seven errors and no rejected
allocation. Search HTTP 200 accepts 1,822,051 bytes before rejecting the same
2,575,110-byte AST request; Runtime charges are 1,516,976 bytes. The remaining
admission gap is 202,857 bytes. The inspected search frame is blank and the
journey exits 2 without a first destination. Responses can vary; the reduced
charges do not establish compatibility beyond that observed stage. Next audit
targets narrow paid-value transfers and container-aware AST costs, with existing
ingress/copy charges and all caps preserved until independently justified changes.
General language/builtin correctness, a pinned independent conformance corpus,
parent-brokered external scripts, persistent realms, real DOM event dispatch/timers
and broader DOM support remain open. CDP Runtime/Debugger remain
unsupported until backed by actual realm and remote-object lifecycles;
`Runtime.evaluate` still returns an unsupported-method error.

After parameter-copy binding, one bounded Google checkpoint still returns no
results. The homepage retains its real form with three completed scripts/seven
errors and no rejected allocation (2,448,406 accepted bytes). Search HTTP 200
has zero items/forms and two completed scripts/three errors repeating one AST
failure: 1,684,603 accepted bytes plus a requested 2,575,110 exceeds 4,194,304.
Runtime charges are 1,379,528 bytes; the admission gap is now 65,409 bytes. The
inspected search frame remains blank and the journey exits 2 without a result or
destination. Changing responses are not a controlled performance comparison or
proof that another optimization completes compatibility. Next work remains an
independent ownership/AST audit with true-copy/ingress charges and all caps intact.

The post-source-ownership live checkpoint is unchanged at search admission:
HTTP 200, no items/forms, two completed scripts and three errors repeating the
same AST failure (accepted 1,684,603/requested 2,575,110/limit 4,194,304). The
65,409-byte gap and all accepted search-phase totals match the preceding
checkpoint. The homepage still supplies its real form. The inspected search frame
is blank; exit 2, no actual result or destination. This optimization has no observed
benefit on that served search response, and does not complete the Google goal.

The subsequent separately reviewed AST representation/accounting change is
implemented above. Its pre-change authored x86_64 allocator probe measured
144-byte statements; boxing only For test/update expressions and the ForIn
binding projected an 80-byte statement. A retained 20,000-statement function
requested 2,880,612 storage bytes and failed a 4,480,164-byte AST charge;
the layout-only projection was 1,600,356 storage bytes.
These were requested retained allocations and layout projections, not parser
temporaries or RSS. Lowering fixed weights alone would have been
incorrect: an uncalled function with 10,000 array holes retained 918,260 bytes,
including capacity for 16,384 slots, while its old AST charge was only 388.
The new boundary charges container capacity, holes, new boxes and separately
owned payloads without counting inline values twice; some charges increased.
Source/attempt accounting, real copies, grammar and all caps remain unchanged.

After the AST change, the bounded live checkpoint still produces no Google
result links. Homepage HTTP 200 retains its real form and no rejected allocation
(2,255,667 accepted bytes). Search HTTP 200 completes two scripts with three
errors: first `ReferenceError: Symbol is not defined`, then an AST rejection
requesting 387,844 after 4,078,595 accepted bytes, repeated by the later script.
Accepted search Ast is 2,438,297 and Runtime 1,451,075. The earlier first admission
failure is no longer the leading diagnostic, but the inspected search frame is
still blank and exit 2 records no result or destination. Next work targets genuine
Symbol/value/property-key semantics against independent authored cases, with
separate cumulative-storage diagnosis; no string shim, fake API success or site
challenge adaptation. Changing live responses are not controlled benchmarks.

Language references are [ECMAScript 5.1](https://262.ecma-international.org/5.1/),
the [current ECMAScript specification](https://tc39.es/ecma262/) and the
[HTML script processing model](https://html.spec.whatwg.org/multipage/scripting.html).
Imported conformance corpora need a recorded revision and license. Extend against
independent local cases, not one site's source; keep private browsing/script
artifacts in ignored `tmp/`, and use only explicit local fixtures in CI.

### Next: core Symbols (proposed, not implemented)

Use opaque immutable symbol identity and typed string-or-symbol property keys,
not description strings. The proposed core includes Symbol calls, constructor
rejection, a bounded registry, boxing, branded prototype methods, and own-symbol
reflection. Keep string-only enumeration distinct from symbol keys.
Reference: [Symbol objects](https://tc39.es/ecma262/multipage/fundamental-objects.html#sec-symbol-objects).

Property-key conversion must preserve symbols. Audit implicit string/numeric
conversion, truthiness, equality, and DOM string arguments; a diagnostic display
string must not become an implicit conversion. Explicit Default/String/Number
hints are needed for genuine toPrimitive dispatch. Reference:
[conversion operations](https://tc39.es/ecma262/multipage/abstract-operations.html#sec-type-conversion).

Only expose well-known hooks with implemented algorithms; initial candidates are
toPrimitive and toStringTag, not iterator/regex/species compatibility flags.
Keep all existing caps and charge identity/description/registry/reflection
storage and ingress. Public foreign-symbol ingress needs explicit identity and
admission rules. Independent cases must cover distinct same-description keys,
array descriptions such as length/0, prototype lookup/deletion/reflection,
coercion order/errors, closure/eval/Function lifetime, cumulative fatal limits,
and a symbol-dependent real-worker/native/CDP form. No live script source is an
implementation input, and this proposal does not resolve the later allocation
failure or establish Google acceptance.
