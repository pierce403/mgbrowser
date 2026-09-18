# Native inspection

The v0.8.0 candidate adds a read-only inspector built into mgbrowser. It does not
require a remote-debugging port or enable page JavaScript.

Right-click visible page content and choose **Inspect element**. **F12** or
**Ctrl+Shift+I** toggles the panel; **Escape** closes it. The Elements section
shows the selected parser-DOM node, ancestors, attributes, text and the union
of its painted bounds. Coordinates are viewport-relative CSS pixels, not a
complete computed CSS box model. Navigation or script-driven document replacement
invalidates stale selection rather than silently selecting a new node.

The Diagnostics section shows retained resource and script failures plus errors
from the actual stylesheet/render pass. CSS parsing reports source, line and
column where available; unsupported layout reports the affected node. Oversized
stylesheets include their byte size and admission limit. Long entries are bounded
and omission counts are visible. Scroll or use arrow/Page Up/Page Down keys to
read the panel: its input does not scroll, type into or navigate the underlying page.

These diagnostics are intentionally partial. No reported error is not proof that
a page is compatible. There is no JavaScript evaluator/debugger, console capture,
DOM/CSS editing, network waterfall or Chrome DevTools frontend. Missing layout
behavior remains missing even when Stylo accepts the property. Scripting stays
disabled by default; enabling it does not make the browser safe for hostile pages.

## Acceptance

Build `inspector_smoke`, then run:

```sh
bash tools/inspector-smoke.sh /absolute/path/to/packaged/mgbrowser
```

The helper uses its own Xvfb, ephemeral local fixture server and temporary HOME/
XDG directories. It tests right-click selection, Elements/Diagnostics, real CSS
and restricted-worker errors, panel input isolation and fresh navigation at
100% and 200% in light/dark themes. Inspect its saved PNGs as well as assertions.
Repeat with the public-installed executable before marking the feature shipped.
No-chrome embedding must remain unaffected.
