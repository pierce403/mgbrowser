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

Plain `http://` URLs work as well as HTTPS. Since v0.1.1, the browser displays a red
title/address strip and "HTTP: Not secure" for a loaded HTTP page; the desktop
window manager still controls the outer decoration. Typing HTTPS in the location
field does not clear the warning until that page loads. Ctrl+L selects the whole
location from either the page or a form field; type a URL and press Enter.
Re-run the website installer to get the latest feature release.

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
then retains the realm for bounded real later click/submit handlers. It has no
external script loading, general event loop/timers, fetch/XHR, or script cookie access. Most modern
sites will still fail. [JAVASCRIPT.md](JAVASCRIPT.md) records exact capabilities,
limits and known semantic approximations.

Every script document uses a fresh worker with an empty environment, closed
inherited descriptors, Linux seccomp/resource limits and a two-second TOTAL active
parent budget across startup and later input, not a fresh deadline per click.
Retained sessions additionally have a 300-second absolute lifetime, 64 transactions
including initialization, and a 32 MiB combined wire budget. No automatic replay or
restart renews these bounds; see [the full session contract](PAGE_SESSIONS.md).
Unsupported isolation/platforms refuse execution. A worker failure or
rejected document retains the original page, including `noscript`; a valid partial
snapshot can still be applied while showing script errors. The status bar and
stderr distinguish completed, partial, rejected and failed worker results.
Each reported partial error also appears as an escaped `SCRIPT_DIAGNOSTIC` line,
so a first parser failure does not hide other missing capabilities in that response.
`SCRIPT_ALLOCATION` contains a fixed JSON realm report with accepted bytes,
exclusive charge-site totals and the first rejected allocation. Repeated script
errors can be the same latched failure; they are not separate allocation attempts.
The fixed `SCRIPT_ALLOCATION` report excludes source text and URLs; it adds
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
`cargo test --locked --workspace --test js_regexp` runs the independent regex semantics and
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
`cargo test --locked --workspace --test js_iteration` for independently authored iteration,
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

`/script-function-prototypes` is the authored default-prototype storage fixture.
It creates 4,800 fresh functions before checking metadata, unique defaults and
constructor backlinks, inherited-owner reads, construction/instanceof and old
prototype lifetime. It has no static form; only after those checks does it create
the actual query, hidden and submit controls:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-function-prototypes \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/function-prototypes-journey
```

The unchanged page passes its actual-worker check at 3,232,783 accepted bytes,
one completed script and no errors. Its old-worker baseline rejected Runtime 137
after 4,194,256 accepted bytes before any controls existed. Different completed
work makes those totals unsuitable for an equal-work performance or RSS claim.
Only the unused default object/backlink is deferred: the real own property and
its metadata remain, actual first reads admit storage once, and observed defaults
retain identity. An observed-default worker control still fails fatally at the
same 4 MiB cap, preserving earlier effects and bypassing handlers/later work.
Object.prototype.constructor and Array.prototype.constructor remain absent;
this storage change adds neither native backlinks nor full descriptor support.

All 721 debug tests and 623 selected release checks pass locally, including
30 DOM and 43 actual-worker groups; formatting, locked builds and the dependency
guard also pass. All three exact CI journey steps pass locally under unchanged
deadlines, with 17 native and 17 external CDP destinations. The new fixture submits
its actual Unicode query, hidden and submit fields, rejects stale nodes, clicks
the first local link and verifies the flattened session and 1100×683 destination
PNG. Root inspected the native query/ready-form and CDP destination frames; owned
servers exited and ports 7878/9222 were clear. These are application-handler and
external protocol checks, not physical input or Google acceptance. The post-change
live checkpoint below still fails; exact-SHA publication is recorded in the daily log.

CI follow-up: the documentation closeout run exhausted the old shared 50-second
deadline after 16 of 17 total CDP journeys, despite all component/release tests and
native journeys passing. At that increment, the 16 scripted CDP fixtures were split
into two groups of eight, each with its own browser, log, cleanup and 50-second
deadline. The original loop/fallback recovery stayed in the first browser before
onward navigation. All fixture commands, screenshots and assertions were retained;
browser/worker resource limits and individual native journey deadlines were unchanged.
This adjusted the aggregate test-harness schedule, not the engine's execution budget.

`/script-bound-functions` is the frozen bound-callable fixture. Calls, receiver and
prefix retention, rebinding, native targets, restricted inherited properties,
ordinary name/prototype fields, construction/instance checks and Symbol identity
must succeed before a bound DOMContentLoaded callback creates the actual query,
hidden and submit controls. There is no static form:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-bound-functions \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/bound-functions-journey
```

The unchanged authored page passes the focused actual-worker check with one
completed script, no errors or allocation rejection, and 104,479 accepted bytes:
Bootstrap 25,999 + Source 2,556 + Ast 49,886 + FunctionCode 1,652 + Runtime 24,386.
The old-worker baseline stopped at the first absent-bind TypeError, accepting
84,032 bytes with no form and no allocation rejection. These executions complete
different amounts of work; their totals are not a performance or process-memory
benchmark. Focused checks pass all 32 DOM and 46 actual-worker groups, including
bound Host methods/startup callbacks, Symbol conversion rejection, ordinary error
recovery and fatal 65-wrapper latching before target/catch/finally/later effects.
The same 4,194,304-byte realm cap and all other execution limits remain in force.
Full regression passes 788 debug tests/33 summaries and 690 selected release
checks/24 summaries. All three exact CI journey steps pass locally with 18 native
and 18 external CDP destinations.
The bound fixture submits its actual Unicode/hidden/submit controls, rejects stale
nodes, clicks the first local result and verifies the flattened session and a
1100×683 destination PNG. Main inspected native query and CDP destination frames;
owned test processes exited and fixture/debugger ports were clear. These checks
exercise application input handlers and public CDP, not physical input or Google.
Exact-SHA remote CI, Pages and HTTPS publication verification remain separate and
pending.

`/script-diagnostics` is an authored two-script diagnostic/recovery fixture with
no static controls. The first script preserves an earlier DOM effect, then fails
to resolve `null.appendChild` before evaluating its call arguments. The second
checks the unchanged caught exception string, evaluates an object key once
without coercing it, skips the failed assignment's RHS, and creates the actual
query, hidden and submit controls:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/script-diagnostics \
  --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke \
  --evidence-dir tmp/diagnostics-journey
```

The frozen old-worker baseline and focused candidate worker check both complete
one script, report one error, retain one form and accept exactly 57,479 bytes:
Bootstrap 25,999 + Source 1,531 + Ast 23,299 + FunctionCode 128 + Runtime 6,522;
both regex phases are zero, with no allocation rejection. Earlier effects,
caught values, control creation and the same 4,194,304-byte cap are unchanged.
The new acceptance is the escaping host-only suffix
`[member operation=resolve-call-target base=null key=appendChild]`, not a new
rendering capability or performance improvement.

Only these annotated nullish-member failures receive the fixed, redacted context:
an operation, null/undefined base kind and allowlisted standard key or category.
The complete annotated runtime message is at most 256 ASCII bytes and does not
retain arbitrary keys, URLs, Symbol descriptions or Host handles. Caught code
still receives exactly `TypeError: property access on null or undefined`.
Other `SCRIPT_DIAGNOSTIC` lines can contain page-supplied text or URLs; escaping
does not redact them. Keep raw live logs in ignored `tmp/`. See
[JAVASCRIPT.md](JAVASCRIPT.md) for the vocabulary and propagation contract.
All 848 debug tests/35 summaries and 749 selected release checks/26 summaries
pass. All three exact CI journey steps pass locally with 19 native and 19
external CDP destinations. The diagnostic form submits its Unicode query and
hidden/submit controls, rejects stale nodes, and reaches the local destination;
the client verifies the flattened session and 1100×683 PNG. Native query and CDP
destination frames were inspected, and owned test services were stopped. These
checks prove preserved local behavior, not a new Google journey stage.

CI now schedules scripted CDP fixtures in three batches of 8/8/2, each with its own
browser, log, cleanup, 10-second readiness guard and 50-second deadline. The last
batch covers bound functions and diagnostics. All previous commands/assertions
remain, including the original loop/fallback recovery before onward navigation
in the first browser.
This adds bounded harness time for another fixture without changing any engine,
worker or individual native journey limit; all three updated batches passed
locally. Remote publication is verified separately in the daily log.

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

The post-default-prototype checkpoint again submits the actual form with verified
TLS and ordinary cookies. Homepage HTTP 200 has 26 items/one form, three completed
scripts/seven errors and no rejected allocation (2,428,284 accepted bytes). Search
HTTP 200 remains blank with zero items/forms and two completed scripts/three errors:
first a non-callable-value TypeError, then Source 26,999 rejected after 4,187,629
accepted bytes against the unchanged 4,194,304 limit, repeated by the next script.
Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination or newly
completed live stage. The generic diagnostic does not identify a missing callable.
Continue independent language/builtin coverage and measured storage ownership;
this changing response is not a controlled benchmark or permission to adapt live
source or raise limits.

The single bounded post-bind checkpoint still submitted the actual Google form.
Homepage HTTP 200/title Google retained 26 items/one form, three completed
scripts/seven errors and no allocation rejection (2,429,821 accepted bytes).
Search HTTP 200/title Google Search remained blank with zero items/forms and two
completed scripts/three errors: first `TypeError: property access on null or undefined`,
then a Source request of 26,794 rejected after 4,191,219 accepted bytes against the
unchanged 4,194,304 cap, repeated by the next script. Exit 2/JOURNEY_INCOMPLETE;
the blank 03-search.png was inspected. No result, destination or new live stage
completed, and no live-source inspection, adaptation or retry occurred. These
changing responses do not establish that bind caused the prior diagnostic or
provide a controlled performance comparison. The full Google goal remains open.

The post-diagnostic checkpoint likewise submits the actual Google form but
renders no results. Its first search error is now
`[member operation=resolve-call-target base=undefined key=<string>]`; this identifies
the immediate operation only, not the undefined value's producer or missing API.
Homepage accepted 2,428,666 bytes with 26 items/one form and no rejection. Search
accepted 4,191,219 before rejecting Source 26,789 under the unchanged 4,194,304 cap;
it completed two scripts with three errors and zero items/forms. Exit 2 and an
inspected blank search frame leave the result/first-link/destination gate open.
There was no live-source inspection, adaptation or second attempt.

The post-retained-event checkpoint completes two later activations on Google's
actual homepage and submits its real form with verified TLS. Homepage HTTP 200
has 26 items/one form, five completed scripts/five errors and 2,429,689 accepted
bytes at startup; the retained activations raise that total to 2,432,977. Search
HTTP 200 still has zero items/forms, two completed scripts/three errors: the same
undefined method-call target, then Source 27,142 rejected after 4,192,013 accepted
bytes against 4,194,304. Exit 2/JOURNEY_INCOMPLETE and an inspected blank frame:
no result or destination. There was one bounded attempt, without source inspection
or adaptation. This is an observed response, not a controlled performance comparison.

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
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-function-prototypes tmp/cdp-function-prototypes-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-bound-functions tmp/cdp-bound-functions-journey.png
cargo run --locked --example cdp_journey -- \
  ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-diagnostics tmp/cdp-diagnostics-journey.png
```

This checks the real DOM-created form, Unicode input, hidden field, result click,
destination response and PNG through our public CDP endpoint. The previously
implemented script-home/loop recovery, dynamic, regex, iteration, grouped-expression, shared-code, prepaid-array, parameter-copy, source-ownership, compact-AST, Symbol, typed-prototype, Error-family, concat, empty-arguments and default-function-prototype
journeys passed on 2026-09-07. It does not use
`Runtime.evaluate`: CDP Runtime/Debugger are still unimplemented despite the new
page interpreter. The bound-function native/CDP journey also passes, as recorded
above. The diagnostic fixture journey also passes. Stop only the
fixture/browser processes you started.

## Validation

For retained interaction, start `journey_server` and use the authored
`/script-events` fixture. Its valid destination cannot be reached without actual
later handlers and retained state:

```sh
target/debug/mgbrowser http://127.0.0.1:7878/script-events --enable-scripts \
  --smoke-events --exit-after-smoke --evidence-dir tmp/event-journey/native
```

The native application-handler driver enters Unicode text, activates a canceled
anchor, cancels the first submit, edits the moved input, then submits proof on the
second attempt and clicks the handler-updated result. It is not independent
physical mouse/keyboard input. The external client runs the same acceptance through
the public protocol against an owned browser with remote debugging enabled:

```sh
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 \
  http://127.0.0.1:7878/script-events tmp/event-journey/destination.png
cargo test --locked --workspace --test page_events --test page_projection --test script_session
target/debug/mgbrowser --script-session-selftest
```

CI also requires zero `/event-trap` requests and exactly two successful event
search/destination requests across native and CDP runs. Canceled updates emit
DOM.documentUpdated and invalidate old CDP node IDs without a fake page load.
Selftests cover real restricted children and manager cleanup; simulated elapsed
validation charges test cumulative accounting, not two seconds of real CPU work.
No new CDP commands or Runtime evaluation are exposed.

```sh
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --release --lib --test js_expressions --test js_allocation --test js_arrays --test js_bindings --test js_sources --test js_ast_storage --test js_static_operators --test js_static_operator_limits --test js_symbols --test js_symbol_keys --test js_symbol_limits --test js_prototypes --test js_prototype_limits --test js_errors --test js_error_limits --test js_concat --test js_concat_limits --test js_empty_arguments --test js_empty_arguments_limits --test js_function_prototypes --test js_function_prototype_limits --test js_bound_functions --test js_bound_function_limits --test js_diagnostics --test js_diagnostic_limits --test js_call_receivers --test js_producer_diagnostics --test js_producer_limits --test js_array_callbacks --test js_array_callback_limits --test js_core_intrinsics --test js_core_intrinsic_limits --test js_object_create --test js_object_create_limits --test js_dom --test script_worker --test page_events --test page_projection --test script_session
mkdir -p tmp
rustc --edition=2024 tools/check-dependencies.rs -o tmp/check-dependencies
tmp/check-dependencies
```

For retained AST storage, build and explicitly run the standalone measurement
example (ordinary example test harnesses do not run `main`):

```sh
cargo build --locked --example measure_ast_storage
timeout 15s target/debug/examples/measure_ast_storage
```

Its fixed 23 authored cases measure retained allocator requests, clone ownership
and final-drop cleanup in one thread. Twelve harmless cases also compare the
runtime's AST allowance; diagnostic/producer sources are parse-only. This is
requested storage, not RSS or a general autoresearch executor. CI runs this
example separately after building it; the original pre-change probe is preserved
with the hashes in the daily log. See [STATIC_OPERATORS.md](STATIC_OPERATORS.md).

For fresh-object descriptor storage, explicitly run the public eight-case guard:

```sh
cargo build --locked --example measure_object_create
env -u RUST_MIN_STACK timeout 5s target/debug/examples/measure_object_create
```

The fixed authored sources prepare identical realms before each measurement
window. The guard compares getter-only versus setter-pair storage, owned ASCII
keys and UTF-16 value copies, and checks complete realm-drop cleanup. It requires
successful descriptor support; the old unsupported behavior cannot pass. Actual
requested allocation sizes and logical Runtime charges are reported separately,
not as allocator usable sizes, peak memory or RSS. The measurement windows and
comparison gates preserve the frozen probe; only its public header and mandatory
support assertion differ, apart from formatting. CI runs this five-second guard
alongside the unchanged AST guard after building examples. Ordinary example test
harnesses do not run its `main`; this is not a general autoresearch executor.
See [OBJECT_CREATE.md](OBJECT_CREATE.md) for the bounded contract.

The unit/integration checks cover parser behavior, Rust font painting, verified
local TLS handshakes and rejection cases, HTTP framing, redirects, cookie scope,
UI/CDP state transitions, original JS syntax/evaluation, DOM mutation and actual
restricted worker children. Consult FEATURES.md and the daily log for dated
end-to-end evidence and remaining acceptance gates. Fixture success is not
language conformance or public-site compatibility.

For a focused expression-parser check, run `cargo test --locked -p mg-butane --lib syntax`
and `cargo test --locked --workspace --test js_expressions --test script_worker`. The release
command above also exercises the library and real worker children; these tests
do not increase the native thread-stack size. Passing language cases alone does
not replace the native/CDP fixture journey or the bounded live-site gate.

## Call receiver and immediate producer fixture

The independently authored `/script-producers` page first encounters a missing
property, then requires correct detached native receivers before creating its
search form. On c05ba38 it remained readable but created no form: the second
script reported Call receiver contract failed. Candidate acceptance uses the
unchanged source, actual restricted worker and the existing native/CDP clients:

```sh
cargo test --locked --workspace --test js_call_receivers --test js_producer_diagnostics --test js_producer_limits --test script_worker
target/debug/mgbrowser http://127.0.0.1:7878/script-producers --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/producers-native
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/script-producers tmp/producers-cdp.png
```

Start the owned fixture service and debugging browser as documented above; 9222
is an example chosen port. CI uses a fresh port and finite process deadlines.
The first error must retain its exact member block and append
`[producer kind=missing-property key=length]`. The corrected second script creates
the real form and the clients submit it, click the first local result and reach
the fixture destination. This is not a Google result or a new CDP command.
At that increment the preexisting `/script-diagnostics` fixture's exact checkpoint
stayed 58,273 bytes; its nullish local binding appends `[producer kind=binding]`.
Array.reduceRight registration later adds exactly 156 Bootstrap bytes, making the
checkpoint 58,429. Core constructor backlinks add another measured 725 bytes,
making it 59,154; all non-Bootstrap phases remain unchanged.

## Array callback and borrowed DOM collection fixture

The independently authored `/script-array-callbacks` page requires all seven
callback methods, then borrows Array.prototype methods on existing DOM collection
snapshots to create a real form. On c858a6f its unchanged source retains readable
fallback but creates no controls because Array.map is unsupported. It now creates
one form with zero errors at 93,783 accepted bytes at the callback increment.
Core backlinks add only 725 Bootstrap bytes, bringing this fixture to 94,508
under the unchanged 4 MiB cap.
See [ARRAY_CALLBACKS.md](ARRAY_CALLBACKS.md) for ordering, sparse/inherited values,
mutation, Host presence and explicit limits; this does not add live collections.

```sh
cargo test --locked --workspace --test js_array_callbacks --test js_array_callback_limits --test js_dom --test page_events --test script_worker
target/debug/mgbrowser http://127.0.0.1:7878/script-array-callbacks --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/array-callbacks-native
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/script-array-callbacks tmp/array-callbacks-cdp.png
```

Start the owned fixture server and debugging browser as documented above; 9222 is
an example, while CI chooses a fresh port and keeps finite deadlines. Verify the
Unicode query, hidden source and submit field, actual first local result and
separate destination, not just a screenshot or created control. At the callback
increment, 1,041 debug/928 selected release tests and 22 native/22 CDP journeys passed.
Counts exclude the repeated default-stack child summary inside the resource suite.
These fixtures prove local behavior only; the actual Google result gate remains open.

## Core constructor and primitive-prototype fixture

The independently authored `/script-core-intrinsics` page requires five original
constructor backlinks, three genuine primitive-prototype payloads and direct/bound
Number/Boolean construction. It recovers constructors from ordinary values and
builds actual query, hidden-source and submit controls. Its frozen b3f8fd3 worker
baseline remains readable but has no form and reports Original constructor
backlink required. The candidate completes one script with zero errors at 84,741
accepted bytes under the unchanged 4 MiB cap. The precise language/resource
contract and acceptance status are in [CORE_INTRINSICS.md](CORE_INTRINSICS.md).

```sh
cargo test --locked --workspace --test js_core_intrinsics --test js_core_intrinsic_limits --test js_dom --test page_events --test script_worker
target/debug/mgbrowser http://127.0.0.1:7878/script-core-intrinsics --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/core-intrinsics-native
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/script-core-intrinsics tmp/core-intrinsics-cdp.png
```

Start the owned fixture service and debugging browser as documented above; 9222
is an example, while CI selects a fresh port. Require the real form fields, first
authored result and separate destination, not merely a created control. This is
local fixture verification, not a Google result or additional CDP command.

## Static operator storage fixture

The frozen `/script-static-operators` page generates 14,500 binary statements in
a Function before creating every form control. The old worker rejected its AST
before controls existed; static operator ownership completes it at 4,053,180
accepted bytes under the same 4 MiB cap. No grammar or evaluator change is involved.

```sh
target/debug/mgbrowser http://127.0.0.1:7878/script-static-operators --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/static-operators-native
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/script-static-operators tmp/static-operators-cdp.png
```

Start the owned server/debugging browser as above; CI chooses a fresh debugger
port. All four CI blocks pass locally with 24 native/24 external CDP destinations,
including Unicode query/hidden fields, actual local first result, stale-node
rejection, flattened session and 1100×683 destination PNG. The submit button is
unnamed. The larger-body control still fails fatally before any form or later
script. Root and a reviewer inspected fresh frames; owned services were reaped.

Full debug/release totals are 1,145/1,032, excluding two repeated child summaries
per profile. Earlier fixture allocations in this document are historical
checkpoints, not promises of unchanged AST charges. The diagnostic worker now
accepts 58,734 after removing exactly 420 operator-buffer bytes from AST/total;
other phases remain fixed. See [STATIC_OPERATORS.md](STATIC_OPERATORS.md) for
frozen sources, measured clone/storage controls and all unchanged limits.

One post-change Google attempt still renders no result: homepage/real form work,
but search HTTP 200 reports unsupported Object.create property descriptors first,
then Ast378,301 rejected after 4,128,478 accepted against 4,194,304. Exit2 and an
inspected blank frame; no first result/destination or new required live stage.
No live source inspection/adaptation or retry. Publication is a separate gate.

## Object.create descriptor fixture

The frozen `/script-object-create` page requires descriptor defaults, typed
Symbol accessors and inherited getter/setter receivers before it creates any
controls. Its old worker completes no script and creates no form, reporting
unsupported descriptors at 69,723 accepted bytes. The unchanged candidate fixture
completes one script with no errors or rejected allocation at 75,451 bytes:
Bootstrap 26,880 + Source 1,861 + Ast 33,702 + FunctionCode 256 + Runtime 12,752.
These are different completed workloads, not a performance comparison. The
existing 4 MiB allocation cap and all execution/worker limits remain unchanged.

Start the owned fixture server and scripting-enabled debugging browser as above;
9222 is an example unused port, while CI chooses a fresh debugger port:

```sh
cargo test --locked --workspace --test js_object_create --test js_object_create_limits --test js_dom --test script_worker
target/debug/mgbrowser http://127.0.0.1:7878/script-object-create --enable-scripts --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/object-create-native
target/debug/examples/cdp_journey ws://127.0.0.1:9222/devtools/page/page-1 http://127.0.0.1:7878/script-object-create tmp/object-create-cdp.png
```

Both native application-handler and external CDP journeys submit the actual
Unicode query and hidden `source=fixture` control using the unnamed submit button,
click the first authored local result and reach the separate destination. Fresh
query/destination frames were inspected. All four CI journey blocks pass locally:
25 native and 25 external CDP journeys, including retained checks with exactly
two searches, two destinations and zero trap requests. Full debug passes 1,193
tests across 51 targets; selected release passes 1,080 across 40 targets, excluding
repeated owned-child summaries. These are local gates; exact remote publication
remains pending. See [OBJECT_CREATE.md](OBJECT_CREATE.md) for deliberate exclusions
and the public allocation guard above for independently reproducible storage checks.

One subsequent bounded Google attempt exits 2. Its HTTP 200 homepage has 26 items,
one form, five completed scripts/five errors and 2,383,102 startup bytes without
allocation rejection. Two retained activations complete and the actual form
submits. Search returns HTTP 200 but zero items/forms, three completed scripts and
two errors: first Ast 377,733 rejected after 4,135,770 accepted against the
4,194,304-byte cap, then the same latched failure. Unsupported descriptors are not
reported in this response, but changing inputs do not establish controlled
causality. The inspected search frame is blank: no result, destination or new
required live stage. There was no live-source inspection, adaptation, retry or
cap change. The ignored original log is `tmp/google-object-create-journey.log`,
SHA256 `007776cf479ee05eb13b673c61dcffacad0d560c5d2040188b454580971025e6`.
