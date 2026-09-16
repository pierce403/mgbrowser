# Updates and build identity

The website installer enables automatic updates by placing
`.mgbrowser-auto-update` beside the executable. Installed browsers check GitHub's
latest published, non-prerelease release on startup and every 24 hours while open.
Checks run off the window thread. A failed check leaves the current binary intact;
the next automatic retry is in 24 hours or at the next launch.

Checks use GitHub's unauthenticated public API and can receive HTTP 403 when its
shared-IP request quota is exhausted. This is an update-check failure, not a
failed browser launch. Wait for GitHub's quota reset before checking again; do
not repeatedly retry. The website's checksum-verifying curl installer uses the
public release-download links and remains an alternative when that API quota
is exhausted. No GitHub token is required or stored by the browser.

Use Menu > Check for updates or `mgbrowser --update` for a manual check/install.
Use `--no-auto-update` or `MGBROWSER_NO_AUTO_UPDATE=1` to disable automatic checks
for a launch. Delete the marker to disable them persistently. Installing with
`MGBROWSER_NO_AUTO_UPDATE=1` does not create the marker. Source builds without the
marker, worker entrypoints and CDP/smoke runs do not check automatically.

Menu > About mgbrowser and `mgbrowser --about` report the running build's version,
compile timestamp in GMT/UTC and source commit. The timestamp is generated during
the Cargo build, not at launch; `SOURCE_DATE_EPOCH` overrides it for reproducible
builds. An updated open window continues showing its old build until restarted.

## Trust and failure behavior

- The host queries only pierce403/mgbrowser releases. Tags must be stable numeric
  major.minor.patch versions; draft/prerelease tags, equal versions and downgrades
  are not installed. Download URLs are constructed from the validated tag, not
  supplied by web content. Current on-disk version is rechecked under a file lock.
- Metadata and downloads use the existing certificate-verifying Rust transport
  with HTTPS required on every redirect, no browser cookie session, and the same
  time/header/8 MiB compressed-body limits. Archives are bounded to 64 MiB unpacked.
  An oversized release fails closed and can be installed with the public installer.
- SHA-256 is checked before extracting the exact regular executable member into
  a newly created sibling file. Archive paths/links/devices are never extracted.
  The candidate must report the expected version and pass the existing restricted
  worker selftest before atomic rename replaces a user-owned executable. Failure
  before rename preserves the existing executable. No sudo or shell is used.
- Checksums are corruption checks, not independently signed releases. This trusts
  GitHub, the repository release pipeline and TLS. There is no updater sandbox,
  signing infrastructure or claim of production-grade supply-chain security.
- Linux file locking serializes updates; the old inode remains usable by already
  running processes. Workers re-exec `/proc/self/exe` so they stay on that process's
  build after replacement. The UI shows `Menu *` when a new build was installed.
  Restart manually: updates never discard the current page or force a relaunch.
- The updater replaces the executable only. The existing desktop launcher/icon
  still targets it. Re-run install.sh if launcher/icon/license material needs to
  be refreshed. Remove the marker and `.mgbrowser-update.lock` when uninstalling.

This is a browser-host capability, not a page API. Chassis receives build/status
strings and emits a host update request; it does not install programs itself.
