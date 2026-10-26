# Reddit and Google News completion targets

The user explicitly added **reddit.com and news.google.com should load and look
as expected** to the active all-features goal on 2026-10-25. This is an acceptance
target, not a claim of current compatibility. The Runtime foundation shipped
as v0.9.20; its separate documentation closeout is verified at
`c30570b4f9654adf168343d1474e76d89b92a426`. The next bounded automation increment
does not replace or complete these real-site acceptance targets.

## First milestone: faithful signed-out desktop reading

Google News retains the user's reading-first, interactions-later choice in
[DESKTOP_NEXT.md](DESKTOP_NEXT.md) and F-025. Reddit's equivalent feed/thread
reading target is F-027. Use the real modern sites, ordinary networking and
actual returned content; do not substitute old.reddit.com, fabricated pages,
site-specific drawings, a consent/challenge response or readable error fallback.

At 1280x800 and 1024x768, capture the actual HTML/CSS/assets and compare identical
inputs in Mg and a reference browser with matched scale, fonts and charset.
Record script mode, viewport and scrollbar policy. Inspect the first screen and
scrolled reading content, keeping the reference and Mg screenshots and resource/
CSS/script diagnostics. A reference capture is diagnostic evidence, not a claim
that the current live page or interactions work.

Acceptance requires:

- Reddit: recognizable navigation, feed/story cards and media; an actual thread
  with readable post/comments and its expected hierarchy.
- Google News: recognizable header, briefing, story groups, readable metadata
  and real thumbnails; preserve explicit destination and placeholder failures
  until fixed.
- Both: expected layout, colors, typography and images without critical overlap,
  missing primary content or unreadable text. Record remaining differences;
  unresolved material visual gaps keep the milestone open.
- Separately verify fresh live loads, aligned scrolling, a real served reading
  link and Back in the packaged and public-installed release. Inspect actual
  images. HTTP 200, CDP attachment, a local fixture or a static snapshot alone is
  insufficient.

Start each increment from fresh actual failures and preserve reproducible
regressions. Release bounded fixes through the standing package/CI/site/public
installer policy; keep both upstream WPT lanes and authored tests distinct.
Do not hide failures, hardcode changing headlines, weaken TLS, impersonate a
different browser, bypass site access controls or raise resource/security limits
without their separate review. A blocked live request remains a reported blocker.

Account actions, personalization, Reddit posting/voting and full interactive
Google News remain later work. Reading success does not complete full web
compatibility, Playwright, JavaScript, WPT or the formal MVP.
