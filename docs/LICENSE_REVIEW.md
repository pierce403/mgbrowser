# v0.1 dependency/license sanity check

Repository history and memory/notes/decisions.md contained no adopted project
license. The user's 2026-09-08 release instruction explicitly selects MIT in that
case. LICENSE and Cargo metadata record that decision; original Mg artwork uses
the same license. No font files are bundled.

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
