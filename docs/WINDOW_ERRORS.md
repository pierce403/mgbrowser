# Next browser increment: bounded Window error reporting

Reviewed contract and frozen authored acceptance plan, 2026-10-22.
The v0.9.8 implementation follows this policy; release/public verification
receipts are recorded separately in the dated work log.
This follows the shipped v0.9.7 timer increment in
[Window-testharness readiness](WPT_TESTHARNESS.md). It does not change the pinned
WPT selection, expectations, scoring, reporter protocol or resource profile.
No new WPT pass, complete Window readiness or general event-loop support is
claimed. Promise rejection delivery, messaging and typed harness observation
remain subsequent increments.

## Source trust before page-visible exceptions

The parent resource broker must supply bounded, typed classic-source provenance:
the effective response URL and whether the entire accepted redirect chain stayed
same-origin with the document. Compute this in transport, where each hop is
checked, not from the final URL alone. A foreign hop followed by a return to the
document origin remains muted. Preserve provenance through repeated-source cache
hits and deferred execution. Page HTML, `src`, DOM attributes, rewritten globals
and listener-registration sites cannot supply or upgrade this authority.

The worker must validate metadata shape, URL bounds and consistency before
retaining it or parsing JavaScript. Missing metadata, unknown provenance and
contradictory same-origin claims must never authorize exposure. Metadata and
repeated entries count in existing encoded-wire and retained host-storage
admissions; source-text accounting still measures source text, not URLs.
No larger request, host-storage or source cap is authorized.
The exact typed request change requires review before coding. Existing callers
without external-source provenance retain readable execution, with errors from
those realms conservatively muted. Inline-only realms do not require external
metadata.

Version the modern session as v4 for the provenance-aware Init contract; preserve
the explicit legacy v1 lane and its wire assertions. `Request` is shared, so
envelope version alone is insufficient: omit the new optional metadata from the
explicit legacy serializer/lane, including requests with external sources.
A new modern reader may accept absent optional provenance as unknown, but an old
v3 reader using `deny_unknown_fields` rejects a newly populated field. Do not describe that as
bidirectional wire compatibility. The one-shot request stays explicitly
unversioned: new readers may accept the old form, and old binaries must reject
new metadata before execution. Same-inode worker re-execution avoids production
mixed-version workers; it does not make the schemas interchangeable. Reject old
v3 session frames rather than reinterpreting them under modern v4 semantics.

Pinned Boa 0.22 exposes source paths and active frames, but its raw execution API
returns after unwinding. A callback's source or registration location cannot prove
the origin of an exception thrown by a nested foreign function, bound function,
Proxy or dynamically compiled source. Do not infer the throw site from those
values, read `Error.stack` or patch upstream just to obtain a favorable result.

For this first increment, use a documented sticky realm-wide exposure floor:
precompute it across all initial admitted source metadata before compiling or
executing any JavaScript. A foreign/unknown accepted source makes all
page-visible errors opaque for the rest of that realm, including syntax errors,
earlier inline sources, later callbacks, timers, nested calls and jobs. Never
clear it after execution, cancellation, navigation proposals or a subsequent
same-origin source. A new realm starts its
own policy; an old realm is not reused to regain exposure. This intentionally
over-mutes some local errors rather than exposing foreign exception objects.
Only a realm with proven same-origin code may expose the actual thrown value.

For this first policy, dynamically compiled strings are conservatively unknown
too. A monotonic worker-local scalar in the existing Butane compilation hook
must mark that fact before admission/parsing; consult it before every exposure,
including errors thrown after a nested `eval`/`Function` call. It never resets
on a later transaction. This deliberately over-mutes even locally constructed
dynamic code. Do not add source-path registries that pretend to recover a throw
site or use a callback/job tag to remove the floor.

Unknown source filename and line/column remain empty string and zero, even for
an unmuted error. Do not fabricate a throw location from the active callback.
Muted errors expose only `Script error.`, empty filename, zero line/column and
null error, following the privacy shape in the
[HTML runtime-error algorithm](https://html.spec.whatwg.org/multipage/webappapis.html#runtime-script-errors).
This conservative attribution is not complete interoperable source reporting.

## Dispatch and retention

Use the existing worker-local raw exception seam, without converting fatal
limits into JavaScript events. Never call coercions, getters, `toString`,
`try_native` or script-controlled message/name/stack properties to describe a
thrown object. Keep the existing bounded data-only host diagnostic even when a
page cancels its error event; cancellation cannot erase execution failure from
future harness arbitration or turn a fatal result into success.

Deliver a genuine cancelable, non-bubbling Window `error` event. Its actual
exception value belongs in a GC-traced worker-owned event wrapper, not the
Rust-only original evaluator event record, an untraced map or a permanent root.
Escaped event references retain their value; unreachable events can release it.
Muted wrappers contain only the constant public payload and null error. Release
the unexposed foreign exception before dispatch rather than retaining it in a
hidden record; the bounded data-only diagnostic does not require that root.
Reuse the cumulative 32-listener and 256-event bounds, preserving listener order,
removal, propagation/cancellation and post-dispatch target state.

Function-valued Window `onerror` receives message, filename, line, column and
error. Only literal true cancels through that handler return; ordinary listener
return values do not cancel. Existing click/submit false-return semantics stay
unchanged. A reporting-mode guard suppresses recursive error dispatch; a thrown
error-handler exception still produces a bounded host diagnostic. Fatal policy
or engine failure prevents all further JavaScript callbacks and job execution;
nonexecuting root/kept-object cleanup remains required. Release native
borrows before any JavaScript re-entry. No new task queue, worker syscall,
automatic restart, wire result channel or renewable budget is introduced.

## Frozen authored controls before engine changes

Implement these in existing modern page/event/timer, resource and real restricted
worker harnesses. First record their behavior on released v0.9.7; unsupported
delivery is an honest failing baseline, not an upstream WPT result. Add the new
tests before implementing their behavior; do not weaken old resource assertions.

1. Inline and same-origin external throws: primitive/object identity, actual
   Window target/currentTarget/phase, message bounds and honest unknown location.
2. Window `onerror`: five arguments, literal-true cancellation, false/other-value
   non-cancellation, listener return-value independence and preserved slot order.
3. Retained native click, lifecycle callback and real timer failures: report once,
   continue eligible ordinary listeners and preserve task/checkpoint ordering.
4. Hostile thrown objects with getter/coercion traps: no trap runs during native
   error extraction. Throwing error handlers do not recurse indefinitely or hide
   the original diagnostic. Fatal loops/admission failure execute no reporter JS.
5. Same-origin, foreign HTTPS, same-to-foreign, foreign-to-same and
   same-to-foreign-to-same response chains; repeated cached/deferred sources,
   missing metadata, malformed metadata and forged DOM provenance. Verify actual
   cookie/TLS/redirect restrictions remain unchanged. Unknown/foreign admission
   mutes all later local callbacks; DOM changes cannot unmute it. An earlier
   inline throw is already opaque when a later admitted foreign/unknown deferred
   source exists, even before that deferred source executes.
6. A local callback calling an exported foreign thrower, bound/Proxy callbacks,
   successful/failed dynamic compilation and microtask-to-timer transitions
   cannot launder provenance. Never expose foreign object identity or secret
   message fields. A caught successful/failed `eval`/`Function` attempt followed
   by a later local object throw stays opaque. A real new realm after teardown
   may expose its own local object again; a navigation proposal cannot reset the
   old realm's policy.
7. Real GC/checkpoint controls: escaped ErrorEvent keeps actual exception alive;
   unreachable event/value can be collected. Retaining a muted ErrorEvent must
   not keep its foreign thrown object alive after ordinary checkpoint/GC.
   Shared-work, listener/event, source,
   allocation, opcode, transaction/wire and fatal teardown bounds remain intact.
8. Existing startup, deferred sources, retained forms, hidden-tab tasks,
   cancellation/default/navigation guards and all legacy AST/allocation/resource
   regressions continue to pass. Preserve native and external CDP acceptance.
   `Promise.reject` alone does not become a Window `error`; preserve existing
   native-job failure queue behavior without inventing rejection delivery.

Review the exact new tests and typed metadata before adoption. Preserve ordinary
native-job failure queue semantics; Promise rejection notification is not part
of this increment. Existing task/page-services smoke checkers reject script
diagnostics: use an error-specific check requiring the exact expected retained
diagnostic, not cancellation, deletion or blanket relaxation of those checks.
Run debug/release and packaged/public gates using the existing
infrastructure, then publish a feature release. The v0.9.7 archive is only 20,969
bytes below the unchanged 8 MiB updater cap: measure the next actual package;
do not silently enlarge that cap or claim an installer-only success proves
compatibility with already installed updaters.

## Local implementation evidence

The final authored error suite has 18 controls, including a regression showing
that capture-phase stopPropagation permits the remaining capture listeners but
suppresses the later non-capture invocation. Final focused debug passes 240;
the complete optimized workspace passes 1,787 with zero failures/two explicitly
ignored updater gates. Existing original AST/allocation assertions are retained.
Independent no-chrome embedding and the locked JSPLAN comparison also pass.

The old public v0.9.7 binary fails the new native fixture honestly; the current
production payload passes native and external CDP error/checkpoint/retained-form
acceptance with exactly one expected diagnostic per journey and zero trap
requests. The existing page/task checkers stay unchanged. Fresh complete WPT
remains 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011; every page pixel hash and
request multiset is unchanged and passing scripted pages have no pending work.
These controls do not complete full Window testharness readiness. Exact-commit,
tagged-public and old-binary upgrade receipts belong in the
[dated work log](../memory/logs/2026-10-22.md).
