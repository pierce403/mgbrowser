# mgbrowser v0.9.25 Experimental Preview

App-wide pure-Rust X11 clipboard (`CLIPBOARD` and `PRIMARY`), mouse drag/double-click
text selection, and Cut / Copy / Paste / Select All across the DevTools Console,
all six DevTools panels and search/style inputs, the Address bar, HTML `<input>`
form controls (with `<input type="password">` plaintext protection), and read-only
web page content. Exact-source publication and public-install receipts are
recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies and pure-Rust implementation backends (zero new crate dependencies).

## Native X11 Clipboard & App-Wide Text Selection

`mg-browser`, `mg-chassis`, and `mg-sparkle` implement pure-Rust ICCCM X11
selection ownership and retrieval (`x11rb`) paired with interactive text
selection, editing, and context-menu clipboard actions across every browser
surface:

- **Pure-Rust X11 `CLIPBOARD` & `PRIMARY` Host (`src/platform/clipboard.rs`)**:
  uses a dedicated unmapped `1x1` child window of the root screen (`EventMask::PROPERTY_CHANGE`)
  so selection ownership survives closing detached browser windows without any C
  toolkit or external helper utility (`xclip`/`xsel`). Serves `TARGETS`,
  `TIMESTAMP`, `UTF8_STRING`, `text/plain;charset=utf-8`, `text/plain`, `TEXT`,
  and `STRING` (`SelectionRequest` / `SelectionClear`), and retrieves inbound
  `CLIPBOARD` (`Ctrl+V` / `Shift+Insert` / context-menu `Paste`) and `PRIMARY`
  (middle-click `Button 2`) selections with `UTF8_STRING` -> `STRING` fallback,
  bounded `INCR` transfers (`256 KiB`), and `0 ms` local short-circuit when
  `mgbrowser` owns the active selection.
- **DevTools Console & Panels (`crates/mg-chassis/src/devtools.rs`)**:
  - **Console Logs & Uncaught JS Errors**: click or click-and-drag across Console
    rows (`Ctrl+A` selects all visible rows), then press `Ctrl+C`, click the
    subtoolbar `Copy` button, or right-click (`Copy message`, `Copy all visible`,
    `Select all`, `Clear console`) to copy full untruncated error messages, stack
    frames, and expanded object property trees. Console Command Line API
    `copy(...)` also syncs directly to the X11 system `CLIPBOARD`.
  - **Console `>` REPL & DevTools Filter/Style Inputs**: click/drag/double-click
    selection, `Shift` + arrow/word (`Ctrl+Left`/`Right`) range extension,
    `Ctrl+A`, `Ctrl+X` / `Shift+Delete` (Cut), `Ctrl+C` / `Ctrl+Insert` (Copy),
    `Ctrl+V` / `Shift+Insert` (Paste), middle-click `PRIMARY` paste, and
    right-click `Cut` / `Copy` / `Paste` / `Select all`.
  - **Elements, Sources, Network, Performance, Application & About**: `Ctrl+C`
    and right-click context menus copy `outerHTML`, unique CSS selectors, or
    computed style summaries in `Elements`; selected code lines or full source in
    `Sources`; request URL, full request/response details, or all captured URLs
    in `Network`; structured telemetry reports in `Performance` and `Application`;
    and build/revision identity on the `About` dialog.
- **Address Bar (`Focus::Address`)**: click-to-select-all on initial focus from a
  page, second-click caret placement, click-and-drag substring selection (updating
  `PRIMARY`) with `2 px` caret and highlight rendering, double-click select-all,
  word navigation, `Ctrl+A` / `Ctrl+X` / `Ctrl+C` / `Ctrl+V`, middle-click
  `PRIMARY` paste, and right-click `Cut` / `Copy` / `Paste` / `Paste and go` /
  `Select all`.
- **HTML `<input>` Form Controls & Password Protection (`Focus::Input`)**:
  positioned caret + partial selection highlight rendering in `mg-sparkle`,
  click/drag/double-click word selection, `Ctrl+A` / `Ctrl+X` / `Ctrl+C` /
  `Ctrl+V` (updating `edit_versions` for isolated script-worker form snapshots),
  middle-click `PRIMARY` paste, and strict `<input type="password">` protection
  that blocks `Cut`, `Copy`, and `PRIMARY` drag export of masked password
  characters while permitting `Paste`.
- **Web Page Prose Selection (`Focus::Page`)**: `Frame::text_runs` records
  positioned glyph runs (`MAX_TEXT_RUNS = 25,000`) for click-and-drag text
  selection with translucent highlight rendering, double-click word selection,
  `Ctrl+A` select-all, `Ctrl+C` copy in reading order, `PRIMARY` export on drag
  or double-click, and right-click `Copy` (when a selection is active) plus
  `Copy link address` on links. Cut and Paste remain disabled on read-only page
  prose.

## Validation status

Workspace debug and release unit/integration tests (including new app-wide
clipboard/selection unit tests and live two-client X11 `CLIPBOARD`/`PRIMARY`
roundtrip tests under Xvfb), component boundary and no-chrome embedding checks,
packaged native X11 smoke gates, all 8 frozen JSPLAN gates, and both complete
reviewed WPT lanes pass.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest published release and verifies SHA-256 before
installing without sudo. Checksums are not independent signatures. Existing
processes must restart to use an installed update.

Modern-web compatibility is poor. The Google search to first result journey is
not working. Real Reddit loading and full interactive Google News features
remain unfinished goals. Ordinary Playwright is unsupported: autoattachment,
debugger waiting/resume, downloads, utility worlds, promise awaiting and stock
locator message-size policy remain open. Clipboard transfers support plain UTF-8
and Latin-1 text up to `256 KiB` on X11/XWayland; rich HTML/image clipboard MIME
types, JavaScript `navigator.clipboard` / `ClipboardEvent` DOM APIs, `<textarea>`
or `contenteditable` editing, breakpoint stepping, heap snapshots, and persistent
Web Storage remain outside this bounded increment. This release does not complete
F-011/F-016/F-024/F-025/F-026/F-027 or formal MVP.
