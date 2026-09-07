# Running the research browser

The current target is Linux with an X11 or XWayland display. The application uses
Rust X11 protocol code and software-rendered pixels; it does not embed another
browser, a native UI toolkit, or a native font renderer.

```sh
cargo run --locked --bin mgbrowser -- https://www.google.com/
```

A readable font file is required. The browser tries DejaVu Sans and Liberation
Sans at common Linux locations. Set `MGBROWSER_FONT=/absolute/path/font.ttf` to
choose another file; this reads font data and does not call a platform font API.

Use Ctrl+L to edit the URL, Enter to navigate, Tab to move between document input
fields, Enter in a field to submit its form, and the mouse to activate links and
buttons. Scroll with the wheel/Page Up/Page Down; Back/Next/Reload controls are in
the toolbar. Text editing initially supports typing, select-all, and Backspace;
there is no clipboard or full cursor/selection editor yet.

## What it renders

HTML is parsed by our own bounded tokenizer/tree builder, then flattened into a
simple flowing document with headings, text, links, and form controls. Rustybuzz
shapes text and fontdue rasterizes it. An original partial JavaScript interpreter
is available only with `--enable-scripts`; see below. Full CSS, font fallback/bidi
layout and downloaded image rendering are not implemented. Images show a
placeholder and alt text. These limits must not be mistaken for compatibility
with the modern web.

HTTP(S) uses our own HTTP/1.1 transport and the selected experimental RustCrypto
TLS provider, with public trust roots and certificate verification. Cookies are
memory-only. HTML zero-delay refreshes are bounded. The script worker has a
restricted process boundary, but the browser's renderer/network do not. There is
no credential store, HTTP/2, proxy configuration or persistent browsing profile.

## Experimental scripting

Scripting defaults to off. On Linux x86_64, opt in explicitly:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-home --enable-scripts
```

Start the local fixture server described below first. The script creates the
form, title and startup status; no form controls are present outside its script.
The worker executes a small non-strict language subset, including bounded
`for-in`/`switch`, regular expressions and explicit-state expression parsing,
plus startup DOM callbacks,
then returns the changed document. It has no external script loading, persistent
realm, general event loop/timers, fetch/XHR, or script cookie access. Most modern
sites will still fail. [JAVASCRIPT.md](JAVASCRIPT.md) records exact capabilities,
limits and known semantic approximations.

Every script document uses a fresh worker with an empty environment, closed
inherited descriptors, Linux seccomp/resource limits and a two-second parent
deadline. Unsupported isolation/platforms refuse execution. A worker failure or
rejected document retains the original page, including `noscript`; a valid partial
snapshot can still be applied while showing script errors. The status bar and
stderr distinguish completed, partial, rejected and failed worker results.
Each reported partial error also appears as an escaped `SCRIPT_DIAGNOSTIC` line,
so a first parser failure does not hide other missing capabilities in that response.
`SCRIPT_ALLOCATION` contains a fixed JSON realm report with accepted bytes,
exclusive charge-site totals and the first rejected allocation. Repeated script
errors can be the same latched failure; they are not separate allocation attempts.
Only the fixed `SCRIPT_ALLOCATION` report excludes source text and URLs; it adds
no page API. `SCRIPT_DIAGNOSTIC` can contain page-supplied exception text or URLs:
escaping is not redaction, so keep raw live logs in ignored `tmp/`.
The report is cumulative logical accounting, not measured process memory.
`--disable-scripts` explicitly selects the default behavior.

The local containment self-test requires no display and starts only owned children:

```sh
cargo run --locked --bin mgbrowser -- --script-worker-selftest
```

This checks denied worker capabilities, memory/CPU/wall/output limits and bounded
pipe exchange. It is not an audit of the whole browser or authorization to treat
it as production-safe.

## Repeatable local interaction check

In one terminal:

```sh
cargo run --locked --example journey_server
```

In another:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/ \
  --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/local-journey
```

The browser opens a real window and drives the same click, text, and Enter
handlers used by desktop input. It captures its rendered frames while opening
the fixture form, typing a query, submitting it to the local server, and clicking
the first result heading. It exits nonzero if any required stage fails. These
are scripted application-handler checks, not independent physical keyboard input
or proof about Google's server behavior. The local pages are explicitly labeled
fixtures and contain no fabricated Google results.

With the same server running, exercise the script-built form and script redirect:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-redirect \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/script-journey
```

`/script-redirect` uses `location.replace('/script-home')`. `/script-home` creates
the actual query/hidden/submit controls with DOM methods and updates visible text
in a `DOMContentLoaded` callback. `/script-loop` is an explicitly local infinite
loop fixture: it should show a readable fuel error and allow normal navigation
afterward. On a headless Linux host, prefix the browser command with `xvfb-run -a`.
The native script-redirect/form/result/destination journey passed under Xvfb on
2026-09-07; rendered frames were inspected.

`/script-dynamic` is a separate authored fixture with no static form controls.
It uses the original Function constructor to build the form and direct `eval`
to read a local variable. Use that path in the native command above or as the
CDP journey URL below to exercise dynamic compilation inside the same restricted
worker. JavaScript's `eval` does not imply support for CDP `Runtime.evaluate`.

`/script-regexp` likewise has no static controls. Original regex captures,
`lastIndex`, replacement and splitting create its usable form. Substitute that
path in either journey command to exercise the matcher through a real worker and
native/CDP input. Both paths passed on 2026-09-07, with rendered frames inspected.
`cargo test --locked --test js_regexp` runs the independent regex semantics and
resource-limit cases without a display or external JavaScript engine.

`/script-iteration` creates every control by enumerating own and inherited fields
with `for-in` and choosing input/button types with `switch`. Run it with the same
fixture server:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-iteration \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/iteration-journey
```

Its native and external CDP form/query/hidden-field/result/destination journeys
passed on 2026-09-07; rendered frames were inspected. Use
`cargo test --locked --test js_iteration` for independently authored iteration,
switch, mutation, scope and resource-limit cases. Enumeration has a bounded
snapshot policy and does not support DOM host objects; see [JAVASCRIPT.md](JAVASCRIPT.md).

`/script-expressions` creates every control through a DOM factory wrapped in
exactly 64 grouping pairs, exercising nested function parsing while expression
state remains pending. It is an independently authored local fixture, not a
website-script adaptation:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-expressions \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/expressions-journey
```

The fixture's actual-worker form-creation check and native/CDP
form-to-destination journeys passed on 2026-09-07; rendered frames were inspected.
The expression increment passed 251 debug tests and 141 selected release tests without increasing
thread-stack sizes, including mixed evaluator/helper recursion regressions.
Parser storage/work is bounded across nested functions, while source/token/node
and structural/AST-depth caps remain unchanged. Logical evaluation-depth guards
are tested, not a claim of production stack safety.

`/script-allocation` is a local storage-regression fixture with no static controls.
It builds a compiled DOM factory containing 9,999 harmless expression statements,
then calls it to create the form. The pre-sharing worker rejected the redundant
function-body copy; the fixture must complete within the same 4 MiB realm budget:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-allocation \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/allocation-journey
```

The shared-code increment completed this fixture with 3,745,970 accepted bytes,
including 137 bytes of per-instance function-code metadata. Native and external CDP journeys
submit the query/hidden field and reach the local destination; rendered frames
were inspected. That increment passed 288 debug tests and 178 selected release tests. Consult
the daily log for the baseline, final local checks and publication evidence.

`/script-arrays` is a separate authored storage fixture: it retains six independent
10,000-slot arrays with holes, checks their contents, then creates every form
control in the restricted worker. The baseline exhausted allocation before the
form existed; the prepaid-ownership increment completes under the same cap:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-arrays \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/arrays-journey
```

At that increment, the complete worker accepted 3,891,459 bytes and created the form without errors.
Native and external CDP journeys reach the local destination; rendered frames were
inspected. That increment passed 312 debug tests and 202 selected release checks. The seventh
maximum array still fails under the unchanged cumulative limit. See the daily log
for publication evidence; this local fixture is not Google acceptance.

`/script-bindings` is another authored fixture with no static controls. It calls
a form-building function with a generated 749,925-unit string. The real input and
independent formal copy remain charged; moving the copy into its binding no longer
adds a duplicate payload charge. At that increment, the fixed fixture completed with 3,681,962
accepted bytes under the unchanged 4 MiB limit:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-bindings \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/bindings-journey
```

That increment passed 327 debug tests, 217 selected release checks and all three exact CI journey
steps locally, including native/CDP submission and destination navigation for
this form. Native query and CDP destination frames were inspected. A larger real
copy still fails fatally; generic ingress/binding/catch and read charges remain.

`/script-sources` compiles and calls a form builder using a generated 749,925-unit
whitespace parameter fragment. A sole owned fragment now moves without an
unnecessary joined-buffer copy; source creation and UTF-8 conversion still pay.
At that increment, the unchanged fixture completed with 2,935,365 accepted bytes below 4 MiB:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-sources \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/sources-journey
```

That increment passed 344 debug tests, 234 selected release checks and all three
exact CI journey steps locally. Native/CDP paths submit its real Unicode query and hidden
field, click the local result and reach the destination; frames inspected.
Multiple source fragments still require a charged joined buffer, and the worker
negative control preserves fatal UTF-8 exhaustion and readable fallback.

`/script-ast` is the frozen AST-storage fixture. Its generated Function contains
19,998 harmless statements followed by 19 real form-building statements, with
no static form. Use the same native or external CDP journey:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-ast \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/ast-journey
```

Its pre-change worker failed AST admission before creating controls. At that
increment the unchanged fixture completed at 2,481,036 accepted bytes with real controls. Native and
external CDP paths submit its Unicode query/hidden field and reach the local
destination; frames were inspected. Smaller statement storage and capacity-aware
AST charges, including holes, preserve every limit. A repeated uncalled sparse
function still exhausts AST admission without handlers or later-script effects.
That increment passed all 368 debug tests, 258 selected release checks and all
three exact CI journey steps locally with 11 native and 11 external CDP destinations.

`/script-symbols` is the frozen Symbol-dependent form. Distinct identities,
symbol-key lookup/reflection, string-key filtering and registry lookup must work
before any control is created:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-symbols \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/symbol-journey
```

At the Symbol increment its unchanged missing-API baseline completed in the actual
worker at 57,954 accepted bytes. Native and external CDP paths submitted the
query/hidden field and reached the local destination; frames inspected. That
increment passed all 438 debug tests, 340 selected release checks and the three
exact CI journey steps locally (12 native and 12 external CDP destinations).
Ordinary Symbol-to-DOM conversion errors preserve
the mutation target and allow later scripts; fatal cumulative creation still
stops catch/finally/later execution. See JAVASCRIPT.md for exact scope and limits.

`/script-prototypes` creates its form only after checking genuine user/native
prototype identity, inherited metadata and Symbol keys, and function-valued
constructor prototypes:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-prototypes \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/prototypes-journey
```

At the typed-prototype increment the unchanged fixture completed at 78,118 accepted
bytes in the actual restricted worker. All 492 debug tests, 394 selected release
checks and three exact CI journey steps passed locally (13 native/13 external CDP
destinations). Its query/hidden field
submits normally and the actual local result is clicked; native query and CDP
destination frames were inspected. Exact legacy traversal limits and all caps
remain, with separate fatal depth and ordinary error/recovery checks.

`/script-errors` is the frozen Error-family form. The six exposed family prototypes,
inherited defaults, real TypeError instances and generic string conversion must
work before any query/hidden/submit control is created:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-errors \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/errors-journey
```

The unchanged authored fixture passes the focused actual-worker check at 76,751
accepted bytes with one completed script and no errors. Focused DOM/worker tests
also cover real Error text/title/navigation conversion, callback-free uncaught
diagnostics, ordinary recovery and fatal fuel/output-allocation checks. All 547
debug tests, 449 selected release checks and three exact CI journey steps pass
locally (14 native/14 external CDP destinations). Native/CDP Error-family forms
submit the real Unicode query and hidden field, then reach the local destination;
form and destination frames were inspected. Independent semantic/resource checks
pass with no limit changes. Publication evidence is in the daily log; authored
local success is not Google compatibility.

`/script-concat` is the frozen concat-built form. Dense and sparse concatenation,
inherited numeric reads, trailing holes, nested identity, UTF-16 and unchanged
source slots must work before any control is created:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-concat \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/concat-journey
```

The unchanged authored page passes the actual-worker check at 75,704 accepted
bytes, one completed script/no errors. Focused tests cover opaque DOM node identity
and actual text/title/navigation, Symbol conversion rejection, ordinary nullish
receiver recovery, and fatal result-length/element-copy limits. The daily log
records 604 debug tests, 506 selected release checks and all three exact CI
journey steps passing locally (15 native/15 external CDP destinations). Root
inspected the real query and destination frames. This is application-handler
and public CDP verification, not physical keyboard or Google acceptance.

`/script-empty-arguments` is the authored empty-snapshot storage fixture. It
completes 8,500 zero-argument calls without reading their arguments bindings,
then checks distinct snapshot identities, the original callee, Arguments brand,
Object.prototype parent, one explicit undefined argument and direct-eval reads.
Only after those checks does it create the actual query, hidden and submit controls:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-empty-arguments \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/empty-arguments-journey
```

The unchanged page now passes its actual-worker check at 2,317,555 accepted
bytes under the same 4,194,304-byte cumulative limit. Its old-worker baseline
stopped before creating any form at 4,194,254 accepted bytes, rejecting a
137-byte Runtime charge. These runs complete different amounts of work, so this
is not a controlled performance or process-memory benchmark. Only empty snapshots
are deferred; observed snapshots and all nonempty lists retain their real storage
and existing behavior. All 659 debug tests and 561 selected release checks pass
locally with unchanged limits. All three exact CI journey steps also pass locally,
with 16 native destinations and 16 external CDP successes. The new client journey
submits the actual Unicode query, hidden and submit fields, rejects stale nodes,
clicks the local first result and verifies a 1100×683 destination PNG and flattened
session. Root inspected the native query and CDP destination frames; owned
fixture/debugger ports were released. This verifies application handlers and the
external CDP path, not physical keyboard input or Google acceptance. The separate
post-change live checkpoint below still fails.

For the live target, use the same command with `https://www.google.com/`,
`--enable-scripts` and `--evidence-dir tmp/google-journey`. It uses the actual
returned form controls, actual heading links, and ordinary session behavior. A JavaScript/interstitial
response without results is a failed journey, not a pass. Keep public-network
checks manual and bounded; ordinary CI uses only the local fixture. The
2026-09-07 post-AST Google attempt submitted the real form. The homepage retained
26 items/one form and no allocation rejection. Search HTTP 200 still has no
items/forms, result or destination. Its first error is now `Symbol is not defined`;
a later script rejects Ast 387,844 after 4,078,595 accepted bytes, and the next
script repeats that failure. Two scripts complete with three errors overall.
The search frame remains blank and the journey exits 2. The earlier admission
failure is no longer first, but Google's acceptance goal remains unmet. Next is
genuine Symbol/property-key support and separate measured storage diagnosis;
that Symbol increment is now implemented and tested above.

The post-Symbol Google attempt also submits the actual form. Homepage HTTP 200
has no rejected allocation; search HTTP 200 remains blank with two completed
scripts/three errors. First is now `TypeError: prototype must be an object or null`,
followed by an AST rejection requesting 387,964 after 4,107,538 accepted bytes.
Exit 2, no actual result or destination. The message does not establish which
prototype value was supplied. Next is independent object/prototype correctness
and cumulative-storage work; real DOM events and timers are also still missing.

The post-typed-prototype checkpoint still reports that same leading TypeError.
Actual form submission works, but search HTTP 200 remains blank with two completed
scripts/three errors; Ast 387,500 is rejected after 4,107,727 accepted bytes and
the later script repeats that failure. Exit 2, no result or destination. The
generic correction has no observed benefit on the leading live diagnostic;
further builtin prototype and storage work requires independent authored cases.

The post-Error checkpoint moves to unsupported Array.concat as its first search
error. Homepage/form submission still work; search HTTP 200 remains blank with two
completed scripts/three errors, followed by Ast 387,620 rejected after 4,132,042
accepted bytes. Exit 2, no result or destination; blank frame inspected. The prior
prototype error is absent in this response, not a controlled benchmark or proof
of its original argument. Next is independently authored bounded concat support
and separate cumulative-storage diagnosis, with every execution limit retained.

The post-concat checkpoint still submits the real Google form with verified TLS
and ordinary cookies. Homepage HTTP 200 has 26 items/one form and no allocation
rejection (2,511,733 accepted bytes). Search HTTP 200 remains blank with two
completed scripts/three errors, now repeating a FunctionCode allocation rejection
after 4,194,294 accepted bytes, requesting 128 against the 4,194,304 limit. Exit 2,
no result/destination; blank frame inspected. Unsupported concat is absent in this
response, not a controlled benchmark. Next is independent cumulative-storage
ownership diagnosis without raised limits or adapting live page source.

The post-empty-arguments checkpoint again submits the actual form. Homepage HTTP
200 has 26 items/one form and no rejected allocation (2,505,994 accepted bytes).
Search HTTP 200 remains blank, with two completed scripts/three errors repeating
Runtime 128 rejected after 4,194,254 accepted bytes against the same 4,194,304
limit. Exit 2, no result or destination; blank frame inspected. No new live
journey stage completed. Continue independently measured storage/ownership work;
this changing response is not a controlled benchmark or a reason to raise caps.

## Browser automation

Enable the experimental Chrome DevTools Protocol subset explicitly:

```sh
cargo run --locked --bin mgbrowser -- https://www.google.com/ --remote-debugging-port=9222
```

Discovery is at `http://127.0.0.1:9222/json/list`; the page WebSocket is
`ws://127.0.0.1:9222/devtools/page/page-1`. Port zero chooses an available port
and prints its address. Debugging is disabled by default and grants local clients
control of the page. See [CDP.md](CDP.md) for commands, limits, and the external
Rust fixture client. This is not yet full DevTools/Playwright compatibility.

For the scripted local fixture, start the server, then launch the browser with
both flags (on an unused debugger port):

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-home \
  --enable-scripts --remote-debugging-port=9222
```

Run the independent client in another terminal:

```sh
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-home tmp/cdp-script-journey.png
```

The same client can navigate that scripting-enabled browser to the regex,
iteration or grouped-expression fixture:

```sh
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-regexp tmp/cdp-regexp-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-iteration tmp/cdp-iteration-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-expressions tmp/cdp-expressions-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-allocation tmp/cdp-allocation-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-arrays tmp/cdp-arrays-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-bindings tmp/cdp-bindings-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-sources tmp/cdp-sources-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-ast tmp/cdp-ast-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-symbols tmp/cdp-symbol-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-prototypes tmp/cdp-prototypes-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-errors tmp/cdp-errors-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-concat tmp/cdp-concat-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-empty-arguments tmp/cdp-empty-arguments-journey.png
```

This checks the real DOM-created form, Unicode input, hidden field, result click,
destination response and PNG through our public CDP endpoint. The previously
implemented script-home/loop recovery, dynamic, regex, iteration, grouped-expression, shared-code, prepaid-array, parameter-copy, source-ownership, compact-AST, Symbol, typed-prototype, Error-family, concat and empty-arguments
journeys passed on 2026-09-07. It does not use
`Runtime.evaluate`: CDP Runtime/Debugger are still unimplemented despite the new
page interpreter. Stop only the fixture/browser processes you started.

## Validation

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --release --lib --test js_expressions --test js_allocation --test js_arrays --test js_bindings --test js_sources --test js_ast_storage --test js_symbols --test js_symbol_keys --test js_symbol_limits --test js_prototypes --test js_prototype_limits --test js_errors --test js_error_limits --test js_concat --test js_concat_limits --test js_empty_arguments --test js_empty_arguments_limits --test js_dom --test script_worker
mkdir -p tmp
rustc --edition=2024 tools/check-dependencies.rs -o tmp/check-dependencies
tmp/check-dependencies
```

The unit/integration checks cover parser behavior, Rust font painting, verified
local TLS handshakes and rejection cases, HTTP framing, redirects, cookie scope,
UI/CDP state transitions, original JS syntax/evaluation, DOM mutation and actual
restricted worker children. Consult FEATURES.md and the daily log for dated
end-to-end evidence and remaining acceptance gates. Fixture success is not
language conformance or public-site compatibility.

For a focused expression-parser check, run `cargo test --locked --lib js::syntax`
and `cargo test --locked --test js_expressions --test script_worker`. The release
command above also exercises the library and real worker children; these tests
do not increase the native thread-stack size. Passing language cases alone does
not replace the native/CDP fixture journey or the bounded live-site gate.
