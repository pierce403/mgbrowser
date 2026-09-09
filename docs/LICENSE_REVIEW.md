# v0.1 dependency/license sanity check

Current decision: the user's subsequent 2026-09-08 instruction selects Apache-2.0
for project-authored code, documentation and original Mg artwork. LICENSE is the
unmodified official Apache License 2.0 text; Cargo metadata and NOTICE identify
the current license and copyright. No font files are bundled.

The initial v0.1.0/v0.1.1 releases used the user's earlier MIT fallback. Their
published tags/archives remain unchanged and retain that license. Current source
and future release packages use Apache-2.0. Dependency licenses are unchanged.

`tools/license-inventory.py` reads the locked Cargo metadata with the Linux x86_64
platform filter and the active normal/build cargo tree, fails on missing declarations or missing license texts, and
packages upstream LICENSE/COPYING/NOTICE material. DEPENDENCY_LICENSES.md is the
generated basic inventory; full texts accompany the downloadable binary.

Inspected declarations have permissive choices: predominantly MIT/Apache-2.0,
with BSD, ISC, Zlib, Unicode-3.0 and CDLA-Permissive-2.0 among the remainder.
Inactive optional entries such as ring appear in raw metadata but are excluded
using the active tree; the existing native dependency guard still applies.
Preserve notices for Unicode alongside MIT/Apache. rustls-rustcrypto is pinned to the existing Git
revision; no dependency upgrades or native-code fallback were introduced.

No missing declaration, mandatory strong-copyleft dependency, or obvious binary
redistribution blocker was found in this limited release sanity check. This is
not a comprehensive legal audit or a replacement for upstream license terms.
