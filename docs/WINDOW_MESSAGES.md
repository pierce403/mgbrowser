# Bounded Window self-messaging

Reviewed implementation contract, 2026-10-22. Published in v0.9.10 at
`6ee10072217d6ccc91a67b31486b1e10cbeb5de6`; exact release/public acceptance
receipts are recorded in the dated work log, separate from authored tests.
v0.9.9 at `af5d8fc3a8f14555520c07946c0c56c355921a01` supplies queued Promise
notifications. Genuine Window messaging is another dependency toward
[complete Window-testharness readiness](WPT_TESTHARNESS.md), not its completion.
Keep the pinned corpus, scorer, expectations, protocol and resource policy fixed.

## Measured starting point

The verified public v0.9.9 executable was run once through its ordinary
`--profile --enable-scripts --no-auto-update` path against an owned loopback
server. It received the complete unchanged 198,291-byte pinned testharness.js,
with `setup({output:false})` and one authored synchronous assertion. An ordinary
tiny-script control completed first. The harness initialization failed with
`TypeError: Unsupported event type`, then the following inline source reported
`ReferenceError: setup is not defined`. Source review identifies the harness's
Window `message` listener registration as the first unsupported event family.

The initial harness reply reported 198,392 admitted source bytes, two source
admissions, 999,330 remaining opcodes, one admitted/pending task and no fatal
latch. Requested allocation was 2,201,761 outstanding / 6,952,278 peak /
18,246,916 cumulative bytes. These are an initial failed-bootstrap measurement,
not complete runtime feasibility, RSS, typed completion or an upstream pass.
The CLI does not await arbitrary later tasks or expose typed source provenance.
All three requests were exact owned inputs with HTTP 200; process/server cleanup
and unchanged executable/harness identities were verified. Ignored receipts:
`tmp/oct22-v099-window-harness-profile-a/report.json` and its raw logs; report
SHA-256 `1a8eb2c42fffe797d1b208d4efe6bdec7296b6ebf20c0a7eab4f80ecbd5c4867`.

The implemented candidate subsequently received those exact same three inputs
through its packaged executable. The complete harness now initializes without
the old registration diagnostic: two scripts complete, 9,007 opcodes are consumed
and no fatal state is reported. Initial requested-System allocation is
2,464,009 outstanding / 6,972,950 peak / 18,603,428 cumulative bytes. Three work
admissions leave two pending jobs; the CLI reports `Running page task` and does
not establish later completion. This is a non-scoring bootstrap measurement,
not a typed test result, full runtime-feasibility proof or public acceptance.
Exact input/binary identities, three HTTP 200 requests and owned cleanup pass;
receipt `tmp/oct22-v0910-window-harness-profile-a/report.json`, SHA-256
`9fc48aff04c79e8a0cdc8d9f343028ecdf9ce65e609b1ac65d511041bfb93137`.

## First supported surface, with explicit exclusions

Implement modern-only `Window.postMessage`, `message`/`messageerror` listener
families and function-valued `onmessage`/`onmessageerror` slots. Deliver real
later native events, not merely successful registrations. Keep the original
interpreter's event names and behavior unchanged.

Initially the only addressable target is the existing actual single Window.
`window`, `self`, `parent` and `top` describe that same browsing context; opener
is absent. Cross-tab, iframe, opener, Worker, MessagePort and BroadcastChannel
routing remain unsupported and need a separate parent-authority review.
Do not manufacture a remote target or use mutable aliases to supply authority.

The first serialization allowlist is primitives (including undefined, UTF-16
strings, BigInt, NaN and negative zero), ordinary objects and arrays. Preserve
cycles, repeated references, sparse array length/holes and own enumerable string
properties in specification order. Ignore symbol-keyed/nonenumerable properties;
reject symbol values, functions, Proxy, Promise, weak collections and host DOM
wrappers, including the actual global Window object, with a genuine
`DataCloneError` DOMException. Do not mistake the global's ordinary native brand
for a cloneable record; an ordinary object's ignored prototype may still be
that Window. Other cloneable builtins
such as boxed Boolean/Number/String/BigInt, Date, RegExp, Map/Set, buffers/views,
Error and DOMException are explicitly unsupported
in this increment, with a genuine `NotSupportedError`, never silent `{}` output.
Boxed Symbol, detached ArrayBuffer and SharedArrayBuffer without an isolated
agent capability are DataCloneError cases, not unsupported successful clones.
Valid nonempty transfer lists are explicitly unsupported and must detach
nothing. Still perform guarded sequence/object conversion and reject duplicate,
invalid or nontransferable entries correctly before reporting unsupported valid
transfers. Empty lists are not a reason to skip Web IDL conversion.
This is not complete structured-clone support and does not add a global
`structuredClone` function. Add real native DOMException branding/prototype and
the necessary standard name/message/code behavior, not an Error message prefix
or page-overridable constructor used for host exceptions. Preserve actual
ErrorData/Stack support as well as private DOMException branding; merely changing
an ordinary native object's prototype to Error.prototype is insufficient.
See [Web IDL exceptions](https://webidl.spec.whatwg.org/#idl-DOMException).

Follow the [Window messaging algorithm](https://html.spec.whatwg.org/multipage/web-messaging.html#posting-messages)
and [structured serialization](https://html.spec.whatwg.org/multipage/structured-data.html#structuredserializeinternal)
for the supported domain. Support both postMessage overloads, guarded Web IDL
argument conversion, default `/`, wildcard `*` and explicit parsed target
origins. Parse target origins as HTML specifies, not with the stricter HTTP(S)
navigation/userinfo admission rule: valid absolute URLs can include paths,
credentials or opaque/non-HTTP origins. Matching compares normalized origins,
not URL spelling; opaque/nonmatching origins silently discard delivery. Relative
or malformed targets other than the special `/` produce DOMException SyntaxError.
After Web IDL argument conversion, validate origin syntax, validate/reject a
nonempty transfer list, then admit work and serialize. Argument/syntax/unsupported
transfer failures do not consume a clone-work permit. Serialize
synchronously even with no listeners or an eventual target-origin mismatch.
Recheck the target origin only when delivering the queued task. Sender origin
comes from trusted document settings (`BrowserHost.url`), not a script URL,
`<base>`, location override, mutable alias or error-exposure flag. Foreign-script
error muting does not suppress a document's legitimate messages.

## Flat graph, admission and re-entry

Use an owned flat node arena with scalar IDs, not recursive reference-counted
edges. Cycles/aliases refer to IDs, so ordinary Drop releases the serialized
graph. A placeholder precedes following an object's edges. Strongly root source
identities throughout getters and GC; raw addresses alone are not a safe memo.
Strings and property keys use owned UTF-16 buffers, and BigInts use a sign plus
owned u32 magnitude limbs. Charge actual Vec capacity before copying, rather
than pretending that an immutable Rc clone allocated the original payload again.
An oversized serialized string/key therefore meets the existing host storage
limit through real owned data, not a new independent message-size allowance.
Keep every temporary key/value/root builder and queue capacity admitted through
the existing host/System accounting. No uncharged prewalk, prepaid allowance,
new independent limit or getter invoked twice is acceptable. The engine's
opaque native own-key scratch Vec is already pre-admitted by the production
System allocator, not by an invented Mg prewalk. Hold a work permit before that
call and charge its actual retained capacity to the host before adopting it as
Mg builder state. Its native work remains subject to existing CPU/active/System
containment, not a claim of complete cooperative opcode accounting.

Snapshot the initial enumerable own string keys before reading any values.
Then recheck own-property presence before Get, not later enumerability: making
a saved key nonenumerable does not remove it; deleting it skips even an inherited
getter; new or newly enumerable keys are excluded. A getter may change later
properties, post another message, collect garbage, navigate or exhaust a budget.
End all native/State/policy borrows before JavaScript; recheck guards after every
conversion/getter/iterator call. Use bounded iterative traversal rather than
unbounded native recursion, while preserving depth-first getter order: an outer
`a` getter, its returned child's `x`, then outer `b`, not breadth-first `a,b,x`.
Recreate properties using CreateDataProperty and
intrinsic allocation, never inherited setters, `__proto__` assignment or a
page-replaced constructor. Restore all object identities before filling edges;
represent sparse arrays by length plus properties, never a length-sized vector.

Obtain one existing shared WorkPermit before retaining clone roots/storage.
A failed attempt releases pending admission, never lifetime admission. Allocate
the actual queue handle and queue-time monotonic deadline only after successful
serialization: an inner message posted by an outer getter must queue first.
One queued message retains one permit and one admitted serialized graph. A
delivery starts that permit before effects; target mismatch still consumes its
task. No JS values, serialized message body or callback handles cross the wire.

Use the existing common task sequence and strict v4 TaskWake/TaskPump exchange,
one in-flight transaction and one selected task plus its real checkpoint. Keep
early/stale/replayed-token, generation/revision, projection, native-edit and
independent navigation checks. No child sleeping, parent JS, unsolicited output,
restart/replay or budget renewal. Cancellation/fatal/close/teardown clears queued
graphs, source roots, permits and wakes outside held policy borrows. Preserve
the documented fatal-VM-stack limitation and actual terminal Context teardown.

## Delivered events and independent privacy policies

MessageEvent payloads expose actual cloned `data`, immutable sender `origin`,
actual Window `source`, empty `lastEventId` and a frozen empty ports array.
Native wrappers trace delivered JS values; Rust-only event records and weak
wrapper caches must not become hidden roots. Keep existing listener ordering,
removal, capture/target phases and escaped-event identity. Message events are
non-bubbling and noncancelable: neither preventDefault nor handler false may
change defaultPrevented. Callback failures retain the existing error diagnostics.
Ordinary deserialization failures dispatch a real messageerror event; fatal
resource or internal-invariant failures remain terminal, never a clean
messageerror recovery. Serialization errors instead throw synchronously and
queue no event. A private fallible-restoration control may verify messageerror
without manufacturing a page-visible trigger. MessageEvent/Event constructors
and synthetic event dispatch are not added or claimed by this increment.

Extending `dom_tasks` requires narrowing rejection-privacy purging to rejection
records only. A falling error/rejection exposure floor may purge local rejection
notices but must not discard queued messages or change their document origin.
Existing sticky incomplete-observation diagnostics remain unchanged.

## Reuse review

The pinned Boa engine archive identifies official source
`337a3668a0dc86dd401ea20906e782249a64a228` (tag v0.22). Runtime clone/store are
re-exports of boa_wintertc; neither package is in the locked production graph.
Review the exact source, not mixed-version `latest` documentation or a fork.

Direct adoption is rejected for this increment. The official
[store](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/wintertc/src/store/mod.rs)
uses recursive strong Arc edges and deduplicates transfer lists. Its
[serializer](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/wintertc/src/store/from.rs)
detaches during traversal, lacks source identity roots, ignores array getters
and permits unsupported exotic objects through ordinary fallback. Its
[restoration](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/wintertc/src/store/to.rs)
uses ordinary setters and loses trailing holes/view geometry. Runtime's
[sender seam](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/runtime/src/message/mod.rs)
is after serialization, too late for our pre-retention work admission. These are
source-review reasons not to reuse it unchanged, not claims from runtime tests
or a general audit of upstream Boa. No dependency or source pin changes follow.

## Frozen authored acceptance before engine changes

Keep new controls separate from the upstream selection. Establish old-public
failures first, then test the same inputs against the implementation:

1. Genuine asynchronous self/reply delivery, listener/slot ordering, target,
   receiver, phases, noncancelability, escaped event/data and frozen ports.
2. Default/wildcard/normalized explicit origins, mismatch, malformed origins,
   forged aliases/base/location, wrong receiver and hostile overload conversion.
   No-listener/mismatch cases must still serialize and retain normal failures.
3. Mutation isolation, cycles/aliases, sparse arrays, Unicode/BigInt/NaN/-0,
   descriptor/key order, deletion/nonenumerability during getters, prototype
   setter traps and explicit unsupported/noncloneable/transfer failures.
4. Getter re-entry and GC with rooted identities, inner-before-outer queue order,
   fatal interruption, no post-fatal effects and admitted storage reclamation.
5. Mixed Promise/timer/message 256-pending and 2,048-lifetime exhaustion,
   failed/canceled admission, IDs/storage failures, navigation/terminal cleanup
   and all existing exact resource assertions without weakened caps or guards.
6. Rejection privacy purging preserves messages and their trusted origin; actual
   message callback failures retain diagnostics and traced payloads release.
7. Real restricted-worker transactions, stale/early/replayed pumps, one combined
   projection charge, pending native edits and a message-created retained form
   completed through unchanged native and external CDP journeys.

Remeasure the complete unchanged harness through the real worker after this
increment. Bootstrap and initial allocation numbers remain diagnostics, not
typed completion or WPT passes. Full owned positive/negative harness controls
and a trusted bounded vendor observer still require their own schema review.
Run all old debug/release, isolation, component, package/public install and
actual prior-binary updater gates. Freeze the unchanged full static WPT cohort
and freshly review every case/page/request/cleanup/passing-script result before
any contribution. Finish user-visible messaging through a new verified release.

## Historical next milestone after v0.9.10: queued lifecycle

Keep bundled Init unchanged in this increment. Mandatory DOMContentLoaded and
complete/load tasks would change existing Init semantic/counter assertions,
including exact zero/2,048-admission controls. Do not add free tasks, hidden
extra allowances, relaxed resource tests or an implicit compatibility mode.
Review genuine lifecycle scheduling and explicit migration of approximation-only
semantic expectations separately, retaining all quota/fatal/security assertions.

In the v0.9.10 baseline, Chassis ends loading at Init and CDP emits DCL/load
together. The separately frozen and implemented v0.9.11 candidate follows
[DOCUMENT_LIFECYCLE.md](DOCUMENT_LIFECYCLE.md), with release gates recorded in
the dated log. The original next-step requirements were:

Proper
task-separated lifecycle needs monotonic generation-fenced progress, a reviewed
strict modern protocol/schema decision, correct CDP events and a finite initial
load wait in profile/static hosts. Never redefine loading as draining all timers
or silently alter scored waits. Full streaming parsing, dynamic load blockers,
typed harness completion and general cross-context messaging remain unfinished.
