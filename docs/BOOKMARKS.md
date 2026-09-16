# Navigation and bookmarks

The toolbar orders Menu, Back, Forward, Refresh and Bookmark before the address
field. Back/Forward are disabled when no matching history entry exists. Alt+Left
and Alt+Right navigate history; Ctrl+R/F5 refreshes the loaded page, not an edited
but unsubmitted URL. Ctrl+L selects the URL: the highlight follows the visible
text width, leaving the unused field background alone.

Click the star or press Ctrl+D to save/remove the **loaded** page. A marked star
indicates a saved URL. Menu > Bookmarks or Ctrl+Shift+O opens the local list.
Click a title to open it, Remove to delete it, or Previous/Next for more entries.
Tab/arrows cycle dialog controls; Enter/Space activates; Escape closes. These
dialogs block clicks and typing into the page behind them.

The Linux host writes $XDG_CONFIG_HOME/mgbrowser/bookmarks.json, or
~/.config/mgbrowser/bookmarks.json. No server, account, telemetry or sync.
At most 128 HTTP(S) URLs are stored, without embedded username/password fields;
URLs are bounded to 8191 bytes and displayed titles to 128 characters. URLs
including query strings are private local data, not encrypted secrets. Do not
bookmark sensitive token-bearing URLs. There are no folders, rename, import,
export or per-site settings yet. Cookies remain memory-only.

Writes use a 0600 temporary file and atomic replacement. A nonblocking lock
merges each change with the latest file, avoiding lost edits between windows.
Opening the list reloads it. Another window's toolbar may remain stale until
then or its next bookmark edit. Read/write/lock errors appear in the status bar
and dialog; malformed or unsupported files are preserved, never silently reset.
Fix the file or save location and retry. Removing the file clears saved entries;
an already-open window refreshes when opening the list.

Chassis owns values and UI only; the platform host owns storage. No new crate,
native toolkit or web-engine behavior is introduced. The existing packaged
scale smoke test also exercises native toolbar navigation, text-only selection,
bookmark add/open/remove, restart and Ctrl+D using private XDG roots.
