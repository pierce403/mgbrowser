# Scripted link defaults without a text projection

2026-10-23: pre-implementation behavior and acceptance contract for the next
bounded browser increment, after the fully delivered v0.9.17 and its exact
`7fb7d2e33d9670777966cf4fe5bb4d6a30d28a80` documentation CI/public closeout.
This contract is not implementation, a test result or a delivery claim.
F-003/F-004/F-008/F-016/F-026 and the formal MVP remain unfinished.

## Behavior and boundary

Repair the existing parent refusal of painted empty, whitespace-only and
non-text anchors when their retained script reply proposes following the
clicked anchor. Use that exact stable node's own present href in the validated
post-handler DOM, resolved with the candidate Document's reconstructed base.
Present empty href remains distinct from absence. Text removal, hiding,
reparenting or detach/reinsert during the handler need not prevent activation
when the same node finally remains connected with href. A replacement anchor
with the same HTML id is not the same arena identity.

Sparkle provides a policy-neutral exact-anchor URL-data method reusing its
existing trim/join/absolute resolver; Chassis retains connectedness and URL
policy. No arbitrary ancestor/sibling acceptance, new network capability,
source loading, synthetic click API or layout/CDP/Boa/worker production rewrite.

Preserve the current worker's suppression after cancellation, permanent
detachment, href removal, fatal/closed execution or explicit script navigation.
Valid Ready canceled/suppressed replies may commit DOM edits with no default.
Forged FollowLink replies with unrelated/stale/non-anchor/disconnected identity,
missing or unresolvable href, invalid graph or invalid envelope reject the whole
projection. Preserve receiver/event/listener/checkpoint semantics; a later timer
does not delay activation. The existing connected-anchor requirement is Mg's
bounded policy, not full HTML behavior for detached anchors.

The HTML Standard's [anchor activation and hyperlink resolution](https://html.spec.whatwg.org/multipage/links.html#links-created-by-a-and-area-elements)
supports using the element's href and document rather than rendered text.
This increment does not implement all its download, target, referrer or
disconnected-anchor behavior.

## Selected navigation and transaction

Stage an owned checked default destination before accepting DOM/acks/revision/
epoch/observer publication. Reuse `checked_navigation`: resolved serialized URL
at most 16,384 UTF-8 bytes, credential-free HTTP(S). No URL lookup after commit.
Only execute after the existing one active-time charge and accepted Ready
projection. A selected forbidden target or failed charge retains the previous
page, native edit versions and observer publication; issue no request.

Preserve explicit-script-navigation precedence and its existing hop limit.
Even with explicit navigation, validate default identity/connectedness/present
resolvable own href. An eligible but suppressed default receives no final URL
policy check: an unselected resolvable ftp/credentialed/overlong href cannot
override or newly reject an otherwise valid explicit HTTP(S) destination.
Invalid explicit navigation rejects rather than falling back to a link.
Preserve typed `click_canceled: None` and `Some(false)` acceptance; a required
false field or mutual-exclusion rule would be separate protocol hardening.
Some(true), terminal state and task-pump defaults keep their current rejection.

Keep all generation/session/sequence/revision, ack, stable-tag/append-only arena,
graph, task/lifecycle, accounting and stale-geometry checks. Forms are unchanged.
Keep the existing raw-attribute, node/depth/aggregate, wire, opcode/source/work,
System, CPU/active/lifetime/transaction/child limits. No new pre-join raw href or
base cap: apply the existing target limit to the resolved selected URL. Existing
input/storage limits are not an independent parent URL-parser allocation quota.

## Additive acceptance, frozen before production

Use current APIs and literal expected values for assertion-only old-production
RED on the targeted old failures; preservation controls may already pass.
Missing new-method compilation is not RED. Do not edit old expectations.

1. Exact URL/identity: text, empty, whitespace and non-text anchors; relative,
   absolute, trimmed and empty href; post-handler href/base, reordered/removed/
   detached bases; same-node moves versus replacement/detachment/href removal.
   Include long admitted trim input and long serialized base with short absolute
   destination, and exact/over 16,384 serialized target plus UTF-8 expansion.
2. Parent receipt atomicity: otherwise-valid replies use the existing fake
   session helpers, seeded activation and real accounting/ack shape. Verify old
   nodes/title, edit values/versions/focus, revision/epoch/address/generation,
   no new observer rows/DocumentUpdated/request, exactly one charge/cancel for
   successfully received rejected frames, and no double charge on another poll.
   Transport Err retains zero projection charges; accepted Ready/None does not
   cancel. This is receipt handling, not worker input.
3. Precedence and suppression: preserve both typed cancellation outcomes,
   explicit selected navigation and eligible suppressed links; reject invalid
   explicit URLs and forged defaults. Legitimate None after cancellation/
   detach/href removal commits valid edits without navigating.
4. Actual execution: fresh restricted child plus retained Browser/loopback HTTP
   tests require real direct/Promise handler changes, cancellation/property-false
   versus listener-false, exception/fatal and explicit-navigation behavior.
   Observe actual destination/body and zero trap requests, not just CDP success.
5. Separate owned native and external-CDP proof at 1x/2x uses a new authored
   empty-anchor fixture, literal paint/input geometry and handler-selected
   distinct destinations; retain resources/lifecycle/HTTP/cleanup and inspect
   actual images. Repeat packaged and public-installed bytes.

Preserve all existing tests, thirteen native helpers, four additive proofs,
both frozen worker suites, Rust/embedding/dependency/JSPLAN gates and both full
independent WPT lanes/ratchets. Authored controls add no upstream passes or
denominator. Complete a new normal versioned release, exact-source green CI/site,
checksum public install/reinstall, actual prior-release upgrade/no-op and public
repeats. Keep v0.9.17 and its assets immutable. Full DOM/CSSOM/WPT/Playwright,
container-unit grammar, Google/Reddit and broader feature/MVP gates stay open.
