# Feature specification

Exact stability values: `planned`, `in-progress`, `stable`. Checked criteria require recorded evidence. This file drives the public status section.

## F-001 — Project foundation and website

Stability: in-progress

### Properties

Public source, readable MVP plan, and an HTTPS project site at mgbrowser.org. Every main push validates and deploys the site; public readiness follows this file and the latest work-log date.

### Test Criteria

- [ ] GitHub repository exists and matches the published commit.
- [ ] Pages deploys the intended commit; live HTML matches local bytes.
- [ ] Custom-domain TLS is valid and HTTPS enforcement is enabled.
- [ ] Generated website status passes the Rust tool's `--check`.

## F-002 — Agent continuity and reusable skills

Stability: in-progress

### Properties

Canonical AGENTS.md, measurable feature contracts, bounded task queue, daily logs, compact memory/skills indexes, and reusable skill procedures. Automatic skill selection follows the catalog and does not authorize unrelated actions.

### Test Criteria

- [ ] Instructions, indexes, referenced skills and first daily log exist.
- [ ] Skill frontmatter validates; harness instruction aliases resolve to AGENTS.md.

## F-003 — Local document rendering

Stability: planned

### Dependencies

MVP scope and Rust dependency boundary; initial versioned fixtures.

### Properties

Our Rust HTML/DOM/style/layout/paint pipeline draws a local heading, paragraph, and link in both a desktop window and headless output.

### Test Criteria

- [ ] DOM and layout snapshots match required vertical-slice fixtures.
- [ ] Repeated headless renders match under a pinned environment.
- [ ] Linux desktop renders the same document and responds to resize.

## F-004 — Static web navigation

Stability: planned

### Dependencies

F-003.

### Properties

HTTP(S), relative links, address bar, reload, back/forward, cancellation, scroll, and visible load/errors; invalid TLS never silently succeeds.

### Test Criteria

- [ ] Local-server tests cover redirects, failures, request/body/time limits and cancellation.
- [ ] Manual Linux navigation and keyboard-control scenarios pass.

## F-005 — Styled text and images

Stability: planned

### Dependencies

F-003.

### Properties

The documented static MVP HTML/CSS subset supports cascade/inheritance, block and inline flow, wrapped UTF-8 text, box styling and PNG images.

### Test Criteria

- [ ] All required versioned DOM/layout/pixel fixtures pass.
- [ ] Unsupported syntax and malformed input produce bounded, documented behavior.

## F-006 — Reproducible autoresearch evaluator

Stability: planned

### Dependencies

F-003 and deterministic baseline corpus.

### Properties

A Rust evaluator executes bounded experiments with immutable evaluation inputs, independent correctness/performance gates, and machine-readable evidence. Humans and agents use the same contribution contract.

### Test Criteria

- [ ] Another contributor reproduces a baseline and candidate report from a clean checkout.
- [ ] Known wrong output, timeout, crash and evaluator-tampering candidates fail gates.
- [ ] Reports contain the provenance and metrics specified in docs/AUTORESEARCH.md.

## F-007 — Linux MVP release

Stability: planned

### Dependencies

F-004, F-005, F-006.

### Properties

Installable experimental static browser with documented limitations and no claims of modern JavaScript or hardened general-purpose browsing.

### Test Criteria

- [ ] Clean-machine install, full required corpus, manual browser scenarios and resource limits pass.
- [ ] Dependency/native-code and license inventory is reviewed.
- [ ] Release artifacts, provenance and known limitations are published.

## F-008 — JavaScript and broader compatibility

Stability: planned

### Dependencies

F-007; separate engine, isolation and standards roadmap.

### Properties

Future own Rust JavaScript engine and expanded platform support; no embedded existing browser/runtime shortcut.

### Test Criteria

- [ ] Language/runtime subset, conformance suite and active-content isolation gates are specified before implementation.
