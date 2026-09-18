# Desktop inspection, workspaces and Google News

Requested 2026-09-17. These are separate acceptance gates, not a claim that the
current browser already supports them. Retain Rust-only implementation backends,
verified HTTPS, restricted page execution and existing regression assertions.

## Native inspection and diagnostics: T-019 / F-022

First increment: right-click a painted page element, choose Inspect element,
and open a Rust-native read-only inspector. F12 / Ctrl+Shift+I also open it.
Show the actual selected DOM identity, ancestors, attributes, text and painted
bounds. A separate Diagnostics section retains bounded stylesheet parse errors,
unsupported computed layout, resource failures and script execution failures.
Attribute source/line/column/node where available; report when entries are omitted.
Do not label a rendering fallback as supported CSS or discard all error detail
behind a count. Navigation invalidates stale selection and clears prior-page errors.

The first UI is an overlay, not the Chrome DevTools frontend. It has no debugger,
console evaluator, DOM/CSS editing, network waterfall or complete CSS audit.
No remote code is evaluated in the parent to supply an inspector feature.
Acceptance includes right-click selection, panel input isolation, scrolling,
navigation invalidation, light/dark and compact/scaled rendering, real CSS parse
locations and unchanged page pixels when closed. Preserve no-chrome embedding.

## Live tab workspace: T-020 / F-023

Proposed interaction: ordinary tab groups that detach into native windows and
dock into left/right groups within a window. At most two visible groups per window,
16 tabs and four windows initially. The user was asked whether side-by-side pages
or desktop-managed window tiling was intended; this is the provisional former
interpretation, not vertical tab strips or arbitrary nested tiling.

One registry owns each Browser exactly once. Moving a tab moves that same live
object, retaining its form edits, history, scroll, document and isolated realm.
Each tab has its own LinuxScripts service. The host owns shared preferences,
bookmarks and one updater. Window creation failure and canceled drags cannot lose
a tab. Closing a loading/scripted tab cancels/reaps its work; other tabs survive.
Restart reopens committed URLs with fresh sessions in one window, explicitly
discarding pane placement, unsaved forms, cookies and history.

Acceptance: new/switch/close/reorder, Ctrl+T/W/Tab, detach/redock and left/right
drop previews through native input; state preservation; correct focused-pane
input at 100/125/200%; transactional failures and caps; final-window exit only.
CDP must stay bound to stable tab identities, never silently follow the active tab.
An initial host integration may explicitly refuse additional tabs with remote
debugging enabled until multi-target routing is implemented. That is not full
completion of the tab/automation integration gate.

## Pinned Playwright acceptance: T-021 / F-024

Use unmodified Playwright APIs and a pinned client, initially playwright-core
1.58.2. Record protocol traffic against owned local fixtures. The present CDP
server is not compatible: HTTP attachment also needs the trailing-slash discovery
path, followed by real auto-attach/target lifecycles, frame lifecycle and Runtime
contexts/handles. Locator evaluation needs the DOM APIs used by the injected
script, not merely successful initialization replies.

Required journey: connect, enumerate/create/select pages, navigate, read title,
locate/fill/click an actual form, follow its result, capture a screenshot, receive
page/console/resource errors, and close a page without retargeting another.
Missing protocol behavior must return useful errors. Broad compatibility remains
unverified until this exact client journey passes against the packaged binary.
Do not execute injected/page JavaScript outside the restricted worker or rewrite
Playwright assertions to treat an unsupported method as success.

## Google News desktop reading: T-022 / F-025

First target: signed-out, scripts-disabled desktop reading at 1280x800 and
1024x768. Use identical actual HTML/assets in Mg and a reference browser, then
fresh live navigation. Require recognizable header/briefing/story groups,
readable metadata and thumbnails, no overlapping critical content, aligned
scroll/link input, one served topic/story navigation and Back. News changes;
do not hardcode headlines, invent content or impersonate another browser.
Consent/challenge responses, if encountered, are a reported boundary.

Baseline 2026-09-17: ordinary HTTPS returns HTTP 200 and server-rendered stories.
Actual Mg load: about 1.85 MB HTML, 987 nodes, 32 links, nine small admitted
stylesheets, zero images. The main inline sheet is 1,227,493 bytes, rejected by
the existing 256 KiB per-sheet cap. The first screen has overlapping menus and
expanded weather that pushes stories down. Main HTML's 8 MiB limit is sufficient.
One same-origin thumbnail redirects to gstatic and is JPEG: the current resource
origin policy and PNG/GIF/SVG-only decoders both reject it.

Prerequisites need deliberate review: a bounded large-stylesheet profile; generic
flex/grid/position/overflow layout; cross-origin subresource and cookie policy;
Rust-only JPEG; inline SVG/icon/clip details. Taffy is a layout candidate, not an
adopted dependency. Keep pure-Rust source/feature/license review and malformed-
input/resource tests before adoption. Do not simply remove limits to fit a site.

Search, account actions, personalization and full interactive Google News are
later gates. Current script-document admission is 1 MiB and external scripts,
fetch/XHR, timers and broad browser APIs remain absent. Static first-screen
acceptance does not complete JSPLAN, Google search or the formal Linux MVP.
