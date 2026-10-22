# Next milestone: genuine Window testharness readiness

Planning only, 2026-10-21. The 1,011-case flexbox reftest corpus remains separate.
No JavaScript testharness result or new browser capability is claimed here.
Implementing browser services below requires a separate feature contribution
and versioned release after the expanded static baseline is published.

## Immutable input

The current corpus already retains the complete upstream harness at revision
`84daed4ae966d9624bed22f43587c79283ad1fea`:

- `resources/testharness.js`: 198,291 bytes, SHA-256
  `d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e`.
- `resources/testharnessreport.js`: 1,231 bytes, SHA-256
  `3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae`.

Do not replace assertions, remove source sections, hide `document` to force a
Shell environment, or equate harness bootstrap with a WPT file pass. The upstream
report file explicitly documents the vendor `add_result_callback` and
`add_completion_callback` integration seam. Use that seam with bounded typed
records, not console text or DOM-log scraping. `setup({output:false})` may disable
the optional results table; it must not change assertions or completion rules.

## First acceptance: owned controls, no WPT score

Run the complete unchanged harness through the real restricted Boa worker with
authored positive and negative controls: synchronous pass, deliberate assertion
failure, Promise completion, load/task/microtask ordering, uncaught error,
unhandled rejection and missing completion. These controls are not upstream
tests and add nothing to the website's WPT denominator.

Required real browser services and evidence:

1. Keep the Window environment and real document. Correct document-ready and
   Window-load targeting/order, single-window identities, and listener behavior
   must replace the documented startup approximations. The harness registers
   load, message, error and unhandled-rejection listeners; accepting registrations
   alone is not proof that error/message delivery works.
2. Execute a genuine zero-delay task queued by the harness's load callback,
   after the load dispatch and its microtasks. A synchronous callback or Promise
   alias does not satisfy this ordering. Define cancellation, traced callback
   roots and finite cumulative task admission before implementing a task queue.
3. Deliver uncaught errors/rejections to the harness or retain explicit execution
   failures. They may not disappear because a normal-looking completion arrived.
4. Carry bounded typed subtest and harness records inside a reviewed session
   protocol: identity, status, name, message/stack and exactly one completion.
   Missing, extra, malformed, truncated or duplicate completion cannot pass.
   Upstream harness ERROR/TIMEOUT and runner infrastructure failure are distinct.
   Trusted reporter configuration must not be encoded in page HTML/comments.

All page execution stays in the isolated child. A future wait for a delayed task
belongs to parent-controlled idle scheduling, not worker sleeping/busy waiting
inside the two-second cumulative active allowance. No independent unbounded
queue or renewed counters are acceptable.

## Resource gate before another scored cohort

The source fits the existing 256 KiB external-source admission, and its core
listeners fit the 32-registration bound. Those numerical checks do not establish
runtime feasibility. Measure the complete harness under the unchanged 32/64 MiB
requested-System-allocation limits, one million opcodes, one CPU second, two
active seconds, existing wire/transaction/DOM bounds and normal process cleanup.
Full parser/native/GC cooperative control remains unfinished as documented in
[BOA.md](BOA.md). A source-size check is not an allocation or CPU result.

If it exhausts those limits, record the real blocker. Do not prune the harness,
renew counters or silently increase a profile to obtain a pass. Any proposed
new resource policy needs explicit review separate from scoring an engine fix.

Only after this non-scoring readiness milestone should a new genuine upstream
testharness selection be declared from source, frozen before execution, and
reported separately from reftest files and subtest counts. Preserve all current
static pass/support requirements. Do not select only files already known to pass.

Before implementation read [BOA.md](BOA.md), [PAGE_SESSIONS.md](PAGE_SESSIONS.md),
the affected FEATURES.md contracts and the pinned upstream harness itself.
