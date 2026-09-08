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
  Error and Math operations exist. Examples include `call`/`apply`/`bind`,
  bounded `Object.create` descriptors, `Object.keys`, array push/pop/slice/join/concat,
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
- Core Symbol primitives have opaque identity, a bounded registry, boxing,
  description/branding methods and typed symbol property keys. Own-symbol
  reflection is separate from string enumeration. Only toPrimitive and
  toStringTag well-known hooks are exposed, with actual algorithms; see below.

Unsupported syntax rejects the complete script with a byte-offset diagnostic,
not a successfully parsed prefix. Current exclusions include strict directives,
non-ASCII identifiers, `let`/`const`, arrows, classes, modules, templates,
`for-of`, `with`, `debugger`, object accessors and block-level function
declarations. There is no garbage collector, Promise implementation, module loader or general event
loop. Several Array methods are exposed but not
implemented. The source is authoritative for individual builtin coverage.
The seven-method callback family and borrowed methods on DOM collection snapshots
are locally accepted under [ARRAY_CALLBACKS.md](ARRAY_CALLBACKS.md), including
independent allocation/ordering tests and real worker/native/CDP form journeys.
Five original core constructor backlinks, genuine String/Number/Boolean prototype
payloads and direct/bound Number/Boolean construction now pass the independent
local contract in [CORE_INTRINSICS.md](CORE_INTRINSICS.md). This does not add
callable Function.prototype or imply Google compatibility.
`Object.create(proto, properties)` supports fresh ordinary data and accessor
properties under [OBJECT_CREATE.md](OBJECT_CREATE.md): typed own-key snapshots,
ordered inherited descriptor reads, flags and original getter/setter receivers.
Definition and descriptor-reflection APIs remain otherwise unimplemented; this
does not add object accessor syntax, Proxy or Host reflection authority.

Known approximations remain: `arguments` is an unmapped snapshot rather than
non-strict parameter aliasing; property descriptors and host coercion are partial;
number formatting/rounding is not fully specification-compatible. Strings retain
UTF-16 code units inside the evaluator, including lone surrogates; the UTF-8 DOM
and display boundary uses replacement characters for unpaired surrogates, and
lone-surrogate string property names are rejected (symbol descriptions retain
their code units). Six exposed Error families have genuine prototype chains,
branding and string conversion, but many ordinary runtime/Host exceptions still
throw diagnostic strings rather than Error objects; modern stack/cause and the
remaining Error constructors are not implemented.
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
scripts do not execute. The native browser retains that restricted realm for
bounded later interaction; the legacy one-shot worker still drops it after its
reply. [PAGE_SESSIONS.md](PAGE_SESSIONS.md) defines the new lifecycle and event
contract, including explicit cumulative interaction budgets.

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
propagation; their third registration argument remains ignored without coercion.
Later native/CDP link and control clicks now deliver a bounded capture/target/bubble
`click`, followed by `submit` when appropriate, to the retained realm. Function-valued
`onclick`/`onsubmit`, boolean capture, removal, propagation stops and cancellation
are supported. Object listener options, synthetic dispatch, content-attribute
handler compilation, other keyboard/mouse/input events, timers, fetch/XHR,
storage and script cookie access remain unsupported. Console methods currently
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
environment and bounded JSON pipes. The retained mode uses strict length-prefixed
messages and keeps only typed input/control edits after initialization, never
replacement scripts or HTML. Before reading page input, the worker closes
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
| Symbol storage | 10,000 cumulative admitted records, including two initial well-knowns; descriptions, registry entries and reflection share the existing realm allocation/fuel limits |
| For-in enumeration | Prototype depth 64; candidate snapshots, including non-enumerable shadows, charge shared realm fuel and cumulative allocation; no separate candidate-count cap |
| URI conversion | 1,048,576 UTF-16 units each input/output, with exact encoded-size preflight; allocations also charge the realm budget |
| Regex compilation | 16,384 pattern units; 8,192 nodes; 64 captures; nesting 64; 128 bracket classes; 2 MiB compiled storage; 2,000,000 compile steps; numeric quantifiers at most 1,000,000 |
| Regex matching | 1,048,576 input units; caller's remaining realm fuel; 16,384 tasks; 4,096 pending states; 4 MiB cumulative state-work accounting per find; lookahead depth 16 |
| DOM bridge | 50,000 nodes; depth 256; 4 MiB cumulative logical allocation; 1,024 snapshot collections |
| Serialized DOM / diagnostics | 2 MiB HTML; at most 64 reported errors |
| Genuine Error host formatting | 4,096 UTF-16 units including prefix/truncation suffix; bounded data-only lookup, no callback execution |
| Worker protocol | 2 MiB request JSON; 4 MiB response JSON per frame; retained event envelopes at most 64 KiB |
| Worker OS / parent deadline | 256 MiB address space; 1 CPU second; 2 seconds total active wall including transfer/startup and later transactions |
| Retained session | 300-second absolute lifetime, 64 transactions including initialization, 32 MiB combined lifetime wire bytes including headers; no renewal/replay |
| Later interaction storage | 32 cumulative listener registrations; 256 event records; 128 edits per event, 8191 UTF-8 bytes per field; same runtime/DOM budgets |

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
The retained manager also bounds pending commands/completions to one and total
owned script children to two per browser, including pending and retiring children.
Idle time does not consume active wall time, but cannot renew the absolute lifetime.
Navigation/shutdown permanently invalidate the relevant generation/pool and reap
owned children. Later failed events cannot replace the last accepted projection or
silently perform their unanswered default action. Toolbar navigation remains usable.
The 32 MiB wire allowance is an explicit new interaction policy, not an increase
to the evaluator heap/fuel or child privileges. New global event-host registration
cost is 794 Runtime-phase bytes; raw language Bootstrap remains unchanged.

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
Fixed grammar operator tags are canonical static Rust strings; unlike identifiers,
property names, literals and regex data, they no longer own individual buffers.
This changes the experimental public AST operator field type to `&'static str`,
not the accepted language or evaluator. [STATIC_OPERATORS.md](STATIC_OPERATORS.md)
records the frozen vocabulary, actual storage measurement and acceptance gates.
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
increment. At that increment it completed one script without errors at 2,481,036 accepted bytes
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
remain. That increment passed all 368 debug tests, 258 selected release checks and
all three exact CI journey steps locally (11 native and 11 external CDP destinations).
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

### Core Symbols and typed property keys

The core uses opaque immutable Arc-backed identity and typed string-or-symbol property keys,
not description strings. Symbol calls produce new primitives; `new Symbol` throws.
The scoped API includes `Symbol.for`/`keyFor`, boxing through Object, branded
prototype `valueOf`/`toString` and the inherited description getter. Descriptions
and registry keys preserve UTF-16, including lone surrogates. Same-description
symbols remain distinct, while registry lookup reuses its registered identity.
Reference: [Symbol objects](https://tc39.es/ecma262/multipage/fundamental-objects.html#sec-symbol-objects).

Property-key conversion preserves symbol identity across get/set/delete, prototype
lookup, `in` and own-property checks. Symbol descriptions such as `0` and `length`
must never enter array-index or length branches. `Object.getOwnPropertySymbols`
returns own symbols in creation order; updating preserves order and deletion then
re-addition moves the key to the end. String-only keys/names/for-in exclude symbols.
No computed object-literal syntax, Reflect namespace or iterator protocol is implied.

Implicit string and numeric conversion must reject Symbols rather than use their
diagnostic display or silently produce NaN. Plain `String(symbol)` is the explicit
exception; boxed symbols and `new String(symbol)` do not get that exception.
Genuine `Symbol.toPrimitive` dispatch supplies default/string/number hints, checks
callability and primitive results, preserves the receiver and shares existing
fuel/recursion limits. `Symbol.toStringTag` affects Object's tag string only when
the retrieved value is a string. These two well-known hooks are the initial scope;
iterator, regex, species, concat and custom-instance hooks stay absent until their
algorithms exist. References: [conversion operations](https://tc39.es/ecma262/multipage/abstract-operations.html#sec-type-conversion)
and [String construction](https://tc39.es/ecma262/multipage/text-processing.html#sec-string-constructor-string-value).

One Runtime is the current worker's single agent/realm. Its registry and symbol
identities survive that document's scripts, closures, eval and Function calls;
no persistent page realm is added. Public opaque handles may outlive their source
Runtime and cross into another, retaining identity but requiring destination
storage admission. The immutable handle preserves public Value's existing
Send/Sync boundary without making the AST/runtime shareable or adding worker
capabilities. Foreign registered symbols do not automatically join the
destination registry. Semantic well-known identity and actual retained record
admission are separate concerns; equal well-known keys must not hide uncharged
foreign storage: admission compares actual allocation pointers while well-known
equality compares their supported kind.

The new symbol table is cumulative and capped at 10,000 admitted records, including
the two initial well-knowns. Its logical allowance is 128 bytes for record/control/
table storage plus description capacity in UTF-16 bytes, charged before retention;
registry indexing adds 64 bytes. Handle copies do not copy description payloads.
Reflection arrays pay existing slot/object charges, and actual description/string
copies remain charged. Existing 4 MiB, fuel, object, parser, array and worker caps
are unchanged; first resource failure remains fatal and latched. These are logical
storage allowances, not RSS or a new garbage collector.

The DOM bridge declares its string-valued setters and argument positions. Runtime
performs fallible ToString, including boxed/custom-object hooks, before passing
those values to Host. Symbol conversion failure must preserve the target mutation
and prevent navigation; earlier ordinary hook side effects are not rolled back.
Node arguments, callback arguments and unused extras are not stringified.
Symbol-keyed Host properties remain explicitly unsupported rather than aliasing
string names. The bridge still uses UTF-8 replacement for lone surrogates and its
existing textual collection.item index approximation; receiver validation versus
argument-conversion ordering is not full Web IDL behavior. Reference:
[DOMString conversion](https://webidl.spec.whatwg.org/#es-DOMString).

The authored `/script-symbols` fixture is frozen byte-for-byte from its missing-API
baseline: it previously reported ordinary ReferenceError before creating controls,
without allocation rejection. At the Symbol increment it completed at 57,954
accepted bytes with a real query/hidden/submit form. Native and external CDP paths
submitted its Unicode query, followed the local result and reached the destination;
frames were inspected. That increment's separate negative worker verified its
builder and first Symbol succeeded, then rejected Runtime 159,984 after 4,193,241
accepted bytes, with readable fallback
and no catch/finally/later/navigation effects.

That increment passed all 438 debug tests and 340 selected release checks on default stacks,
including 24 independent Symbol semantics, 14 key and 13 admission/lifetime/limit
groups, 20 DOM and 26 actual-worker groups. Private cases verify storage allowances,
Send/Sync, foreign well-known record admission, reserved description capacity,
real getter copies and preflighted hook argument slots. The full three CI journey
steps passed locally with 12 native and 12 external CDP destinations. No existing
test was weakened; review caught and corrected readonly, coercion-order and
public transport regressions plus new-copy charge omissions. Native string-key
deletion retains its existing character-level fuel metering.

One post-Symbol live checkpoint still produces no Google result links. Homepage
HTTP 200 retains its form with three completed scripts/seven errors and no rejected
allocation (2,506,006 accepted bytes). Search HTTP 200 completes two scripts with
three errors: first `TypeError: prototype must be an object or null`, then an AST
request of 387,964 after 4,107,538 accepted bytes, repeated by the next script.
The missing-Symbol error is absent, but the inspected frame remains blank; exit 2,
no result or destination. The diagnostic alone does not identify the supplied
prototype or prove the cause. Next work independently audits object/prototype
coverage and cumulative storage, without site-source adaptation or raised limits.
No live script source was an implementation input. Publication evidence is in
the daily log; local Symbol support does not establish Google acceptance.

### Typed prototype identities

Ordinary objects, user functions and native functions now remain distinct object
identities when used as prototypes. The private Copy identity stores an ordinary
object index, user-function index or stable native-identity index; it never exposes
a function's ordinary property bag as that function. `Object.create(fn)` and
`Object.getPrototypeOf` round-trip the actual function while the child remains
noncallable. Function/native-valued constructor `prototype` properties participate
in `new` and `instanceof` without falling back to `Object.prototype`. Inherited
getters and coercion hooks retain the original receiver.

Shared iterative owner/descriptor helpers cover string and Symbol reads, readonly
writes, `in`, enumeration and constructor traversal. Native enumeration owners
carry their actual identity rather than a root-only sentinel, so inherited native
virtual metadata and stored fields agree with reads. The existing snapshot/shadow
policy is unchanged. Virtual readonly String/RegExp/function/native metadata takes
precedence over stored writable fields; deleting or shadowing a child's ordinary
property does not mutate its prototype.

First traversal/prototype use of a native identity retains one UTF-8 name and an
ordinary property bag, precharging the existing 64-byte record allowance plus
name length and 128-byte object allowance before retention. Stable repeated use
does not re-admit that record. Native-name matching spends shared fuel; returning
`Value::Native` from `getPrototypeOf` charges its real name copy. Generated native
method names and UTF-16 function-name results are also preflighted before allocation.
Unknown public native names may retain identity without gaining callable or Host
authority. Native records consume the existing object cap; allocation/fuel failures
remain fatal and latched across handlers and later calls/scripts.

At the typed-prototype increment, measured x86_64 layouts were Object 104 bytes, Property
64 bytes, typed identity and optional identity 16 bytes each, native table record
32 bytes, and public Value 32 bytes. The optional identity is the same size as the
prior optional object index. Existing 128-byte object/property and 64-byte native
record allowances still cover these layouts; this is logical accounting, not RSS.
No allocation, fuel, object, parser, worker or other existing cap was raised.

Exact legacy traversal edges are preserved, including their existing differences:

| Operation | Existing bounded traversal policy |
| --- | --- |
| Ordinary/native string read | Own fields, then up to 64 ancestors; a hit at the final ancestor succeeds, but an unresolved final walk is fatal even if it just reached null |
| User-function/primitive-wrapper string read; Symbol read; `in` | Up to 64 owners including the root; a final-owner hit succeeds, but unresolved exhaustion is fatal |
| String readonly-write guard; for-in enumeration | Up to 64 owners including the root; terminal null after the final owner is accepted |
| Symbol readonly-write guard | Up to 64 owners including the root; unresolved exhaustion remains fatal, including terminal null |
| `instanceof` | Up to 64 ancestors of the left object, excluding itself; unresolved exhaustion remains fatal |
| `getPrototypeOf`; own-only reflection | One parent, or own fields only; no ancestor-chain walk |

Primitive string out-of-range indexed reads also retain their existing immediate
undefined behavior. These boundary distinctions are regression-tested rather than
silently normalized by increasing a shared limit.

The ES5-style `Object.getPrototypeOf` policy still rejects primitives/null/undefined
without boxing or coercion, now explicitly with TypeError. `Object.create` checks
prototype type before rejecting unsupported descriptors. A primitive left side of
`instanceof` returns false before accessing the constructor's prototype. Host
prototypes, mutable prototypes, proxies, full descriptors and custom
`Symbol.hasInstance` remain unsupported; `Function.prototype` is still the existing
noncallable object. Existing coercion/Host exceptions may be strings rather than
Error objects, so their readable TypeError text is not an `error.name` guarantee.
No parser, AST, dependency, TLS, worker authority or CDP behavior was expanded.
References: ES5.1 [Object.create](https://262.ecma-international.org/5.1/#sec-15.2.3.5),
[Object.getPrototypeOf](https://262.ecma-international.org/5.1/#sec-15.2.3.2),
[function construction](https://262.ecma-international.org/5.1/#sec-13.2.2), and
[[HasInstance]](https://262.ecma-international.org/5.1/#sec-15.3.5.3).

That increment passed all 492 debug tests and 394 selected release checks on default stacks,
including 24 independent prototype semantics groups, 17 prototype resource/boundary
groups and eight new private layout/admission/copy regressions, plus 22 DOM and
29 actual-worker groups. Formatting and locked binary/example builds pass. No old
test assertion was weakened. One new DOM test was corrected to the existing
string exception representation, retaining all hook-effect/content/navigation
assertions; that did not expand Error-object fidelity.

The frozen 2,382-byte authored fixture is now `tests/fixtures/script/prototypes.html`,
served at `/script-prototypes`. Before implementation it stopped at
`Object.create(User)`: ordinary TypeError, preserved fallback, zero forms/inputs/
navigation, no heap rejection and 69,851 accepted bytes. Its unchanged SHA-256 is
`b917aa23d76bda209589c70d4f7a3f92fd050410b0194d6076c9348aa9f0ca2d`. Actual-worker
acceptance at that increment completed one script without errors at 78,118 accepted bytes
(Bootstrap 19,773, Source 2,348, Ast 42,130, FunctionCode 390, Runtime 13,477),
creating the real controls. Separate cases cover depth failures and ordinary
TypeError recovery. All three exact CI journey steps pass locally with 13 native
and 13 external CDP destination successes. The new form submits its actual Unicode
query and hidden `source=fixture`, follows the local result and reaches HTTP 200
at the destination; the native query and CDP destination frames were inspected.
This is generic correctness work, not evidence of Google's supplied prototype
argument or a promise to complete its live journey.

Exactly one post-prototype live checkpoint submitted the actual Google form over
verified TLS. Homepage HTTP 200 retained 26 items/one form, three completed scripts/
seven errors and no allocation rejection (2,506,470 accepted bytes). Search HTTP
200 still had zero items/forms, two completed scripts/three errors: the same first
`TypeError: prototype must be an object or null`, then Ast 387,500 rejected after
4,107,727 accepted bytes and repeated by the next script. The inspected search
frame remained blank; exit 2, no result or destination. This change has no observed
benefit on that leading live diagnostic, whose supplied argument and cause remain
unknown. Changing responses are not controlled benchmarks. Further builtin/
prototype correctness and storage proposals require independent authored cases;
no live-source inspection or additional retry was used. Implementation a3295d5
passed exact-SHA remote CI with all 492 debug/394 selected release tests and
13 native/13 external CDP destinations. Pages deployed matching HTTPS HTML with
an approved certificate and HTTPS enforcement; publication evidence is in the
dated log. The actual Google goal remains incomplete.

### Error-family prototypes and string conversion

The six already-exposed constructors—Error, TypeError, RangeError, ReferenceError,
SyntaxError and URIError—have distinct intrinsic prototype objects. Error.prototype
inherits Object.prototype; the other five inherit Error.prototype. Constructors
retain Function.prototype as their parent under the ES5 policy, and their own
prototype property is nonenumerable, nonwritable and nonconfigurable. Each family
prototype has its own writable/configurable, nonenumerable name, empty message
and constructor backreference. Only Error.prototype owns the shared toString.
References: [ES5 Error objects](https://262.ecma-international.org/5.1/#sec-15.11)
and [native Error structure](https://262.ecma-international.org/5.1/#sec-15.11.7).

Call and new create fresh correctly linked instances without an own name. An
undefined or omitted argument leaves message inherited; other arguments undergo
fallible ToString and create a nonenumerable own message. The existing first-
argument clone and generic property-storage charges remain. UTF-16 messages retain
lone surrogates. The family brand belongs to genuine instances and, per ES5, the
intrinsic prototypes themselves; Object.create(Error.prototype) inherits behavior
without acquiring that brand. Object.prototype.toString respects the existing
Symbol.toStringTag hook before its Error fallback. No new globals, modern
constructor-parent semantics, stack/cause support or general subclassing is implied.

The own ErrorKind brand increases x86_64 Object from 104 to 112 bytes, within
the unchanged 128-byte allowance. Property remains 64 bytes; ErrorKind and its
Option are one byte each. Bootstrap explicitly charges all six prototypes,
their stored fields and native constructor/method identities: 25,854 bytes total.
Instances reuse these prototypes and pay existing per-object/property costs.
No dependency, TLS, worker capability, CDP command or existing cap changed.

Generic Error.prototype.toString accepts object/function/native and existing Host
receivers, rejects primitives, and performs Get(name), its string conversion,
Get(message), then its string conversion with the original receiver. Undefined
uses the standard defaults. Empty name/message branches return the already-paid
other buffer; joining nonempty values preflights the actual UTF-16 output before
allocation. Getters, conversion hooks and Host reads remain fallible live operations
with existing fuel, depth, ingress and real-copy accounting. Reference:
[Error.prototype.toString](https://262.ecma-international.org/5.1/#sec-15.11.4.4).

Already-object parser, dynamic-source, URI and regex errors now use the intrinsic
family links independently of overwritten global constructors. This does not
convert free exception() diagnostic strings into objects. Host reporting of an
uncaught genuine Error is deliberately separate from JavaScript ToString: it
borrows data fields through at most 64 typed owners, never invokes getters,
coercion, toString or Host callbacks, and never adds realm charges after failure.
Unavailable names use the intrinsic family; inaccessible/accessor/nonprimitive
message fields use explicit placeholders. The formatted output is capped at
4,096 UTF-16 units including prefix/truncation suffix, using at most 12 KiB of
bounded host buffer. This is not a script string limit or a larger realm budget.
Non-Error diagnostic behavior and first fatal failure remain unchanged.

The frozen 2,216-byte tests/fixtures/script/errors.html is served at /script-errors.
Its old actual-worker baseline stopped at Object.create(Error.prototype), before
creating controls, at 63,160 accepted bytes and without heap rejection. The unchanged
fixture now passes the focused worker check at 76,751 bytes (Bootstrap 25,854,
Source 2,162, Ast 37,660, Runtime 11,075; FunctionCode and regex zero). This is
different completed work, not an equal-work memory comparison. Focused checks
pass all 24 DOM and 33 worker tests, including all six family DOM conversions,
Symbol rejection before mutation, callback-free reporting/later-script recovery,
and fatal fuel plus real joined-output storage rejection. Independent coverage
passes 27 semantic, 15 resource and seven private groups; the same final semantic
suite passed five and failed 22 against the pinned pre-change library. All 547
debug tests and 449 selected release checks pass on default stacks, together with
formatting, build and dependency checks. Three exact CI journey steps pass locally
with 14 native and 14 external CDP destinations. Root inspected the new actual
form and destination frames. Limits and existing assertions are unchanged.

One bounded post-Error Google checkpoint still submits the actual form with
verified TLS and ordinary cookies. Homepage HTTP 200 has 26 items/one form,
three completed scripts/seven errors and no allocation rejection (2,512,389 bytes).
Search HTTP 200 remains blank, with two completed scripts/three errors. First is
now unsupported Array.concat; Ast 387,620 is later rejected after 4,132,042 accepted
bytes (Bootstrap 25,854, Source 159,091, Ast 2,438,297, FunctionCode 15,232,
Runtime 1,493,568, regex zero). Exit 2, no actual result or destination; blank frame
inspected. This response no longer reports the prototype error, but does not
establish its original argument or a controlled benchmark. Next is generic bounded
concat support using independent authored cases and separate storage diagnosis.
Exact-SHA remote acceptance/publication is recorded in the daily log when verified.

### Bounded Array.concat and array identity

Array.prototype.concat now accepts a generic receiver, boxes primitives and
rejects null/undefined. The receiver comes first, followed by arguments in order.
Only genuine arrays spread, exactly one level. Other values—including functions,
Errors, boxed values, arguments snapshots and opaque Host handles—are single
elements. There is no element coercion, array-like length read, Host callback,
constructor lookup, species or isConcatSpreadable protocol. The result is a fresh
intrinsic Array independently of replaced globals or source constructor fields.

Each array's intrinsic length is captured when that operand is reached, after
earlier getters may have changed it. Ascending HasProperty then Get operations
include inherited numeric properties with the original receiver; missing indices
remain holes, explicit undefined stays present, and trailing holes contribute to
the full result length. Nested objects/arrays and Symbols retain identity; string
copies preserve UTF-16. New result slots are own properties, not inherited writes,
and do not change source slots. References: [ES5 concat](https://262.ecma-international.org/5.1/#sec-15.4.4.4)
and [current concat](https://tc39.es/ecma262/multipage/indexed-collections.html#sec-array.prototype.concat).
The explicit final length preserves trailing holes rather than reproducing the
printed ES5 algorithm's omission; modern spreadability/species hooks remain absent.

Array.prototype is now itself an empty genuine array. Arguments snapshots retain
their prepaid indexed storage and independent parameter copies, but have a distinct
own Arguments brand and Object.prototype parent. Array.isArray returns false for
them, concat appends them whole, and Object.prototype.toString uses Arguments
unless the existing toStringTag hook overrides it. This is not full arguments
semantics: unmapped parameters, array-style index/length coupling and partial
descriptors remain explicit approximations. Borrowing existing array methods on
indexed snapshots retains its current behavior; unrelated generic methods are
not expanded by concat.

The unpublished empty result pays its existing 128-byte metadata/object-cap cost
before source getters. A private builder prepays bounded geometric capacity at
64 bytes per slot before each HasProperty/Get, including holes and spare credits.
Each operand's known length must fit the remaining 10,000-element allowance before
visiting its slots. Real element-read payload copies stay charged; already-owned
nonarray payloads and the completed builder move without redundant copies. Index
names use a bounded five-byte stack buffer, not an uncharged heap string. Shared
fuel and existing prototype-depth rules apply; failures remain fatal/latched and
return no partial array. An unpublished empty object remains cumulatively charged
on failure. Other construction, mutation, ingress and every cap stay unchanged.

The frozen 1,852-byte tests/fixtures/script/concat.html is served at /script-concat.
Its actual-worker baseline stopped at the first dense concat call at 69,290
accepted bytes, no heap rejection and no controls. The unchanged page now completes
one script/no errors and creates real controls at 75,704 bytes: Bootstrap 25,854,
Source 1,798, Ast 38,070 and Runtime 9,982, FunctionCode/regex zero. These are
different completed workloads, not a memory-performance comparison. Focused
checks pass 26 semantic, 26 DOM and 37 actual-worker groups. Worker negatives
preserve fallback/latching at the 10,001st result slot and a real 159,984-byte
element-copy rejection; ordinary null-receiver errors allow a later script.
All 26 independent semantic, 15 resource and 10 private groups pass. Private
checks cover inherited getter mutation/order, recursive re-entry on default
stacks, admission before callbacks and owned moves versus actual paid copies.
Object remains 112 bytes, Property 64 bytes and bootstrap 25,854 bytes. The frozen
full suite passes 604 debug tests and 506 selected release checks with unchanged
limits. The three exact CI journey steps pass locally with 15 native and 15
external CDP destinations, including real Unicode query/hidden/submit controls
from the concat fixture. Root inspected the query and destination frames.

One bounded post-concat Google checkpoint still renders no result links.
Homepage HTTP 200 has 26 items/one form, three completed scripts/seven errors and
no allocation rejection (2,511,733 accepted bytes). Actual form submission works
with verified TLS and ordinary cookies. Search HTTP 200/title Google Search has
zero items/forms and two completed scripts/three errors repeating a FunctionCode
rejection: 4,194,294 accepted, 128 requested, 4,194,304 limit. Phases are Bootstrap
25,854, Source 132,384, Ast 2,438,297, FunctionCode 21,760 and Runtime 1,575,999,
regex zero. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination.
Unsupported concat is absent in this response, not a controlled performance
comparison or a guarantee that one more change will finish compatibility. Next
is independent cumulative-storage ownership diagnosis; the last rejected phase
is not automatically the best optimization target. No live-source inspection,
adaptation or retry. Exact publication evidence follows in the daily log.

### Empty arguments snapshot storage

The implemented storage change defers only an arguments snapshot whose actual
argument list is empty. A real nondeletable local binding is installed after
formal parameters and before existing hoisting, retaining the original callee
identity privately. Any binding read, including typeof, void, member access and
direct eval, materializes the existing branded empty object and callee property
once. Successful replacement cancels the pending value without reading it;
metadata-only lookup, bare declarations and unsuccessful deletion preserve it.
Nonempty lists, including a single explicit undefined, retain their current
prepaid slots, parameter copies and snapshot construction. A formal named
arguments retains the existing suppression behavior. Scope, identity, callee,
prototype and the documented unmapped/indexed-length approximations do not change.
References: [declaration binding](https://262.ecma-international.org/5.1/#sec-10.5)
and [arguments objects](https://262.ecma-international.org/5.1/#sec-10.6).

The environment and real binding still pay their existing 128 and 137 bytes.
The 128-byte object and 134-byte callee property are admitted only when actually
created. The existing callee-property fuel step also moves to that point; there
is no new artificial step or cap. Materialization publishes only after complete
success. Failed admission remains fatal/latched, retains earlier effects and
already accepted orphan storage, and skips handlers/later work. This explicitly
defers resource-failure timing, not JavaScript value behavior. Unread/replaced
snapshots do not consume nonexistent object/callee allocations or object counts.
The private pending callee id increases Binding from 64 to 80 bytes; Environment
remains 48 bytes and Object 112, all within their existing 128-byte allowances.
Bootstrap remains 25,854 bytes. No public Value variant, retained nonempty Vec,
collector, limit increase, worker permission, dependency or CDP expansion is
included.

Before implementation, the frozen 1,576-byte local worker fixture attempted
8,500 empty calls, then checks actual snapshot/eval identity and builds all form
controls. It rejected Runtime 137 after 4,194,254 accepted bytes, retaining fallback
and no controls. The unchanged tests/fixtures/script/empty-arguments.html now
completes one script without errors and creates the actual query/hidden/submit
controls at 2,317,555 accepted bytes: Bootstrap 25,854, Source 1,653, Ast 26,790,
FunctionCode 407 and Runtime 2,262,851, regex zero. Baseline and completion execute
different amounts of work; these totals are not an equal-work performance or RSS
comparison.

The authored public-invoke control now measures 265 Runtime bytes per unread
empty call, compared with the frozen baseline's 527. First read pays the remaining
262 exactly once; repeated reads preserve object identity without another object
or callee charge. Nonempty costs remain unchanged. A separate actual-worker
control that reads every snapshot still exhausts the unchanged 4 MiB budget:
Runtime 137 is rejected after 4,194,207 accepted bytes. Its earlier snapshot-ready
DOM marker survives, but catch/finally/later-script effects and navigation do not.
Focused private/resource cases separately verify first-read object 128 and callee
134 admission failures without publishing a partial binding value.

All 29 independent semantic groups pass against both the pinned old library and
the implementation, alongside 14 independent resource and seven private groups.
They cover read/replace/delete/hoisting and eval scope, brand/callee/identity,
retained environments, exact charging and fatal preflight. The focused integration
run passes 28 DOM and 40 actual-worker groups, including ordinary-error recovery.
Frozen default-stack gates pass 659 debug tests (29 summaries) and 561 selected
release checks (20 summaries); no existing assertion or cap was weakened.
Evidence is in ignored tmp/empty-arguments-final-tests.log,
tmp/empty-arguments-final-release.log and tmp/empty-arguments-integration-tests.log.
All three exact CI journey steps pass locally under unchanged deadlines, with
16 native and 16 external CDP destinations. The new form submits its actual
Unicode query/hidden/submit fields; stale-node/session and 1100×683 destination
PNG checks pass. Root inspected query/destination frames and checked port cleanup.
Exact-SHA publication evidence follows separately in the daily log.

One bounded post-change Google attempt still fails. Homepage HTTP 200 retains
26 items/one form, three completed scripts/seven errors and no allocation rejection
(2,505,994 accepted bytes). Actual served-form submission works with verified TLS
and ordinary cookies. Search HTTP 200/title Google Search has zero items/forms
and two completed scripts/three errors repeating Runtime 128 rejected after
4,194,254 accepted bytes against 4,194,304. Accepted phases are Bootstrap 25,854,
Source 132,384, Ast 2,438,297, FunctionCode 22,016 and Runtime 1,575,703, regex zero.
Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination or newly
completed live stage. These local results do not establish Google's actual call
distribution; changing served responses are not a controlled benchmark. Further
ownership work needs independent authored measurements, with no raised limits or
live-source inspection/adaptation/retry.

### Default user-function prototype storage

The implemented bounded storage change defers only the fresh default prototype
object and its constructor backlink for original user functions. Function code/name,
captured environments, the actual 128-byte function property bag and its real
137-byte nonenumerable writable prototype property remain admitted at creation.
A private pending flag distinguishes the unpublished default; no public Value
variant or generic property lazy-value mechanism is introduced. Native builtins,
Function.prototype itself, existing descriptors/deletion approximations, shared
code and the empty-arguments policy remain unchanged.

The first actual prototype-value read creates a unique Object.prototype-linked
object with an own nonenumerable constructor referencing the original function.
Repeated/inherited reads resolve the same owning function's object, not a new
object for each receiver. Names/keys, hasOwnProperty, in, unsuccessful deletion,
and Object.getPrototypeOf(function) inspect existing metadata without forcing the
default value. Successful own replacement cancels pending only after the normal
paid write; readonly/no-op/failed writes and inherited child shadows do not.
Previously observed defaults and their constructor identities survive replacement.
Construction and instanceof use their existing actual Get and typed-prototype
rules, including primitive-left instanceof short-circuit and constructor fallback.
References: [function creation](https://262.ecma-international.org/5.1/#sec-13.2),
[prototype property](https://262.ecma-international.org/5.1/#sec-15.3.5.2),
and [instance checks](https://262.ecma-international.org/5.1/#sec-15.3.5.3).

Materialization admits the existing 128-byte object and 139-byte constructor
property before publishing any value; the existing constructor-write fuel step
moves to this point. The original prototype-property write and its fuel stay at
function creation. No synthetic step or cap replaces unperformed work. Failure
timing intentionally moves to actual admission: prior effects and accepted orphan
storage remain, pending state is not exposed as a partial value, and fatal
latching bypasses handlers and later work. Object/function/heap/fuel/depth limits,
payload ingress and real copies are unchanged. If construction succeeds but the
subsequent ordinary read exhausts fuel, the complete object stays published;
failure does not undo accepted work. No getter or inherited readonly constructor
can intercept internal own-backlink creation. Public deletion of a function's
prototype property still returns false, retaining the existing descriptor
approximation rather than adding a general descriptor API.

The private pending flag increases Function from 24 to 32 bytes on x86_64. Code
remains 56 bytes, Object 112 and Property 64; the fixed metadata still fits its
existing allowances. Bootstrap remains 25,854 bytes. A fresh anonymous function
now pays 265 Runtime bytes for its real bag/property, plus the unchanged 128
FunctionCode bytes (and any display name). The first pending-default value read
adds 267 Runtime bytes exactly once. The authored otherwise-empty zero-argument
factory pays 530 Runtime bytes including its separate 265-byte unread-call
overhead; that is not a cost assertion for arbitrary factory bodies. This is not
a collector, a resource refund, or permission to share
default prototypes across different functions.

The frozen authored six-case baseline isolates 532 Runtime bytes per fresh
function beyond empty-call overhead; default object/backlink storage accounts
for 267. Same-size creation costs for small and 1,000-statement bodies show that
code is already shared. The frozen 1,982-byte authored worker fixture attempts
4,800 fresh function creations before metadata, default/backlink identity,
inherited-owner, new/instanceof and actual form-building checks. The old worker
rejects Runtime 137 after 4,194,256 accepted bytes, with no controls and readable
fallback. This is an independently authored workload, not website source.

All 30 independent semantic groups pass on the pinned baseline and candidate;
17 candidate resource groups verify creation, first-read, overwrite, copy and
fatal admission costs under unchanged caps. Three initial new-test assumptions
about Object.prototype.constructor and Array.prototype.constructor were corrected
before candidate testing: those existing native backlinks are absent. User-function
backlinks/attributes remain required, and native controls check unchanged behavior.
This does not add native backlinks or claim full descriptor conformance. All ten
new private groups and all 91 runtime groups pass on default stacks, including
shared code versus fresh identity, metadata/computed keys, successful and failed
overwrites, inherited owner/shadowing, poisoned constructor fields, retained old
defaults/data cycles, exact fuel movement and object/function/heap preflight.
Failed 128-byte object or 139-byte backlink admission leaves pending state intact;
accepted orphan objects remain charged, with no partial value exposed.

The unchanged tests/fixtures/script/function-prototypes.html now completes one
script without errors and creates the real query, hidden source=fixture and
submit controls at 3,232,783 accepted bytes: Bootstrap 25,854, Source 1,916,
Ast 36,118, FunctionCode 614,535 and Runtime 2,554,360, regex zero. The old baseline
stopped before those controls; these totals represent different completed work,
not a controlled performance or RSS comparison. A separate worker control that
reads each generated default still exhausts the same 4 MiB budget: Runtime 128
is rejected after 4,194,238 accepted bytes. Its earlier ready marker survives,
but catch/finally/later-script changes and navigation remain bypassed. Ordinary
post-materialization errors still permit recovery.

Focused integration passes 30 DOM and 43 actual-worker groups. Frozen local gates
pass 721 debug tests (31 summaries) and 623 selected release checks (22 summaries),
alongside formatting, the locked binary/example build and dependency guard on
Rust 1.91.1. No previous assertion or cap was weakened. Evidence is in ignored
tmp/function-prototype-private-focused.log, tmp/function-prototype-integration-tests.log,
tmp/function-prototype-final-tests.log and tmp/function-prototype-final-release.log.
All three exact CI journey steps also pass locally under unchanged deadlines,
with 17 native and 17 external CDP destinations. The new fixture submits its
actual Unicode query, hidden and submit fields; stale-node rejection, first local
link, flattened session and 1100×683 destination PNG checks pass. Root inspected
the native query/ready-form and external CDP destination frames. Owned servers
exited and fixture/debugger ports were clear. These are application-handler and
external protocol checks, not independent physical input. Exact-SHA publication
is recorded separately in the daily log. These authored results do not establish
Google's actual prototype-read distribution or complete its results/destination goal.

One bounded post-change live attempt submitted Google's actual form through verified
TLS and ordinary cookies. Homepage HTTP 200 retains 26 items/one form, three completed
scripts/seven errors and no rejected allocation (2,428,284 accepted bytes). Search
HTTP 200 still has no items/forms and two completed scripts/three errors: first
`TypeError: value is not callable`, then Source 26,999 rejected after 4,187,629
accepted bytes, repeated by the next script. Search phases are Bootstrap25,854,
Source132,384, Ast2,438,297, FunctionCode25,216, Runtime1,564,706,
RegexCompile1,152 and RegexResult20. Exit 2/JOURNEY_INCOMPLETE; the inspected search
frame is blank, with no result/destination or newly completed live stage. The
generic error does not identify a missing callable; changed served responses are
not a controlled performance comparison. Next is independently authored language/
builtin coverage and measured storage ownership, with no raised limits or live
source inspection/adaptation/retry.

### Bound functions

The runtime implements original, bounded Function.prototype.bind using a private
ordinary/bound function kind behind existing Value::Function and typed function
identities.
No wrapper source, synthetic AST, replacement runtime or new public Value kind.
Binding validates the callable target before binding-specific reads, retains the
unconverted receiver and ordered prefix arguments, and returns a fresh callable.
Its parent is the intrinsic Function.prototype; that prototype's existing
noncallability remains outside this increment. Bound eval calls are indirect.

Calls prepend inner bindings before outer bindings and final call arguments;
rebinding cannot replace the original bound receiver. The eventual ordinary
target alone creates its lexical environment and arguments snapshot, preserving
its callee identity and existing non-strict boxing. Construction delegates to the
supported target constructor and ignores bound this and any bound.prototype.
Instance checks delegate to the target's current prototype/instance policy,
including target-prototype replacement after binding and primitive-left handling.
Binding grants no new native or Host constructor/call authority. Existing native
constructor exclusions and native instance-check approximations remain explicit.

The initial bound function has own readonly, nonenumerable, nonconfigurable length
and restricted caller/arguments metadata, but no own prototype or name. This is
the ES5-shaped policy, not modern bound-name generation. Length is captured from
the target's existing numeric length, reduced by prefix count and floored at zero;
existing native arity approximations are unchanged. Reading or writing restricted
caller/arguments throws TypeError, including inherited access; metadata-only
presence/enumeration does not invoke the restriction. Deletion returns false.
An explicitly assigned ordinary name/prototype remains writable, enumerable and
deletable, without affecting call/construction/instance behavior. No general
descriptor/accessor API is added. Ordinary function metadata and lazy defaults
retain their existing semantics and charges.

Resource contract: keep every current realm, worker, parser, call/entry and
argument cap. Bound records share the existing function arena/count cap and pay
for their actual fixed metadata, property bag and retained argument slots before
publication. Owned incoming payloads may move only after slot admission; retained
strings/native names copied for subsequent calls are charged, while object/Symbol
identity is preserved. Do not retain uncharged spare capacity from public vectors.
Measured Function 40/Code 56 plus control overhead fit the ordinary 128-byte allowance;
Function 40/BoundData 88 plus control overhead fit the separate bound 160-byte allowance.
The retained property bag pays 128 Runtime bytes and each exact prefix slot pays 64.
Adding the real bind builtin increases Bootstrap by 145 bytes to 25,999 (property 128,
key 4 and native name 13), without changing a limit or dropping existing charges.

Nested bound forwarding uses bounded, fuel-metered traversal and a single ordered
output list, without an unguarded recursive chain or repeated prefix concatenation.
Conceptual bound calls/construction forwarding count against the existing 64-call
and combined-entry limits, including reentry into user/Host code. Discovery checks
wrapper/entry and aggregate argument bounds while walking the chain. For construction,
the terminal target's constructor eligibility is checked after discovery and before
output-slot admission, retained-payload copies, constructor prototype Get, or target
body/Host effects. A discovery limit can therefore precede an error for a
nonconstructible native target; this is the adopted bounded resource policy, not a
claim about unbounded ES5 error ordering. Output slots are prepaid before copying
only retained values actually forwarded. Instance delegation has a fuel-metered 64-bound-target
traversal, separate from the existing prototype walk. Fatal failures retain earlier
effects and admitted storage, publish no partial bound value, latch across later
scripts/ingress, and unwind counters.

Reference: [ES5.1 bind, call, construction and instance delegation](https://262.ecma-international.org/5.1/#sec-15.3.4.5).
All 36 independent semantic, 16 independent resource and ten new private groups
pass on default stacks. Tests cover admission/publication, retained buffer identity,
copy slopes and failure ordering, conceptual entry cleanup, aggregate arguments,
separate instance traversal, repeated fuel exhaustion and an owned mixed-recursion
child. The three old Bootstrap pins changed only by the measured 145-byte builtin
addition; no other old assertion or limit changed. Full validation passes 788 debug
tests/33 summaries and 690 selected release tests/24 summaries, including 32 DOM and
46 actual-worker groups, formatting, locked builds and the native-backend guard.

The authored old-worker fixture was frozen before implementation and failed at
absent bind with no form. The unchanged candidate fixture now completes one script
and creates its actual controls at 104,479 accepted bytes, without errors or rejected
allocation: Bootstrap 25,999 + Source 2,556 + Ast 49,886 + FunctionCode 1,652 + Runtime 24,386.
It executes more work than the 84,032-byte negative baseline, so those totals are not
a performance comparison. Restricted-property errors allow later-script recovery;
fatal depth exhaustion preserves earlier DOM effects without target/handler/later
effects or navigation. All three exact CI journey steps also pass locally with 18
native and 18 external CDP destinations. The new fixture submits its real Unicode,
hidden and submit fields, rejects stale nodes, clicks the first local result and
verifies a 1100×683 destination PNG and flattened session. Native query and CDP
destination frames were inspected; owned fixture processes exited and ports 7878
and 9222 were clear. These are application-handler and external protocol checks,
not physical keyboard input or Google compatibility.
Bind was a source-confirmed missing feature, not a diagnosis of the generic live
non-callable error; no website script source is an implementation input.

Exactly one bounded post-bind Google checkpoint followed. Homepage HTTP 200/title
Google retained 26 items/one form, three completed scripts/seven errors and no
allocation rejection: 2,429,821 accepted bytes = Bootstrap 25,999 + Source 63,524 +
Ast 1,861,598 + FunctionCode 39,063 + Runtime 436,733 + RegexCompile 2,816 + RegexResult 88.
Actual served-form submission worked. Search HTTP 200/title Google Search remained
blank with zero items/forms and two completed scripts/three errors. First was
`TypeError: property access on null or undefined`; the next two repeated a Source
request of 26,794 rejected after 4,191,219 accepted bytes against the unchanged
4,194,304 cap. Search phases were Bootstrap 25,999 + Source 132,384 + Ast 2,438,297 +
FunctionCode 25,376 + Runtime 1,567,991 + RegexCompile 1,152 + RegexResult 20.

Exit 2/JOURNEY_INCOMPLETE; the blank 03-search.png was inspected. No actual result,
destination or new live journey stage completed. No live-source inspection,
adaptation or retry occurred; changing responses are not a controlled comparison
or proof that bind caused the prior error. Implementation 0c48eeb also passed
exact-SHA Rust CI 34176183048 with the same 788/690 test totals and 18/18 journeys.
Pages 34176182971 deployed matching full HTTPS HTML with the approved apex
certificate, HTTPS enforcement and HTTP redirect. Publication does not complete
the Google acceptance gate; the daily log preserves the evidence.

### Nullish member diagnostics

This implemented increment improves evidence for the live nullish-property failure rather
than assuming an unrelated missing builtin caused it. Attach compact private
context to the actual catchable fault created when member-reference resolution
rejects a null/undefined base. Keep the original thrown Value and exact catch-visible
string unchanged. No new browser capability, AST/source capture, public Value kind,
worker schema, page-visible property or global last-error state is introduced.

Context records the originating reference operation (read, assignment target,
compound-assignment target, update target, delete target, call target or for-in
target), distinguishes null from undefined, and categorizes the already-evaluated
key. Base and key expressions still run exactly once in their existing order;
reject before ToPropertyKey, as before. Diagnostics must not evaluate source,
coerce a key, invoke a getter/Host/callback, or infer how the base became nullish.
An inner failing operation keeps its own context rather than being relabeled by
an outer operation. Direct internal nullish checks outside member-reference
resolution remain unchanged and are not claimed as annotated by this increment.
The host suffix is ` [member operation=OP base=BASE key=KEY]`. Operation labels
are resolve-read, resolve-write-target, resolve-compound-target,
resolve-update-target, resolve-delete-target, resolve-call-target and
resolve-for-in-target. BASE is null or undefined. Recognized KEY names are plain;
redacted categories use angle brackets (for example `<string>` or `<object>`).
The complete annotated nullish diagnostic is bounded to 256 ASCII bytes at
construction, below the existing worker limit, without affecting other diagnostics.

Only a fixed public standard-property vocabulary may be named. Recognized keys:
prototype, constructor, length, name, message, call, apply, bind, toString, valueOf,
forEach, map, filter, some, every, reduce, push, pop, shift, unshift, slice, join,
concat, indexOf, includes, reverse, appendChild, removeChild, insertBefore, remove,
addEventListener, removeEventListener, querySelector, querySelectorAll,
getElementById, getElementsByTagName, getElementsByClassName, createElement,
createTextNode, setAttribute, getAttribute, hasAttribute, removeAttribute,
textContent, innerHTML, innerText, style, classList, className, id, parentNode,
parentElement, firstChild, lastChild, nextSibling, previousSibling, ownerDocument,
documentElement, head, body, children, childNodes, forms, elements, document,
navigator, location, href, search, cookie, userAgent, getComputedStyle, onload,
onclick, submit, focus. Matching is bounded against this fixed ASCII list and
does not copy a key buffer. Other strings are redacted to their category; number,
boolean, null, undefined, Symbol, ordinary object, function, native and Host keys
are categories only, without values, handles or Symbol descriptions. No URL,
identifier, source fragment or arbitrary runtime string is captured.

Render a short labeled suffix only when this annotated fault escapes to existing
host diagnostic formatting. Catch consumes the unchanged Value and discards the
annotation; an explicit later throw of that value starts an unannotated throw.
Normal finally preserves the pending fault, while return or a replacement throw
overrides it. Swallowed faults cannot taint later execute/invoke calls. Fatal
errors/latching and genuine Error formatting remain unchanged. Reply.errors and
the existing 512-character worker error/64-error/protocol limits remain intact.

Use only fixed enum fields, without retained heap/source/key storage or new realm
charges/fuel. Measure Fault/Result layout and keep existing default-stack depth
guards valid; all existing realm, worker and dependency limits remain unchanged.
Independent tests must preserve caught strings, evaluation/coercion order, Host
calls, catch/finally propagation, redaction, output bounds, allocation reports and
fatal behavior. Freeze an authored actual-worker baseline before production, then
verify the same readable fallback and later-created real controls through worker,
native and CDP paths. Such a fixture may already navigate successfully on the old
runtime: the new acceptance is accurate diagnostics without semantic regression,
not a new rendering capability. A subsequent bounded live checkpoint may identify
only the immediate operation, not the original producer or missing capability.

Independent acceptance passes 32 semantic and 15 resource groups, plus seven
private runtime groups, 34 DOM and 49 actual-worker tests. These cover all seven
operation labels, both nullish bases, the fixed vocabulary and redacted key
categories, evaluation/coercion order, catch/rethrow/finally replacement, fatal
latching, later-script recovery and no stale context across execute/invoke.
The four-byte context grows Fault from 32 to 40 bytes on x86_64; measured
Eval<Value>/Eval<Flow>/Eval<Reference> remain 40/64/56 bytes. Default-stack
pending-fault/finally checks and existing depth guards pass unchanged.
Frozen pre-change allocation reports, the 21,462-tick fuel checkpoint and the
265-byte public-invoke delta match exactly. The unchanged two-script worker
fixture already created its form before this change and still accepts exactly
57,479 bytes; its uncaught error now identifies resolve-call-target, null and
appendChild, while its caught error remains the original primitive string.

Full local validation passes 848 debug tests across 35 summaries and 749 selected
release tests across 26 summaries with Rust 1.91.1 and default thread stacks.
The first all-targets run exposed a test-list mistake (two scripts in a list
asserting one), corrected by a dedicated exact-two-script test without changing
the fixture, production, old assertions or limits. The original failed log is
retained alongside the successful rerun. Native/CDP and live checkpoints are
recorded separately below and in the daily log.

All three exact CI journey steps also pass locally: 19 native and 19 external
CDP destinations, including the diagnostic fixture's actual Unicode query,
hidden/submit fields, stale-node rejection, first local result click, flattened
session and decoded 1100×683 destination PNG. Native query and CDP destination
frames were inspected. The old fixture already navigated; this verifies preserved
behavior with better host diagnostics. No dependencies, TLS, worker permissions,
CDP commands, resource caps or thread-stack overrides changed.

One bounded live checkpoint still submitted Google's actual form through verified
TLS and ordinary cookies. Homepage HTTP 200/title Google retained 26 items/one
form, three completed scripts/seven errors and no allocation rejection at
2,428,666 accepted bytes. Search HTTP 200/title Google Search had zero items/forms,
two completed scripts and three errors. The first now reports
`[member operation=resolve-call-target base=undefined key=<string>]`; this names
only the immediate resolution failure, not the undefined value's producer or a
missing capability. Then Source 26,789 was rejected after 4,191,219 accepted bytes
against the unchanged 4,194,304 cap, repeated by the next script. Exit 2 and the
inspected blank search frame confirm no result, destination or new live stage.
No website-source inspection, adaptation or retry occurred. Changing server
responses are not a controlled performance comparison; the Google goal stays open.

Implementation 0ca28d6 passed exact-SHA Rust CI 34178535480 with the same 848/749
test totals and 19/19 journeys. Pages 34178535502 deployed the matching 8,576-byte
HTTPS body, with approved apex certificate and HTTPS enforcement. The daily log
records original remote evidence and the separate website visual-QA limitation.

## Retained interaction acceptance — 2026-09-07

The separately specified [page-session contract](PAGE_SESSIONS.md) now passes
905 debug tests, 792 selected release checks and all four exact CI journey steps
locally: 20 native and 20 external CDP destinations. The new frozen fixture
requires later handlers, two cancellations, a moved Unicode input, retained
closure proof and a handler-updated result URL. The server sees zero trap requests.
Native/CDP frames were inspected. Independent actual-process tests cover framing,
stale identities, aggregate admission, terminal replies and owned-child cleanup.
The old one-shot worker and its tests remain. The measured added browser-host
setup costs 794 Runtime bytes; language-only allocation checkpoints and existing
execution caps remain unchanged. Retained lifetime/transaction/wire policies are
explicit new bounds, not claims of unchanged protocol capacity.

One subsequent live Google attempt loaded HTTP 200/title Google with 26 items,
one form, five completed scripts and five errors. Two later page activations
completed in the same realm before the actual form submitted. Search HTTP 200
still rendered zero items/forms, with two completed scripts and three errors:
the same redacted undefined method-call base, then Source 27,142 rejected after
4,192,013 accepted bytes against the 4,194,304 limit, repeated by the next script.
The additional 794 accepted search bytes are the explicitly measured host setup;
the served source/request size is not a controlled benchmark. Exit 2 and an
inspected blank frame leave the first-result/destination goal incomplete. Raw
live artifacts remain ignored; no website-source adaptation or retry occurred.

## Call receivers and immediate producer context

This increment passes local acceptance. Independently authored cases froze the
old non-member-call discrepancy: the evaluator supplied the global object before
dispatch, so a detached Object.prototype.toString observed Object instead of
Undefined. Per ES5.1 sections 11.2.3, 10.2.1.1.6, 10.2.1.2.6 and 15.2.4.2,
non-member calls in the supported environments supply undefined. Ordinary
non-strict user functions perform their existing global substitution themselves;
native functions must receive the actual receiver. Preserve member, bound,
primitive, direct/indirect eval and argument evaluation behavior. This is a generic
semantic correction, not a diagnosis of Google's error.

The browser's existing EventTarget add/remove-listener entries must apply their
own nullish-to-Window receiver rule, following [Web IDL operation functions](https://webidl.spec.whatwg.org/#es-operations).
This applies to the shared Window/document/node listener operation, not arbitrary
Host functions or JavaScript builtins. Explicit document/node receivers retain
their targets and other invalid receivers still reject. Preserve existing
argument conversion, listener identity/capture/removal, limits and callback order.

For nullish member failures, add one successful evaluation's immediate origin to
the private fault. Retain the existing member block and append
` [producer kind=K]`, with ` key=KEY` before its closing bracket for a property
read. Fixed kinds are binding, expression, present-property, missing-property,
getter-result, host-get, user-call, native-call, host-call and bound-call. A binding
read stops attribution; assignments, conditional/sequence results and other
expressions use expression without tracing backward. Calls describe the immediate
dispatch, not the ultimate producer through eval/call/apply/bound forwarding.
Host-returned undefined does not prove an absent or unsupported Host capability.
The existing primitive-string out-of-range index fast path does not inspect the
prototype chain; conservatively label its result expression without a key, not
missing-property. This increment does not repair that separate lookup limitation.

Observe property presence/getter invocation during the existing single traversal,
not a repeated has/descriptor/get operation. Classify a producer property key only
from the already-converted PropertyKey, using the same fixed public vocabulary
and redaction; no source, arbitrary names, values or handles are retained. The
metadata belongs to the caller-local evaluation result, never global last-error
state. Later key evaluation, nested callbacks and caught failures cannot overwrite
it. A failing inner operation retains its own context. Do not expand this into a
provenance history or infer the cause of a missing value.

Producer instrumentation preserves page-visible exceptions, catch/finally/fatal
behavior and exact fuel/allocation reports. The separate receiver correction
intentionally changes formerly incorrect native-call behavior. Preserve existing
caps, dependency policy and worker isolation. The
complete host diagnostic stays within 256 ASCII bytes without realm charges or
callbacks. Keep existing Fault/Value/Flow/Reference result-layout bounds and
default-stack guards; at most seven fixed bytes of MemberContext are permitted.
Acceptance requires independent authored order/privacy/semantics tests, frozen
pre-change resource reports, actual-worker recovery and the existing native/CDP
script-created-form and retained-event journeys. Only then run one bounded live
checkpoint, keeping the full Google goal open unless actual results and onward
navigation are observed.

Acceptance — 2026-09-07: 24 independent receiver cases (eight fail on the old
build), 26 producer-semantic and nine resource groups pass, alongside 11 private
diagnostic, 35 DOM, 18 page-event and 50 actual-worker groups. Seven frozen phase
tuples, 21,120 remaining fuel ticks, Host/getter counts and old diagnostic resource
checkpoints remain exact. MemberContext is seven fixed bytes; Fault and
Eval<Value>/Eval<Flow>/Eval<Reference> remain 40/40/64/56 bytes. Bootstrap remains
25,999. A formerly failing authored page now creates its real form at 56,597
accepted bytes, after an ordinary missing-property failure with the expected
producer suffix. The unchanged legacy diagnostic page retains its 58,273-byte
browser checkpoint. All 971 debug tests, 858 selected release checks and all four
exact native/CDP CI steps pass locally: 21 native and 21 external CDP journeys,
including retained cancellations and zero trap requests. Query/destination frames
were inspected; owned workers and fixture processes were reaped.

One subsequent live Google attempt returned HTTP 200 homepage with 26 items/one
form, five completed scripts/five errors and 2,429,450 accepted bytes. Two later
activations completed before submitting the actual form. Search HTTP 200 remained
blank with zero items/forms, two completed scripts and three errors. Its first
diagnostic is now `[member operation=resolve-call-target base=undefined key=<string>]
[producer kind=binding]`. Attribution stops at that binding and does not identify
its origin or a missing capability. Later Source 26,963 was rejected after
4,192,013 accepted bytes against the unchanged 4,194,304 cap, repeated by the next
script. That later failure does not establish the earlier TypeError's cause.
Exit 2 and the inspected blank search frame leave the first-result/destination
goal incomplete. No live-source inspection, adaptation or retry occurred; raw
artifacts remain ignored. Changing server responses are not a controlled benchmark.
