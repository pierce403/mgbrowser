# Typed Window observer: implementation and evidence

2026-10-22: **v0.9.12 candidate, locally verified, not yet released**.
The [reviewed contract](WPT_OBSERVER.md) and
[sixteen frozen authored inputs](WPT_OBSERVER_CONTROLS.json) remain byte-for-byte
unchanged, including their historical preimplementation status. This companion
records later implementation, usage and measured local results. It does not
adopt a scored testharness selection or complete general Window readiness.

## Implemented boundary

Observation is trusted host configuration, Disabled for ordinary browsing.
`BoaPageRealm::start_with_observer` and `LinuxScripts::with_observer` accept
`ObserverInit`; default constructors and unversioned one-shot requests cannot
enable it. The host must verify actual HTML and every admitted external source,
including complete bootstrap bytes, before forwarding startup. The child checks
bootstrap node/order and exposure authority independently. Page markup, URLs,
globals and CDP cannot grant this configuration.

Strict modern v6 requires `ModernInit { page, observer }` and matching non-null
observer metadata on every reply, including explicit Disabled. Closed objects,
strict scalar/enum types, mode/run identities and bounded sequences reject
missing/null/extra/malformed data and earlier modern versions. Explicit legacy
v1 and one-shot representations retain their old bytes.

Native callbacks register through the complete unchanged harness's vendor seam.
Capture reads only required own DATA descriptors of ordinary objects and real
Arrays, without getters, Proxies, coercion, JSON scraping or recursive checkpoints.
A private traced weak Test-to-scalar map preserves identity without strongly
retaining Test objects. Lossless UTF-16 and Undefined/Null/Text stay distinct;
final names reconcile only unchanged text or the pinned surrogate sanitizer.
Raw harness/subtest statuses and final-only negative inventories are preserved.

The shared `page_session::observer::ObserverLedger` stages cumulative deltas
without cloning accepted history. Chassis publishes only after outer frame,
resource, lifecycle and arena validation plus the existing one combined active
charge. Rejected projections/charges publish nothing and block later work.
`Browser::script_observation` exposes this accepted read-only ledger, not raw
worker fields or page metadata.

Completion is provisional through the current checkpoint and actual outstanding
rejection/handled-notification tasks. Normal scheduling reports Settling until
those obligations finish. Finished is the accepted checkpoint cutoff, not
absence of future errors: remaining unrelated work is reported, further test
commands stop, and the host closes/cancels and explicitly verifies `wait_idle`.
An assertion Fail, uncaught error, unhandled rejection, missing completion,
genuine harness Timeout and infrastructure failure remain different outcomes.
Expected-negative verification is not a browser PASS.

## Bounds and trust

Observer narrowing caps are 256 identities/final rows, 256 Result callbacks,
one Completion, 257 total ordinals, 4,096 UTF-16 units per field and 131,072
cumulative copied units separately in worker capture and consuming parent.
Actual retained Vec capacities and copies are admitted before retention; freeing,
publishing or abandoning a stage does not refund cumulative copy admission.
The worker charges fixed capture/native-callback storage, its weak-map wrapper
and 256 bytes per membership. Opaque upstream weak-map bucket storage is covered
by actual System admission, not claimed as a measured host-map capacity.

No existing limit increases: 4 MiB host/DOM storage, 32/64 MiB outstanding/
cumulative requested System allocation, one million opcodes, 256 pending / 2,048
lifetime shared work, one CPU second, two cumulative parent active seconds,
64 transactions, 300-second session lifetime, 4 MiB replies and 32 MiB wire.
Source, listener, event, arena and worker-permission limits remain unchanged.
These counters are not RSS or GC live heap; full cooperative P1 control remains
unfinished. Faults, privacy loss and native/parent execution failures are sticky;
fatal failure retains its original cause and requires real terminal teardown.

This is cooperative source-inventory-based observation, not hostile-page
attestation. The pinned harness exposes its internal Tests/callback arrays to
configured JavaScript; well-formed fabricated callbacks are not unforgeable.
The Browser host requires an immutable reviewed two-route owned server. Request
inventory checks happen after initial resource fetch, not before arbitrary-server
network access. Its one-navigation fence and navigation/meta-refresh denial are
test-host guards, not a whole-browser network sandbox.

## Measured local acceptance

Both paths used all sixteen identical frozen controls and the complete upstream
198,291-byte `testharness.js`, SHA-256
`d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e`.
Manifest SHA-256:
`0e82815d53f382904ae5786c5da82d1b700872b7eb73adcdc63767a0f9da7317`.
Evaluated production/build source fingerprint for these receipts:
`f8b3f06924dfe11b4dc76887636eb047cd89efd052cb2bec5afb5e11b997148d`.
These are interim normal release-mode, non-legacy candidate builds identifying
the unpublished `d9ce21c10469-dirty` workspace, not published v0.9.12 artifacts.

| Local path | Outcome | Aggregate elapsed | Receipt SHA-256 |
| --- | --- | ---: | --- |
| Restricted executable via external stdio driver | 16 selected/completed, all frozen expectations met | 34.365 s | `9ad0bfe8b0bafe2382b327eb2bdb1bae3cb4fe7f8c271636767b8b7ef7354844` |
| Real Browser, resource broker, manager and accepted ledger | 16 selected/attempted/completed/verified, all expectations met | 33.945 s | `f8ade6c929ccb249b613a2e0b873fede9ff8a30b6647f64f581be603ce53b90e` |

Direct receipt: `tmp/oct22-observer-direct-driver-a/receipt.json`. Actual normal
`mgbrowser` executable SHA-256:
`69c866d1db4eb011da2caa71f6c047ddad2dc9aa866192b76b0bb1e2cf5a92b6`.
The external driver sends strict v6 Init/ordinary TaskPump frames to that binary's
existing `--script-session` entry, uses the shared ledger and independently
accounts parent work. All owned worker groups/direct-leader cleanup and outer
driver cleanup were confirmed; no case reservation remained. It bypasses the
Browser/network/manager and is not native/CDP acceptance.

Browser receipt: `tmp/oct22-wpt-observer-browser-a/report.json`. Real host
executable SHA-256:
`4bceeddf0a91d0403eb74ed9c8620d59c84022ed484abe3d75a7dfa82c64da44`.
Every case used one real worker startup, the actual post-charge Browser ledger,
exactly two ordered complete GET/200 responses for its HTML and full harness,
explicit successful pool `wait_idle`, normal host exit, owned-group cleanup and
fixture shutdown. Cases 13/15 retained first Settling then Finished with sticky
native failure. Missing completion remained Waiting at 22.023 s; the genuine
unchanged 10-second harness timer produced Timeout at 10.114 s. These are
different negative controls, not substituted timeouts or killed-host successes.

Each case has one absolute 25-second whole bound. The direct driver stops
admission by 23 s and reserves two seconds for owned cleanup. The Browser owner
deliberately narrows admission to 22 s and process/report completion to 22.9 s,
leaving the existing two-second group cleanup plus 0.1 s fixture/final reserve
inside the same 25 s. Host setup/fetch/pending/validation/verdict/artifact work
counts against one two-second active accumulator; only measured validated
no-in-flight Idle/Scheduled sleep and cleanup are excluded. No renewed deadline
or inference of a full 23-second Browser run is made. Cleanup evidence concerns
owned groups/direct leaders and explicit pool wait, not individual descendant
reaping or an adversarial executor.

## Final-version local gates

The final 0.9.12 candidate has evaluated source fingerprint
`9f7b953a21f267af76f3546c1e81ec3ec7c7d3922bea686a26ae69d741275aa4`.
It still identifies the unpublished `d9ce21c10469-dirty` build, not a public tag.
All 2,065 workspace tests pass in both debug and release, with two intentionally
ignored updater gates, plus independent no-chrome embedding and dependency/
component guards. The separate locked JSPLAN comparison preserves all 364
outcomes. Full static WPT remains 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011;
complete case, pixel, request, cleanup and actual System-delta review passes.

The normal, non-legacy package is 8,006,812 bytes, within the unchanged 8 MiB
download ceiling. Its executable is 22,991,408 bytes, SHA-256
`92ee92ca02e2bf0a41a17d63814cc4f9a275610e639d20b344d37543ed1d77f1`.
Default/custom installation, running-binary reinstall, bad checksum rejection,
desktop/icons, real Boa/worker/session and packaged updater checks pass, followed
by all thirteen unchanged native helpers. Package receipt SHA-256:
`bfde1904794449f139670e5be89d1b65662c9bfacc63a2eebf868525c4e7a85b`.
These local package bytes are not presumed identical to a future cold CI build.

All sixteen exact controls pass again against the actual packaged executable
through the direct driver in 34.887 seconds. Receipt
`tmp/oct22-v0912-observer-direct-package-a/receipt.json`, SHA-256
`56a906e9e6f10977721df35dcbda2b9f1d43ad6520efdde7523cb30e4a84b395`.
A newly built normal Browser example independently verifies all sixteen in
34.617 seconds, with 32 exact ordered GET/200 requests, no omissions, explicit
pool wait_idle and confirmed owned cleanup. Receipt
`tmp/oct22-v0912-observer-browser-final-a/report.json`, SHA-256
`e30c60c7cf3fa802b198496245cc764a287c6b55cb97af1f6e4adde87ff1f834`.
MissingCompletion remains Waiting through the declared second-22 cutoff
(22.038 seconds total); the unchanged 10,000 ms harness timer genuinely expires
at 10.123 seconds. Cases 13 and 15 first publish Completion as Settling, then
Finished with sticky execution failure. No reservation or unknown cleanup remains.

## Contributor reproduction

Linux x86_64 is required. Build the contributor examples with locked Rust 1.91.1;
they are not installed browser commands. Keep normal production binary builds
separate from `legacy-test-engine` regression builds, which can overwrite Cargo's
root executable. Supply the exact verified binary SHA, not a version-only match.

```sh
cargo +1.91.1 build --locked --release --example wpt_observer
target/release/examples/wpt_observer \
  --binary /absolute/path/to/verified/mgbrowser \
  --expected-sha VERIFIED_64_CHARACTER_LOWERCASE_SHA256 \
  --case 01-sync-pass \
  --output "$PWD/tmp/observer-direct-01-new"
```

The output must be a new direct child of this checkout's ignored `tmp/`.
`--case` accepts only a frozen manifest ID; one invocation tests one case.
The driver retains bounded raw frames and a typed receipt and exits zero only
when that case's exact positive or negative expectation and cleanup are verified.
Full acceptance must enumerate all sixteen; a single positive is not the gate.
It does not fetch a test page or require a font. Use a reviewed finite outer
owner for full-matrix process/evidence admission; ignored session-specific owners
are evidence artifacts, not public entry points or a general WPT runner.

The separate Browser example's interface is:

```sh
cargo +1.91.1 build --locked --release --example wpt_observer_browser
target/release/examples/wpt_observer_browser \
  --origin http://127.0.0.1:OWNED_EPHEMERAL_PORT \
  --case 01-sync-pass \
  --font /absolute/path/to/verified/DejaVuSans.ttf \
  --font-sha VERIFIED_FONT_SHA256 \
  --output "$PWD/tmp/observer-browser-01-new" \
  --deadline-monotonic-ns OWNER_ABSOLUTE_CLOCK_MONOTONIC_NS
```

This is an interface example, not a runnable arbitrary-URL command. A trusted
owner must first create the immutable loopback fixture serving only the chosen
exact `/owned/CASE_ID.html` and complete `/resources/testharness.js`, provide
fresh private HOME/XDG directories, set the absolute deadline once, supervise
the process/group and fixture within it, and verify exact requests and receipts.
The host re-executes itself for existing early worker modes. It writes bounded
`accepted.json` from the actual Browser ledger plus `receipt.json`; normal exit,
matching pins, expected outcome and confirmed cleanup are all prerequisites.
The host's process/report cutoff is owner deadline minus 2.1 s, not a new local
25-second allowance. Do not reuse a stale deadline/output or point it at a
mutable/general-purpose server.

## Remaining release and standards gates

Exact-commit CI/site delivery, tagged release, public assets/checksum/install,
actual old-binary update and all public native/direct-observer acceptance remain
pending. Completed local candidate gates above do not satisfy public delivery.

The published static selection remains 1,011 upstream files: last published
317 PASS / 632 FAIL / 62 UNSUPPORTED. Corpus, scorer, assertions and expectations
are unchanged. These authored controls add **zero WPT passes**. General WPT
testharness file/status mapping, a new source-selected cohort, remote contexts,
WebDriver/ordinary Playwright compatibility and broad web support remain open.
