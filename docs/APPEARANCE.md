# Browser appearance

Open **Menu > Settings** and select **System**, **Light** or **Dark**.
Changes apply immediately. System is the default and follows the desktop's
light/dark preference while the browser is open. Explicit Light/Dark overrides
ignore subsequent system changes until System is selected again.

This themes the browser toolbar, address field, status bar, menus and dialogs.
It does not force websites into dark mode or change page CSS/media queries.
Plain HTTP retains the red title/address strip and the "HTTP: Not secure" label
in both themes. The desktop window manager controls outer decorations; Mg sends
the conventional `_GTK_THEME_VARIANT` hint, which not every manager honors.
This hint does not load GTK or introduce a native toolkit dependency.

## Saved preference

The small JSON file is `$XDG_CONFIG_HOME/mgbrowser/settings.json`, or
`~/.config/mgbrowser/settings.json` when XDG_CONFIG_HOME is unset or relative:

```json
{"theme":"system"}
```

Accepted values are `system`, `light` and `dark`. Missing files use System.
Invalid/oversized files produce an understandable error and use System.
Saving creates parent directories and atomically replaces this file. If saving
fails, the choice still applies to the current window and Settings reports the
failure. No sudo, shell command or background service is installed. Separate
open windows load their preferences independently; the last saved choice wins
on the next launch. Remove this file to restore the default.

## Linux desktop integration

The host reads `org.freedesktop.appearance` / `color-scheme` from the
[XDG desktop Settings portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html):
1 means dark, 2 means light, and 0 means no preference. Missing/unsupported portal,
unknown values or failed reads fall back to Light. A desktop must expose this
standard preference to follow it automatically; arbitrary GTK/Qt theme names,
accent colors, wallpaper and custom desktop skins are not copied.

Discovery runs off the UI thread with bounded method calls, rechecking roughly
every two seconds. The pinned Rust zbus client talks directly to the session
bus; no GTK, Qt or libdbus implementation is linked. This facility stays in the
desktop host and is not available to pages or restricted JavaScript workers.
The component libraries accept an explicit color scheme from their embedding
host and never read the user's desktop or settings themselves.

## Verification

Chassis tests cover palettes, selection and keyboard/pointer modal behavior,
HTTP warning retention and unchanged page/no-chrome pixels. Host tests cover
bounded persistence, XDG paths, malformed files and save failures. The native
`tools/theme-smoke.sh` acceptance test uses an isolated session bus and temporary
configuration to exercise the packaged executable, explicit overrides, live
system changes, restart persistence and the window-manager hint. It does not
change the real desktop's preference.
