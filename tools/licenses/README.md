# Upstream license text omitted from crate archives

Reviewed 2026-09-16. These are third-party texts, not Apache-2.0 project files.
`license-inventory.py` uses them only for explicitly listed versions whose
published manifest declares the matching license and whose archive omits it.
Other missing texts continue to fail packaging.

- `MPL-2.0.txt`: unmodified canonical license from
  https://www.mozilla.org/media/MPL/2.0/index.txt . The exact Servo/Stylo-related
  crate versions are listed in the inventory script. Their unmodified source
  archives are linked individually in the generated release notices.
- `void-1.0.2-MIT.txt`: upstream copyright and MIT terms from
  https://github.com/reem/rust-void/blob/ea5a2526d7a81ff45960525b62a3dbfd34c1703c/LICENSE-MIT .
  The 1.0.2 archive declares MIT in its manifest and README but omits the text.
  Upstream added this text when also offering Apache-2.0; we preserve the MIT
  choice declared by the locked release, not the later dual-license metadata.
