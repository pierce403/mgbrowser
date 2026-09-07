# Autoresearch contract (design, not yet implemented)

The harness should let a person or any coding agent submit the same kind of bounded, reproducible experiment. It does not require a hosted model or paid API for ordinary contribution. The iterative hypothesis/change/measure loop is inspired by https://github.com/karpathy/autoresearch; browser quality needs several independent gates rather than one optimization score.

## Experiment loop

1. Select one task from `TASKS.md` and affected feature criteria. State a falsifiable hypothesis and editable paths.
2. Record a clean baseline commit, toolchain, fixture-manifest hash, environment, random seed, and explicit wall-time/CPU/memory/disk limits.
3. Make one bounded change in an isolated checkout. Run formatting/build/unit tests and the frozen required corpus.
4. Only after correctness passes, compare repeated performance samples against the same baseline on the same machine. Report distribution and noise, not just the best sample.
5. Write a result with failures, raw artifact hashes, and disposition: improvement, neutral, regression, or inconclusive. Preserve negative results in the daily log.
6. Submit a reviewable patch and evidence. A maintainer or independent CI reruns it. Merge only after review; a local score is not publication authority.

## Evaluation policy

Required conformance is a hard gate. Crashes, hangs, newly skipped tests, missing expected output, resource overruns, or changed evaluator inputs invalidate an experiment. Optional coverage is a separate denominator, explicitly including unsupported cases. Report performance independently; never trade correctness for speed in a combined score. Equal correctness with simpler code can be valuable even without a speed gain.

Candidate code may not modify the evaluator, thresholds, golden outputs, or corpus in the experiment being scored. Changes to those need a separate reviewed contribution and a new baseline. CI uses the trusted evaluator revision, not candidate-provided commands. Hidden/held-out checks and periodic corpus expansion reduce overfitting; published fixtures remain available for contributors.

## Proposed report fields

`schema_version`, `experiment_id`, `hypothesis`, `base_commit`, `candidate_commit`, `toolchain`, `platform`, `hardware`, `corpus_hash`, `evaluator_commit`, `seed`, `limits`, `commands`, `required_passed`, `required_total`, `optional_passed`, `optional_total`, `crashes`, `timeouts`, `render_time_samples`, `peak_memory_bytes`, `artifact_hashes`, `disposition`, `limitations`.

The executor must distinguish failures from unsupported tests and infrastructure errors; write partial results on timeout. Future CLI target: one documented command for baseline evaluation, another for candidate comparison. These commands do not exist yet.

## Execution boundary

Use disposable environments for candidate code, explicit resource limits, no deployment credentials, and local fixtures with network disabled by default. Never run untrusted PR code on a privileged/self-hosted runner or in `pull_request_target`. Agent iteration requires an explicit run budget and stops at exhaustion, permission failure, repeated infrastructure failure, or unresolved scope. The public contribution path runs on normal CPU hardware; optional agent orchestration sits outside the deterministic Rust evaluator.
