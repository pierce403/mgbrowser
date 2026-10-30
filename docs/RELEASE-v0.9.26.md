# mgbrowser v0.9.26 Experimental Preview

Desktop browser UX parity with Chromium: Omnibox search & live autocomplete
dropdown, `chrome://` internal pages (`newtab`, `history`, `bookmarks`,
`downloads`, `settings`, `version`), Find in Page (`Ctrl+F` / `F3` /
`Shift+F3`), persistent user-local Browsing History (`Ctrl+H`) & Clear Browsing
Data (`Ctrl+Shift+Delete`), pure-Rust File Downloads (`Content-Disposition:
attachment`, `<a download>`, `Save link as...`, `Ctrl+S` Save Page, `Ctrl+J`),
toggleable Bookmarks Bar (`Ctrl+Shift+B`), Hard Reload (`Ctrl+Shift+R`), Stop
Loading (`Escape` / toolbar `×`), `Ctrl+U` View Page Source, `Ctrl+Shift+T`
Reopen Closed Tab, `Ctrl+N` New Window, `Ctrl+Shift+W` Close Window,
`Ctrl+1..9` tab switching, middle-click background link opening & tab closing,
tab-strip right-click context menu, mouse buttons 8/9 Back/Forward, and `F11`
EWMH fullscreen. Exact-source publication and public-install receipts are
recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies and pure-Rust implementation backends (zero new crate dependencies).

## Desktop Browser UX & Chrome Parity (`v0.9.26`)

- **Omnibox Search, URL Normalization & Live Autocomplete (`crates/mg-chassis/src/omnibox.rs`)**:
  - Smart input classification distinguishes explicit `http(s)://` and
    `chrome://` / `about:` schemes, `localhost` / IPv4 / dotted hostnames
    (defaulting bare domains to `https://` and loopback to `http://`), and
    free-text search queries routed to the user's configured Search Engine
    (`DuckDuckGo`, `Google`, or `Wikipedia`, persisted in `settings.json`).
  - Live Omnibox autocomplete dropdown (`Up` / `Down` / `Enter` / pointer click,
    up to 6 suggestions) surfaces direct URL/Search actions, matching
    `chrome://` internal pages, saved Bookmarks (`★`), and visited Browsing
    History (`⏱`).
- **`chrome://` Internal Pages**:
  - Built-in styled HTML pages rendered natively through `mg-sparkle` with safe
    internal-only `chrome://` link navigation: `chrome://newtab` (search hero,
    pinned bookmarks, recent history), `chrome://history`, `chrome://bookmarks`,
    `chrome://downloads`, `chrome://settings`, and `chrome://version` (plus
    `about:blank`).
- **Find in Page (`crates/mg-chassis/src/find.rs`)**:
  - `Ctrl+F` opens the floating Find bar with live case-insensitive substring
    matching across positioned `Frame::text_runs`, live `N/M` match counter,
    translucent yellow (`#ffe066`) inactive and orange (`#ff922b`) active match
    highlights, automatic viewport scroll-into-view, `F3` / `Enter` / `▼` next
    match, `Shift+F3` / `Shift+Enter` / `▲` previous match, full clipboard
    support (`Focus::FindInput`), and `Escape` / `×` dismissal.
- **Persistent Browsing History & Clear Browsing Data (`crates/mg-chassis/src/history.rs`, `src/history.rs`)**:
  - User-local `$XDG_CONFIG_HOME/mgbrowser/history.json` (`O_NOFOLLOW`, `0o600`,
    advisory `flock` `.history.lock`, `2 MiB` read cap, `512` deduplicated
    entries sorted newest-first with visit counts and timestamps).
  - `Ctrl+H` and `Menu > History` open the paginated History modal with click
    navigation and per-item `Remove`; `Ctrl+Shift+Delete` clears browsing
    history, session cookies, HTTP/render caches, and the download list.
- **Pure-Rust File Downloads & Save Page (`crates/mg-chassis/src/downloads.rs`, `src/downloads.rs`)**:
  - RFC 6266 / RFC 8187 `Content-Disposition: attachment` and binary MIME
    detection (`application/pdf`, `application/zip`, `application/gzip`, etc.),
    `<a download="...">` link activation, context-menu `Save link as...`, and
    `Ctrl+S` / context-menu `Save page as...` stream downloads without navigated
    page loss.
  - Safe host persistence into `MGBROWSER_DOWNLOAD_DIR` / `XDG_DOWNLOAD_DIR` /
    `~/Downloads` using sanitized filenames, `O_NOFOLLOW`, `create_new(true)`,
    `0o600` mode, and automatic `(1)`, `(2)` collision numbering (`8 MiB` per
    download cap), inspected via `Ctrl+J` / `Menu > Downloads` /
    `chrome://downloads`.
- **Bookmarks Bar, Toolbar Reload/Stop & Page Source**:
  - Toggleable Bookmarks Bar (`Ctrl+Shift+B` or `Menu > Settings > Bar`) renders
    one-click saved bookmark pills directly beneath the main toolbar.
  - Active page loads turn the toolbar reload button into Stop (`×`, or `Escape`
    to cancel in-flight navigation); `Ctrl+Shift+R` / `Shift+F5` performs a
    cache-bypassing Hard Reload (`Cache-Control: no-cache`, `Pragma: no-cache`).
  - `Ctrl+U` and context-menu `View page source` open the raw HTML document in
    DevTools `Sources`.
- **Tabs, Windows, Pointer & Fullscreen Shortcuts (`src/desktop.rs`, `src/workspace.rs`)**:
  - `Ctrl+Shift+T` reopens closed tabs from a 32-entry LIFO stack; `Ctrl+N`
    opens a new native browser window; `Ctrl+Shift+W` closes the current window;
    `Ctrl+1..8` selects tabs 1–8 and `Ctrl+9` jumps to the last tab.
  - Middle-clicking (`Button 2`) a page link opens it in a background tab
    without stealing focus; middle-clicking a tab in the tab strip closes it;
    right-clicking a tab opens a native Tab Context Menu (`New tab`,
    `Duplicate tab`, `Reload`, `Close tab`, `Close other tabs`,
    `Reopen closed tab`); X11 mouse buttons `8` and `9` navigate Back and
    Forward; `F11` toggles EWMH `_NET_WM_STATE_FULLSCREEN`.
