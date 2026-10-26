# Native target mutation foundation

Pre-implementation contract, 2026-10-26. This is a bounded prerequisite for
F-011/F-024, not complete Target support or ordinary Playwright compatibility.
The all-features goal separately includes real Reddit and Google News reading
and visual acceptance in [SITE_COMPATIBILITY.md](SITE_COMPATIBILITY.md).

## Required behavior

- Browser-root `Target.createTarget` accepts only a required `url`: empty or
  `about:blank` creates a genuinely empty document, not the Mg welcome document;
  HTTP(S) creates a real tab and starts the ordinary checked navigation only
  after workspace admission. Return its actual permanent `page-N` identity.
  Other options, contexts, windows, geometry and background creation reject.
- Browser-root `Target.closeTarget` closes exactly the named native tab and its
  actual Browser/worker. Reject closing the last tab in this subset, rather than
  queue a success response that process exit would discard. Return `success:true`
  only after removal and ordinary teardown. Stale/foreign IDs reject.
- Browser-root `Target.activateTarget` selects the actual tab/pane/window and
  requests native focus. It neither navigates nor creates/replaces a Browser.
- Reject direct-page and flattened-session mutation commands before any action.
  Preserve sixteen tabs, four windows, eight sessions/client, four connections,
  all transport queues and all existing worker authority/resource limits.

## Host handoff and ordering

Chassis borrows pages and never owns native tabs. A host-enabled tick validates
one command and stops draining immediately at its mutation boundary. At most one
host command is pending globally. A typed single-use request is handed to Desktop
only while its originating client still exists. A client lost before handoff
causes no mutation; after handoff an admitted native action may complete, with
no reply to a disconnected client. Do not replay it or retarget a replacement.

Desktop reuses its real new-tab construction, fonts, independent script service,
shared network session/preferences and transactional workspace ownership. On
completion Chassis validates and publishes the resulting full live-page roster,
performs existing pending-navigation/Runtime revocation and direct-socket closure,
then replies using the actual outcome. Later queued commands run only on a fresh
roster. Keep the initial `page-1` alias and never recycle identities after closure
or native detach/redock. An embedding without the host handoff rejects mutation
commands explicitly; it must not leave clients waiting indefinitely.

## Acceptance and remaining work

Before production implementation, retain failing additive assertions for real
create/activate/close handoff. Cover malformed/unsupported options, route
authority, missing/stale IDs, final-tab and capacity rejection, failed creation,
disconnect-before-handoff, single-flight ordering and invalid host outcomes.
Exercise a supplied packaged/public binary: create and navigate B, retain A's
state/worker, identify B in the native strip, activate both, move B without
identity loss, close B, reject stale routes/discovery and prove A still works.
Inspect actual captured images and verify owned process cleanup.

Automatic discovery events, automatic attachment, debugger waiting/resume,
download policy, browser contexts, promise awaiting, utility worlds and the
unchanged full pinned Playwright journey remain separate requirements. Never
acknowledge their commands as no-ops. Preserve all existing tests, both full WPT
lanes and manual ratchets. Normal release/package/site/public-install/update
acceptance is required before calling this increment shipped.
