# Dependency/license sanity check

Current decision: the user's subsequent 2026-09-08 instruction selects Apache-2.0
for project-authored code, documentation and original Mg artwork. LICENSE is the
unmodified official Apache License 2.0 text; Cargo metadata and NOTICE identify
the current license and copyright. No font files are bundled.

The initial v0.1.0/v0.1.1 releases used the user's earlier MIT fallback. Their
published tags/archives remain unchanged and retain that license. Current source
and future release packages use Apache-2.0. Dependencies retain their own licenses.

`tools/license-inventory.py` reads the locked Cargo metadata with the Linux x86_64
platform filter and the active normal/build cargo tree, fails on missing declarations or unreviewed missing license texts, and
packages upstream LICENSE/COPYING/NOTICE material. DEPENDENCY_LICENSES.md is the
generated basic inventory; full texts accompany the downloadable binary.

The v0.1 graph had permissive choices: predominantly MIT/Apache-2.0,
with BSD, ISC, Zlib, Unicode-3.0 and CDLA-Permissive-2.0 among the remainder.
Inactive optional entries such as ring appear in raw metadata but are excluded
using the active tree; the existing native dependency guard still applies.
Preserve notices for Unicode alongside MIT/Apache. rustls-rustcrypto is pinned to the existing Git
revision; no dependency upgrades or native-code fallback were introduced.

## v0.3.0 additions

The standalone Stylo family adds MPL-2.0, a file-level copyleft license: it is not
relabeled Apache-2.0. These crates are consumed unmodified from pinned registry
archives. The bundled notices now identify each registry version's downloadable
source archive and preserve its declared license. Project-authored integration
files remain Apache-2.0. See the [MPL terms](https://www.mozilla.org/en-US/MPL/2.0/)
and [official FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).

Ten exact MPL crate versions and void 1.0.2 omit the license text in their
published archives. The inventory uses a narrow name/version/license allowlist
and tracked upstream texts from `tools/licenses/`, with provenance recorded
there; any other missing text still fails packaging. The void fallback preserves
its MIT copyright notice and terms. The resvg/usvg/tiny-skia image graph retains
its upstream permissive licenses; no fonts or website artwork are packaged.

The regenerated locked Linux inventory was inspected for missing declarations
and obvious redistribution problems. This is a limited sanity check, not a
comprehensive legal audit or a replacement for upstream license terms.
