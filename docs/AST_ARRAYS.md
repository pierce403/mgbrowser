# Completed array-literal AST storage

Bounded implementation contract adopted 2026-09-08, before parser edits.
Acceptance is pending the old/candidate parser, resource, worker and native/CDP
checks below. This is not a Google compatibility claim or a general research
executor.

## Scope and invariants

Only completed array-literal builders change. After the existing node/depth
admission, both expression-parser closing paths consume spare-capacity
`Vec<Option<Expr>>` storage through `into_boxed_slice().into_vec()`. Already exact
vectors are moved directly. Do not revisit the complete tree or add recursive
finalization. Preserve the public Vec AST shape and every element/payload owner.

The pinned Rust 1.91.1 local standard-library documentation explicitly shows
excess capacity removed by `into_boxed_slice`; `Box<[T]>::into_vec` consumes the
allocation without cloning or allocating and sets capacity to length. See local
`alloc/vec/mod.rs` lines 1539–1574 and `alloc/slice.rs` lines 458–481 in the
toolchain's rendered source. The consuming conversion is chosen over a promise
that `shrink_to_fit` must always leave exact capacity. No new unsafe code is
needed in the parser.

Retained AST accounting remains capacity-based, including holes, separately
owned descendants and the existing 16-byte per-block allowance. The storage
visitor and all Runtime array builders remain unchanged. Public manually built
AST vectors with spare capacity must still pay that capacity. No weight reduction
without real storage removal, source refund, payload clone omission or shared
mutable arrays is allowed.

Preserve holes versus explicit undefined, trailing length, element order and
single evaluation, UTF-16 units, signed zero, object/Symbol identity, closure/code
lifetime, deep AST clone ownership and existing shared function slices. Grammar,
offsets, first errors, node/depth/work/frame counters and failure cleanup remain
unchanged. Finalization must not run before a node/depth rejection or invoke
JavaScript, coercion, getters or Host callbacks.

No root/block/case/object/sequence/argument/string/regex container changes, new
language feature, dependency, worker authority, CDP command or live-site source
adaptation is included.

## Costs and limits

The existing logical realm policy charges successfully retained AST storage, not
each parser temporary or failed partial tree. It is not allocator RSS accounting.
Keep that boundary explicit: finalization can add one realloc request and move
the live outer buffer. A moving realloc may transiently retain old and new blocks;
retained savings alone do not prove lower peak memory or faster parsing.

The independently frozen 168-case probe compares Keep, shrink and consuming box
round trip over actual 56-byte `Option<Expr>` slots. Root observed all cases pass
on the pinned x86_64 toolchain before adoption: values, holes and descendant
pointers/capacities were preserved, no more than one outer-sized allocation
request occurred, and complete drop restored live requested storage. Empty and
exact-capacity inputs needed no request. A 10,000-slot geometric builder changed
from 16,384 to 10,000 slots (917,504 to 560,000 bytes); 4,097 changed from 8,192 to
4,097 (458,752 to 229,432 bytes). Both resizing strategies matched on this
allocator, but that is not an API guarantee for arbitrary allocators.

Each resized case requested its complete new buffer once. Observed post-call
live-request peaks added zero; the separately reported conservative moving-realloc
overlap allowance adds up to the complete new buffer, including 560,000 bytes
for 10,000 slots. This wrapper cannot observe allocator-internal peaks, usable
sizes, RSS or physical copying. Parser-level adverse cases and actual worker
deadlines remain required; no timing speedup is inferred from finalization-only
windows.

Preserve the 4 MiB cumulative logical realm cap, Bootstrap 26,880, one million
fuel, object/function/environment and runtime-array/argument bounds, 64 calls,
128 expression/384 combined entries, and default stacks. Keep parser 1 MiB
source, 100,000 tokens/nodes, depth 128, 1,536 continuation slots and 3,200,000
dispatches. Do not introduce a 10,000-slot parser cap: an uncalled larger literal
may parse, while executing it must still reject at the existing runtime limit.
Worker 1 CPU second, 256 MiB address-space and two seconds total active wall,
retained lifetime/transaction/wire limits, and fatal latching are unchanged.

## Independent gates and explicit old assertion migration

The frozen 21-group semantic/resource corpus passes unchanged on the baseline
before implementation. Preserve its source for candidate comparison. It includes
both closes, holes/undefined, side effects/errors, UTF-16, clone/lifetime, eval
scope, parser limits and exact unrelated fuel/full-phase controls.

Freeze parse/clone/drop allocation windows on authored empty, exact/spare, owned
payload, nested/shared-body and many tiny arrays, including arrays completed
before a later syntax error. Report request traffic, retained storage, observed
and conservative requested-byte peaks, complete cleanup and observational timing
separately. Run on the unchanged default stack and bounded timeout. Stop for
unacceptable transient/time behavior; do not raise limits to accommodate it.

The existing 23-case AST guard stays source-identical. Its expected retained
changes are exactly minus 357,504 bytes for 10,000 holes and minus 229,320 for
4,097 numbers; 4,096 and the other 20 cases stay unchanged. Live block counts,
payload ownership and the independent retained-plus-block AST allowance must
still agree. The separate descriptor/runtime measurement stays unchanged.

Four historical pins require explicit review, not silent weakening:

- Preserve the old 4,096-to-4,097 geometric assertion and old raw results. Replace
  its representation-specific expectation with exact capacities and a 56-byte
  AST increment for both dense and sparse inputs.
- Preserve the five-nested-10,000-hole source as a new positive if it fits; add a
  separately frozen eight-array AST negative before any prefix, hoist or handler.
  Never remove the fatal pre-admission gate merely because less retained storage
  makes the old source admissible.
- Preserve the existing six-iteration sparse actual-worker source and its old
  report: Ast 917,689 rejected after 4,002,854 accepted. Candidate first phase
  and exact request must be measured before changing its obsolete geometry pin;
  readable fallback, no controls/navigation and repeated fatal error remain.
- Repeated execute/eval/Function roots each now accept seven independent trees.
  On attempt eight execute rejects Ast 560,520 after 4,034,775; Function rejects
  Ast 560,185 after 4,192,573. Eval first admits its unchanged 20,066-byte incoming
  UTF-16/native-name copy, then rejects Source 10,031 after 4,185,104, before
  parsing. The initial expectation that all three would still reject Ast was
  wrong. Preserve both original test sources and failed logs; require this exact
  mode-specific Source boundary, unchanged Source/Ast/FunctionCode totals and the
  exact ingress delta. Do not use a broad Source-or-Ast alternative. The separate
  eight-nested negative still requires Ast pre-admission for all three roots.

The original six sparse *evaluations* must still reject the sixth 640,000-byte
Runtime request with `i == 5`. Repeated parse/eval/Function storage stays cumulative.
No unrelated test allowance may be relaxed to hide a changed first failure.

Freeze a clearly authored handler-free form requiring several retained AST
arrays but executing only one. Verify its old failure before implementation and
its candidate real controls in the actual worker, native window and external
Rust CDP client. A separate over-runtime-limit literal must compile, then fail
before its element effects without catch/finally/later navigation. Run the full
component, retained-event and native/CDP regression gates before one bounded
authorized live Google attempt. The Google result/destination goal stays open
unless those actual stages are observed. Exact publication evidence belongs in
the dated log, separately from local acceptance.
