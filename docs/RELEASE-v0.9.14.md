# mgbrowser v0.9.14 Experimental Preview

v0.9.14 adds parser-backed modern element-style declarations, using the existing
locked Rust Stylo parser and serializer. Full local Rust regressions and the
frozen JSPLAN comparison pass. The reviewed Window parsing measurement records
15/16 passing files and 77/79 passing actual final subtests.
Publication, public installation/update and native/CDP verification are separate
gates; consult the [dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-23.md) for their actual status.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Modern element-style declaration bridge

- Modern `element.style` supports declared-value named reads/writes, `cssText`
  and callable `getPropertyValue`. Values are parsed and serialized by the same
  enabled Stylo declaration grammar used by Sparkle, not a fixture-specific
  property table or computed-style result.
- The actual DOM style attribute remains authoritative. Raw
  `setAttribute('style', ...)` text stays raw until a successful style mutation;
  reads do not rewrite it. External attribute changes, removal and detached
  elements remain coherent without a second retained declaration store.
- CSS-name lookup folds non-custom ASCII case but does not apply named
  camelCase conversion. Named accessors support enabled dashed/camelCase names
  and `cssFloat`; custom properties can be set through `cssText` and queried
  through `getPropertyValue` with case preserved.
- Empty single-property values remove declarations, invalid single values leave
  them unchanged, and valid writes replace both value and importance. Generic
  shorthand updates are atomic. `cssText` replaces the complete parsed list,
  retains valid siblings and creates an explicit empty style attribute even
  when its replacement is empty or wholly invalid.
- Native receiver checks precede arity and callback-capable string conversion.
  Forged, inherited and author-Proxy receivers are rejected without argument
  coercion. Conversion reentry observes the current attribute; ordinary Host
  errors remain catchable, while existing fatal execution failures remain
  terminal. Original-engine raw-style behavior is unchanged.

The [scoped contract](https://github.com/pierce403/mgbrowser/blob/v0.9.14/docs/CSSOM.md) records conversion details, resource admission,
authored controls and explicit exclusions. Relative URL declaration syntax does
not add fetch authority. No dependency, enabled feature, worker syscall,
execution profile or resource allowance is added.

## Local evidence and separate delivery records

Focused local debug validation passes nine declaration controls, twenty public
binding controls, nine private quota/fatal controls and three fresh restricted-
worker controls. A real retained-worker Init/lifecycle/click control also passes.
The separate frozen JSPLAN comparison retains all 364 reviewed outcomes.
These authored checks are not upstream WPT results or public-binary acceptance.
Full local debug/release all-target regressions, optional-UX embedding and the
existing tooling checks pass, retaining the original two explicit manual
updater-test ignores.

The complete local measurement of the unchanged sixteen-file upstream Window
parsing cohort is independently reviewed: fifteen files pass, with 77 passing
actual final subtests out of 79. Two real assertion failures remain in
`flex-valid.html`. Review covers the lossless final inventories, all 64 original
resource requests, resource accounting, clocks and cleanup. The manually
strengthened Window ratchet protects the fifteen passing files and leaves
`flex-valid.html` failing. The historical first baseline and frozen input bytes
remain intact; authored controls are not added to the upstream denominator.

Static pixel reftests remain a separate lane with unchanged corpus and ratchets;
the Window results are not added to its totals. Every commit requires fresh
complete measurement and exact-source publication checks. Normal packaging,
installer/updater checks, tagged release, public install/reinstall, actual
prior-version update and native/CDP verification require separate dated receipts;
local regression or Window results alone do not establish those delivery gates.

## Limits and deferred interfaces

All existing source, opcode, requested-System allocation, DOM, CPU, active-time,
lifetime, transaction, wire and shared-work limits remain. Actual declaration
result capacities are admitted before return or DOM mutation. Native parsing
work remains under the existing restricted-process limits, not a new cooperative
CSS quota or a GC/RSS guarantee. Archives must still fit older updaters' 8 MiB
compressed / 64 MiB unpacked ceilings.

This is not the full CSSStyleDeclaration/CSSStyleProperties interface.
`setProperty`, `removeProperty`, `getPropertyPriority`, `length`, `item`, indexed
access/enumeration, constructors/prototypes, generic expandos, computed/rule
styles and `Element.style` PutForwards remain deferred. Existing define/delete
refusal and explicit unsupported whole-style assignment remain. Full CSSOM,
general Window readiness, F-016/F-026 and the formal MVP remain open.

## Current public installer and limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer selects the latest actually published release. Installation uses
a SHA-256 check and a user-level prefix without sudo; checksums are not independent
signatures. Existing builds must restart to use an installed update. Exact
delivered-version and update verification belong to the separate dated receipts.

Modern-web compatibility remains poor. Google search to first result, ordinary
Playwright compatibility, complete browser event-loop/testharness support and
React/Vue browser applications are not acceptance claims. CSS layout, resources,
images and fonts remain bounded subsets. Cookies are memory-only. The restricted
script worker is not a sandbox for the browser as a whole: do not use this
preview for banking, sensitive authenticated browsing or arbitrary hostile sites.
Pinned Boa fatal callback roots may require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old published tags and assets
remain immutable.
