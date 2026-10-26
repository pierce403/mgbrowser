# Automatic attachment and initial-target startup

Pre-implementation contract, 2026-10-26. No behavior in this document is shipped
by v0.9.21. It is one prerequisite of the next meaningful Playwright increment:
ordinary connection, actual utility-world title and initialized blank-page
creation/closure. It is not a replacement for the complete unchanged F-024 probe,
F-011's full protocol inventory or real Reddit/Google News acceptance.

## Source and compatibility baseline

The unchanged playwright-core 1.58.2 client against the public v0.9.21 binary
fails both HTTP-discovery and browser-WebSocket connections. Both logs retain
`Target.setAutoAttach` and parallel `Browser.setDownloadBehavior` errors. No
title, locator, navigation, screenshot or client-event assertion ran. See the
[retained baseline](PLAYWRIGHT.md#observed-public-v0921-baseline-2026-10-26).

The upstream reference is the official protocol schema pinned at
[`7245ec551bc9d450170193456ab3965bb7e716cb`](https://github.com/ChromeDevTools/devtools-protocol/tree/7245ec551bc9d450170193456ab3965bb7e716cb/json).
Its Target autoattachment and Runtime resume commands inform the subset below;
they do not establish implementation or a full-schema compatibility inventory.
The ownership, capacity and pre-navigation barrier choices below are Mg design
decisions, not claims that Chrome uses the same implementation.

## Automatic sessions and identity

- Browser-root `Target.setAutoAttach` operates on real top-level page targets.
  Page-session configuration concerns related descendants, not its own page or
  sibling tabs. Mg currently exposes no separate iframe/worker targets; accepting
  that subscription does not claim iframe/worker execution or discovery support.
- Require explicit booleans `autoAttach` and `waitForDebuggerOnStart` and
  `flatten:true`. Reject unsupported filters, non-flat sessions, extra fields and
  invalid routes. Preserve the existing client/session/target and queue bounds.
- Catch-up attachment never adds a startup lease to an already-admitted page.
  Running pages report `waitingForDebugger:false`; if another client's lease
  still holds that target, report its actual waiting state without granting the
  new subscriber authority over that lease. Never retroactively pause work.
  Emit actual attachment events before the configuration acknowledgement, and a new page's
  attachment before its `createTarget` reply. A page's recursive subscription
  must not create duplicate attachments to itself.
- Distinguish manual from automatic sessions. Repeated identical configuration
  is idempotent. Disabling autoattachment detaches only that configuration's
  automatic children, not manual sessions or other clients' sessions.
  Changing the wait flag affects only future target admissions, not existing
  leases. Explicitly detaching an automatic child does not cause an identical
  configuration request to reattach it. Revoking a parent route recursively
  removes its configuration, automatic descendants and their leases.
- A stable nonempty default `browserContextId` identifies the actual shared
  native network/cookie context. It is not context creation, incognito storage or
  an owner-provided arbitrary string. Keep explicit context creation unsupported.
  Attachment/detachment events include the actual immutable target identity;
  native moves never change it. Old target IDs remain unreusable.

## Real startup barrier

Desktop currently starts navigation inside tab insertion, and Browser navigation
starts its fetch thread before polling. A later polling flag is therefore not a
startup barrier. Reserve the automatic sessions and their waiting leases before
admitting either a protocol-created or native-created page and before navigation.
Embeddings without this pre-admission host capability reject waiting
configuration explicitly; observing a later roster cannot promise startup control.

A held target owns a real empty Browser and at most its original bounded URL.
It launches no HTTP/resource fetch, script child, Init, callback or task. Do not
suspend a process with OS signals, adjust clocks, preload a hidden document or
return a fabricated loaded context. Existing pages remain usable.

Each waiting lease binds the client, parent route, child session and immutable
target. Multiple clients can hold the same newly admitted target; the initial
navigation begins only after every lease is resumed or revoked. A client cannot
release another client's lease through a direct page, foreign or stale session.

`Runtime.runIfWaitingForDebugger` works before `Runtime.enable` and without a
loaded script realm. It removes only the caller's lease. Repeating it on an
already-resumed valid session is harmless. It never enables page scripting,
creates synthetic contexts or renews any worker allowance. After the final
lease leaves, start the still-current original navigation exactly once using
current effective script permission; blank initialization stays genuinely empty.

While held, reject additional navigation, reload, input and execution explicitly.
Do not queue them or return a navigation reply tied to generation0 and later
start generation1. Ordinary pending-navigation identity and completion rules
remain intact. This subset does not pause arbitrary already-running documents.

## Capacity, failure and teardown

- Preflight a complete catch-up attachment set against the existing eight
  sessions/client. Failed configuration has no partially published attachments.
  Reserve required automatic sessions before admitting future pages. Reject new
  page admission when reservations cannot fit; report the same capacity error to
  native UI. Never run an unexpectedly unpaused target or disconnect another
  client just to fit it. Keep four clients/sixteen tabs/four windows unchanged.
- Disconnect, explicit detach or configuration disable revokes only its leases.
  Reconcile delivery failures too: sending to a slow client may disconnect it.
  Once the last lease leaves, surviving pages resume normally. Turning debugging
  off or losing the broker revokes all debugger holds, without enabling scripts.
- Closing a held target discards its initial URL without a fetch. Native moves,
  activation and redocking retain its holds. A queued stale command cannot
  recreate a revoked session or resume a replacement target.
- Keep the native mutation handoff single-use and single-flight. Actual close
  teardown still precedes its acknowledgement. No next command is drained with
  stale borrowed pages. Preserve the final-tab-close restriction.
- Before spawn there is no worker lifetime to renew. After spawn all existing
  cumulative source/work/System/transaction/wire/time bounds and real reaping
  rules apply unchanged. Waiting is not an allowance for a replacement worker.

## Additive acceptance before adoption

Retain missing-feature failures before production changes. Require:

1. Existing-target catch-up/order, recursive scope, real context identity,
   manual/automatic separation, idempotence and all malformed/unsupported cases.
2. A counting owned server and script runtime: a held initial URL produces zero
   requests and zero starts; the last authorized resume produces exactly one.
   A real script/Promise/timer sentinel stays unexecuted before resume and works
   afterward. Scripts-disabled resume never fetches/executes page sources.
3. Two clients, independent leases, capacity failure before target admission,
   disable/re-enable, close while held and disconnect before/after handoff.
   Test slow-client revocation, native moves and stale queued commands too.
4. Actual packaged/public native/CDP witnesses: A stays usable while B waits;
   no B request before resume, exactly one afterward; closing held B fetches
   nothing. Inspect real images, exact requests, resource ledgers and cleanup.

Do not release an autoattachment-only result as the completed connection/title
increment. Its remaining real download policy, utility-world/disabled-script
authority, promise/activation and page-initialization requirements are recorded
in [PLAYWRIGHT.md](PLAYWRIGHT.md#next-meaningful-increment). Implement and verify
them before that completion claim. Preserve the untouched full probe and every
later failure, old tests, both complete WPT lanes and manual ratchets. No new
transport/resource profile, syscall authority or dependency is adopted here.
