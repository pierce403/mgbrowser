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

## Browser size

**Menu > Settings > Size** defaults to **System**. It scales both the browser
controls and page content. Use the minus/plus buttons or **Ctrl+minus/plus**
(Ctrl+equals also works) to choose 75, 100, 125, 150, 175, 200, 250 or 300 percent.
**Ctrl+0** or the Size **System** button restores the desktop preference.
Changes apply immediately and are saved; explicit sizes ignore later DPI changes.
This is a whole-browser size override, not independent page-only/per-site zoom.

System reads XSETTINGS `Xft/DPI` first (DPI times 1024), then the X11 root
`RESOURCE_MANAGER` resource `Xft.dpi`. Effective size is DPI divided by 96:
192 DPI selects 200%. The two signals are alternatives, never multiplied.
Only 75..300% is accepted; missing/invalid desktop values use 100%. The host
rechecks roughly every two seconds. This follows the screen-global X11 setting,
not per-monitor Wayland scale. It does not infer readable size from resolution
or physical monitor dimensions. See the [XSETTINGS registry](https://wiki.freedesktop.org/www/Specifications/XSettingsRegistry/).

The existing Rust X11 connection reads these bounded properties directly, with
no Xft library, toolkit, shell helper or native font binding. Text is shaped in
logical pixels and rasterized at the physical size, so enlarged letters stay
sharp. Page layout and hit regions use logical pixels; the host converts physical
mouse positions. Images use their existing bounded decoded resolution. Window
surfaces are limited to 4800x3600 physical pixels; internal raster rounding can
add up to three pixels, clipped on upload. Window managers may enforce scaled
minimum sizes. Desktop title bars remain controlled by the window manager.

## Saved preferences

The small JSON file is `$XDG_CONFIG_HOME/mgbrowser/settings.json`, or
`~/.config/mgbrowser/settings.json` when XDG_CONFIG_HOME is unset or relative:

```json
{"theme":"system","scale":"system"}
```

Theme values are `system`, `light` and `dark`. Scale is `system` or a supported
integer percentage, for example `200`. Existing theme-only files keep their
theme and default to System size. Missing files use both System defaults.
Invalid/oversized files produce an understandable error and use the defaults.
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

Display sizing adds bounded DPI/parser and preference-migration tests, physical
text/image/composition checks, logical hit geometry and CDP screenshot checks.
`tools/scale-smoke.sh PATH_TO_PACKAGED_BINARY` uses its own 4K Xvfb display and
temporary settings. It exercises native form navigation at 100/125/200%, the
existing CDP journey at fractional/2x size, live Xresources/XSETTINGS priority,
manual overrides, keyboard shortcuts, restart persistence, scroll and resize.
It never changes the user's desktop DPI or preferences.
