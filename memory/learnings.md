# Candidate learnings

- 2026-09-23: Release version bumps must synchronize the excluded JSPLAN
  workspace's local mg-butane lock entry and exact dependency-audit records.
  Previous v0.7.1/v0.7.2/v0.8.0/v0.9.0 changes did this; the initial v0.9.1
  candidate omitted it and failed --locked before research execution. Curator
  added the recurring check to publish-site, without changing third-party pins
  or weakening the research gate. See the dated log for correction evidence.

- 2026-09-17: Inspector context-menu, compact resize and three-tab About tests
  independently accepted an unchanged previous framebuffer while a requested
  new paint was still computing. Requiring actual requested control/text pixels
  plus stability fixed the native waits without relaxing page equality, query,
  worker or resource assertions. Curator promoted this repeated observation to
  browser-check. Exact artifacts/failures are in the dated log.
- 2026-09-17: Sequential tab-smoke cases lost their owned Xvfb connection when
  the final client disconnected between cases. Its private display now uses
  `-noreset`; the full three-scale/script/restore journey passes. This is a
  test-display lifetime observation, not a browser or global display workaround.

- 2026-09-07: One manually invoked script-worker baseline redirected the restricted child's stdout to a regular file and correctly hit its zero file-size limit. The normal pipe-based protocol returned a valid bounded reply with the same source and unchanged restrictions. Preserve the failed harness output separately; this is a local harness observation, not permission to weaken worker limits.

- 2026-09-07: Feature status and work-log date drive a generated region of the project page. Initial local and GitHub CI validation passed; Pages served matching bytes. The rule is recorded in AGENTS.md and publish-site.
- 2026-09-07: Certificate approval and HTTPS enforcement settings propagated before an already cached HTTP response changed. Check live HTTPS independently and avoid treating a successful settings mutation as proof of redirect behavior.
- 2026-09-07: One documentation-only CI run hit the shared 50-second scripted-CDP deadline on its final fixture after all tests/native journeys and 16 total CDP journeys passed. The same implementation previously passed. Two eight-fixture browser batches pass the exact local replay with17 native/17 CDP destinations; remote verification follows. Every check is retained and harness scheduling remains distinct from browser/worker resource limits. This is a single observed timeout, not general guidance to relax limits.
- 2026-09-07: One core-intrinsic local CI replay exited when a CDP readiness loop read the child launch log before background redirection created it. Missing log is a pending readiness state, not browser failure. Guarded the five identical reads while preserving process checks, deadlines, HTTP readiness, read errors and final assertions. Independent shell cases and corrected replay are recorded in the daily log. Curator leaves this single observation here; no broader skill or limit change.
