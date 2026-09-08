# Static AST operator ownership

## Adopted contract

Baseline: published 67ab8c0a6c63eb5cb5676aef493a0a9bb03bd890. This is a
representation-only change to the original Rust parser, not new JavaScript
syntax or permission to adapt website scripts. Freeze independently authored
semantic cases, allocator measurements and a real-worker form baseline before
building the candidate.

The four Unary/Binary/Assign/Update AST operator fields and BinaryRight parser
continuation use canonical `&'static str` instead of individual owned Strings.
Punctuation is already static; map owned keyword tokens to literal `in` and
`instanceof` spellings. Never borrow source/token buffers or leak a String.
The public experimental Rust AST field type changes; consumers constructing an
operator must supply a static spelling. Runtime language behavior is unchanged.

Frozen vocabulary:

- Unary: `+ - ! ~ typeof void delete`.
- Update: `++ --`, prefix and postfix.
- Assignment: `= += -= *= /= %= <<= >>= >>>= &= |= ^=`.
- Binary: `|| && | ^ & == != === !== < > <= >= instanceof in << >> >>> + - * / %`.

Preserve precedence, associativity, NoIn, ASI, invalid-target errors and current
rejection of exponentiation, nullish and logical assignment syntax. Do not edit
the evaluator or enable its parser-inaccessible branches. Identifiers, property
names, string literals, regex patterns/flags and all dynamic source remain owned.

Remove only the storage visitor's charges for the operator buffers that no
longer exist. Actual boxes, vector capacity/spare slots/holes, Rc payload/control
allowances and every other String/UTF-16 buffer remain charged. Measure actual
enum layouts, requested retained allocator bytes and block counts independently;
do not lower generic node weights. Source/ingress/copy accounting, fatal latching,
fuel, parser limits and worker isolation/deadlines remain unchanged. Separate
parses still pay for all real storage; existing closure sharing stays unchanged.

## Acceptance gates

- Frozen explicit semantic expectations pass on both baseline and candidate,
  including every operator family, ordering/short circuits/errors, UTF-16,
  dropped source buffers and dynamic/retained closures.
- An allocator probe records old/new retained bytes and live blocks after parser
  temporaries disappear, then verifies drop restores the original snapshot.
  Operator-free and sparse/container controls remain; this is not RSS evidence.
- An independently authored operator-rich Function builds every form control in
  the actual restricted worker. Freeze its source and old failure first; require
  completion under the unchanged 4 MiB budget and real native/CDP submission,
  first authored result and separate destination.
- Review old exact AST checkpoints against counted removed operator allocations;
  preserve non-AST phases for equal completed work. Keep all original failures
  and measurements. Negative storage/copy/depth/fatal controls must still pass.
- Run full debug and selected release gates, dependency guard, worker/session
  selftests and all existing native/CDP journeys before one bounded Google attempt.
  A local form or changed live diagnostic never completes the actual Google goal.

## Frozen baseline

On Linux x86_64, the old library measures Expr/OptionExpr 56 bytes and Stmt,
ForInBinding and SwitchCase 80 bytes. The unchanged 23-case allocator probe
passes: 12 structural/owned-buffer controls also compare runtime AST allowance
with retained requested bytes plus 16 per live block; 11 diagnostic/producer
sources are parsed, cloned and dropped without executing their Host/fuel work.
Every final-owner drop restores the allocation snapshot. The 14,500-binary
retained function owns 2,798,856 requested bytes in 43,504 blocks, allowance
3,494,920. Its operator inventory is 14,500 one-byte spellings; eliminating those
buffers predicts 14,500 fewer requested bytes/blocks and 246,500 less allowance.
The candidate must verify that prediction; these are not RSS measurements.

The unchanged semantic suite passes all 26 groups against the old library.
The 778-byte authored form fails in the old actual restricted worker: no scripts
complete, no controls are created, readable fallback remains, and Ast 3,507,024
is rejected after 789,817 accepted bytes under 4,194,304. Accepted phases are
Bootstrap 26,880, Source 59,444, Ast 2,874 and Runtime 700,619; other phases zero.
The first harness accidentally supplied regular-file stdout and hit the worker's
file-size limit (exit 153). Its empty output is preserved separately. The normal
pipe-based harness subsequently returned the above valid reply with exit 0;
neither fixture nor worker restriction changed.

Frozen source hashes and original logs are recorded in the daily log. These
baseline observations alone did not establish candidate or live-site acceptance.

## Candidate storage and component checks

The identical allocator corpus confirms all predictions on x86_64: layouts,
source/inventories and all non-AST phases are unchanged. Every case removes only
the counted operator payload and one allocation per operator. All eight
operator-free controls are identical, and all 23 final drops restore their
snapshots. The retained 14,500-binary function now requests 2,784,356 bytes in
29,004 blocks: 14,500 fewer bytes/blocks and 246,500 less logical allowance
(3,494,920 to 3,248,420). Its Rc-sharing clone is still 84 bytes/two blocks.

The public [measurement example](../examples/measure_ast_storage.rs) is a
comment/format-only successor to the frozen probe; their complete candidate
outputs match. CI explicitly executes its fixed 23-case corpus under a 15-second
outer timeout. This is a reproducible storage guard, not the general research
executor. See RUNNING.md for the command.

The candidate passes 26 unchanged semantic groups, all ten resource groups,
two focused private lifetime/storage tests, DOM creation and actual-worker
positive/larger-body-fatal checks. Full debug passes 1,145 tests/48 targets;
selected release passes 1,032/38. Counts exclude exactly two repeated owned-child
summaries per profile. Formatting, locked builds, dependency guard, standalone
allocator guard and actual worker/session selftests pass. Old exact checkpoints
subtract only independently counted operator buffers from AST and summed totals;
both fuel checkpoints and every other phase remain unchanged. The diagnostic
worker fixture specifically loses 24 buffers/36 payload bytes: 420 AST bytes.

All four local CI journey blocks pass: 24 native/24 external CDP completions,
scripted batches 8/8/6 and retained requests two searches/two destinations/zero
traps. The frozen operator-rich form now completes one script with no errors at
4,053,180 accepted bytes: B26,880 + Source59,444 + Ast3,263,194 + FunctionCode137 +
Runtime703,525. Its real Unicode query/hidden field submits, first authored result
is clicked and a separate HTTP 200 destination loads. Frames were inspected and
owned processes/listeners reaped. The old failure and new completion do different
amounts of work; their totals are not an equal-work performance comparison.

One subsequent bounded Google attempt remains incomplete. Homepage and actual
served-form submission succeed; search HTTP 200 has no items/forms. First error
is unsupported Object.create property descriptors, then Ast378,301 is rejected
after 4,128,478 accepted at the unchanged 4,194,304 cap. Two scripts complete with
three errors; exit2 and a blank inspected frame leave the actual result and
destination gates open. Changing served inputs are not a controlled benchmark.
No live source inspection/adaptation, retry, impersonation, substitute service,
new dependency, raised limit, worker permission or CDP command was introduced.

Local acceptance is complete. Exact-SHA remote CI, Pages and public HTTPS
verification are separate publication gates recorded in the daily log.
