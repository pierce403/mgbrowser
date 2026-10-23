# Modern declaration editing methods

2026-10-23: next browser increment after fully delivered v0.9.16 and its
`e0341d81bf67767c4013b64a1f467a9994598b07` documentation closeout. This is the
pre-implementation behavior and acceptance contract, not a delivery claim.
The broader F-003/F-005/F-016/F-026/MVP criteria remain open.

## Behavior

Add callable modern element-style `setProperty`, `removeProperty` and
`getPropertyPriority`, complementing existing `getPropertyValue`. Reuse the
locked Stylo declaration block, enabled grammar and real DOM attribute/base URL.
Names are CSS names, not camelCase; custom-property case remains significant.
Priority is empty or ASCII-case-insensitive `important`, without trimming.
An empty value removes even with invalid priority. Invalid nonempty values or
priority and unknown properties leave the original attribute untouched.
Setting replaces both value and importance, including same-value changes.
Use the declaration update planner: equal value/priority can still require
moving a physical longhand after a competing logical longhand.
Shorthands are generic: set/remove all longhands atomically; priority is important
only when every constituent is present and important. Removal returns the prior
specified value, even when its empty shorthand serialization hides partial
longhands that still must be removed. Reads never rewrite the raw attribute.

Use CSSOMString=USVString. Validate genuine same-realm style receiver before
required arity (2/1/1) and conversions. Convert arguments left-to-right before
semantic CSS checks. Value/priority use LegacyNullToEmptyString; missing or
undefined optional priority defaults empty. Ignore extra arguments. Set returns
undefined. Removal/priority return strings. Native dispatch cannot call author
overrides. All four style methods have stable per-realm identity and correct
public name/length metadata; other DOM method metadata is unchanged.

Primary review: [CSSOM draft 2026-10-13, section6.6.1](https://drafts.csswg.org/cssom/#the-cssstyledeclaration-interface)
and [WebIDL 2026-10-19, conversions and operations](https://webidl.spec.whatwg.org/).

## Implementation and resource boundary

Keep the actual style attribute as the only retained declaration store. No
parser shortcut, expected-result table, grammar preference, atom prewarming,
dependency feature/version change, new task, wire field or worker profile.
No State/DOM borrow survives callback-capable conversion. Reentrant conversions
can mutate/detach the owner; read its current state only after all conversions.
Preserve thrown-value identity, completed inner effects and unmaskable VM fatals.

Keep existing 16,384-byte current/incoming-name/value/canonical-output limits;
apply the same input ceiling to priority. Oversize is an ordinary Host error,
including invalid/unknown/removal inputs, not a successful CSS no-op. Do not
relax cumulative DOM, worker System, opcode/source/work, active/lifetime/CPU,
wire, transaction or child limits. Native parser/atom work still relies on
existing process containment, not comprehensive cooperative budgeting.

Prepare the mutation without touching DOM. Admit serializer/canonical String
capacities and UTF-16 result capacities through the existing Host accounting.
For removal, construct/admit its return string before committing the attribute;
result or canonical/attribute admission failure must preserve the whole prior
attribute. Attempted charges remain cumulative, not refunded. A no-op must not
create an absent style attribute or canonicalize malformed/raw spelling.
Method presence is structural: `in` must not parse, allocate a return value or
materialize a method/wrapper, including at the existing exhausted Host quota.

## Frozen acceptance before implementation

Add separate additive public controls and private declaration/resource controls,
without editing prior CSSOM or legacy expectations. Freeze/read their literals
before execution, then run on the unchanged implementation and retain actual
missing-method assertion failures. Required coverage:

- Real callable/cache/name/length and borrowed/extracted/bound receivers;
  forged, inherited, author-Proxy, wrong-kind and foreign receivers reject
  before arity/conversion or user getters. Structural membership is allocation-free.
- Arity, left-to-right conversion, omitted/undefined/null priority, null value,
  lone-surrogate scalar replacement, Symbol/throw identity and ignored extras;
  unknown property/invalid priority still perform argument conversions.
- Important replacement, exact no-op raw preservation, empty-value removal before
  priority checks, invalid whole-value/trailing-declaration refusal, case-sensitive
  custom properties, shorthand set/read/priority and partial/mixed removal.
- Escaped style identity, external attribute changes/removal, detached owners,
  conversion reentry and no calls through author prototype methods.
- Exact input/output/capacity edges, ordinary quota errors, pre-commit result and
  mutation failure, sticky existing VM fatal behavior and unchanged work counters.

Use real fresh restricted children for first parser/custom-atom/URL operations,
and a retained lifecycle/click control for actual later method mutation. Keep all
older worker/resource/native/CDP assertions. Add a separate owned native/CDP
fixture requiring these methods to create/style usable ordinary text links or
controls and navigate to distinct real destinations. Literal native paint and
real requested URLs must complement worker/library assertions. Repeat packaged
and actual public-installed bytes; do not modify earlier physical/atomic or
thirteen native helpers merely to make this fixture pass.

Both complete unchanged score lanes must run before every commit, with complete
retained outcome/resource/cleanup review and unchanged manual protections.
Authored controls add no upstream numerator/denominator. Do not predict gains:
the two current flex-valid assertions require separate container-unit grammar
review; they are not missing editing-method calls.

Finish ordinary version/versioned notes, full Rust/JSPLAN regressions, package/
installer/updater, all existing native/CDP and frozen observer suites, exact-source
CI/site, immutable tag/normal release, checksum-verified public install/reinstall,
actual v0.9.16 update/no-op and public repeats. Preserve the <=8MiB archive limit.

## Still unfinished

Length/item/indexes, ownKeys/define/delete/enumeration, declaration constructors/
prototypes, parentRule/computed/rule styles, generic expandos, Element.style
PutForwards and custom elements/CEReactions remain separate requirements.
No full CSSOM, framework, Playwright, general WPT or modern-web compatibility
is inferred from these methods. Empty-anchor parent default validation also
remains its own increment; this fixture uses ordinary text links.
