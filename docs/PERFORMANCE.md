# Performance tools and cache resource review

The native inspector's **Performance** section opens with Ctrl+Shift+P, or F12
followed by the Performance tab. `mgbrowser --profile URL` prints the same load
and render measurements without opening a window. It uses the normal browser,
resource loader and restricted script service. A failed navigation returns a
nonzero exit status and retains its measured fetch duration.

The CLI paints the page, attempts a scroll and requests a UI redraw. Each row
labels the cache path actually used. A page that fits the viewport reports no
scroll movement. These are measurements of this invocation, not predictions for
all pages, hardware, display sizes or later interactions.

## What is measured

- HTML fetch, parsing, CSS/font/image resources, external script preparation and
  restricted-worker startup have separate durations. Slow resource rows record
  actual request durations/statuses.
- The last completed Browser paint includes style/layout/page painting, snapshot
  admission, geometry, page composition and browser controls/inspector drawing.
  The visible inspector describes the previous completed paint, avoiding a
  measurement that recursively changes while drawing itself.
- Cache hits distinguish complete Frame reuse from computed-style reuse.
  Shaping lookups/hits/misses/evictions and retained payload describe the Fonts
  lifetime, including retired downloaded faces in cumulative counters.
- Worker allocation reports explicitly identify unavailable accounting. Reported
  outstanding/cumulative requests are the existing System-allocation counters,
  not GC heap, RSS or total browser memory.

## Reviewed persistent-cache policy: v0.9.6

These limits bound retained payload per Browser independently of existing work,
scene, image, network and worker budgets:

| Cache | Admission | Oversized behavior |
| --- | --- | --- |
| Frame snapshot | 16 MiB including pixel/vector/string/diagnostic payload | Render normally; do not retain the snapshot |
| Computed styles | 8 MiB including style/vector/string/diagnostic payload | Compute and render normally; do not retain styles |
| Shaped text, per face | Existing 4,096 entries / 1 MiB text and glyph payload | Existing bounded eviction/bypass |

Frames use shared immutable snapshots; a chrome repaint copies the page into
the output once, rather than deep-cloning the cached page first. Standalone
no-chrome rendering returns its owned canvas without retaining a Frame clone.
Styles share immutable storage across scrolling layouts. Existing raster glyph
caches retain their separate 4,096-entry/8 MiB per-face bounds.

Payload accounting is not an allocator or RSS ceiling. Multiple regular/bold and
downloaded faces have separate font caches. At the existing 16-tab limit,
Frame/style retention alone can total 384 MiB. No whole-browser quota is claimed;
renderer/network isolation and comprehensive resource accounting remain open.
No existing transport, worker, layout-work, scene or image ceiling is increased.

Cache keys track document/projection identity, viewport, scale and controls.
Tests compare cached and fresh pixels/hit geometry and cover document/control,
viewport/scale and no-chrome invalidation, retained-payload bypass, shaping
equivalence/eviction and accounting after downloaded faces are cleared.

Native acceptance extends `tools/inspector-smoke.sh`: Performance must open by
shortcut and tabs, show selected controls and body text, isolate input, resize
compactly, and render at 100/200% in light/dark themes. Inspect the saved images.
`tools/page-services-smoke.py PATH_TO_BINARY` also checks short/tall CLI pages and
a malformed response using owned loopback fixtures and the supplied executable.
Release and public-binary receipts are recorded in the dated work log.
