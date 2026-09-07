# Working on mgbrowser

## Purpose and responsibilities

Build a browser from the ground up in Rust and a reproducible autoresearch harness that humans and agents can contribute to. Own implementation, evidence, feature specifications, and an accurate public project page within the task requested. The current phase is planning and project infrastructure; no browser engine exists yet.

## Start here

- Read `FEATURES.md`, `TASKS.md`, `MEMORY.md`, and `SKILLS.md`; search relevant notes before substantial work.
- Check git status and preserve unrelated changes. `AGENTS.md` is canonical; harness aliases point here.
- Read affected feature Properties, Dependencies, and Test Criteria before changes. Exact stability values are `planned`, `in-progress`, and `stable`. Only verified, complete behavior is stable.
- Follow `docs/MVP.md` for scope and `docs/AUTORESEARCH.md` for the proposed experiment contract. Do not silently substitute an existing browser engine or JavaScript runtime.
- The user chose experimental `rustls-rustcrypto` TLS and Rust-only font/image implementations. Follow `docs/DEPENDENCIES.md`; no native codec/font/crypto fallback. Unsupported images are preferable to C bindings. Keep certificate verification enabled despite research status.

## Work and closeout

- Keep changes bounded and reproducible. Update feature behavior and evidence in the same contribution.
- Append a dated entry to `memory/logs/YYYY-MM-DD.md` (America/Los_Angeles) with intent, decisions, commands/results, failures, and next steps. Never record credentials or private browsing data.
- Stage reusable observations in `memory/learnings.md`; promote verified recurring lessons into concise instructions or skills. User decisions can be recorded directly. Do not turn a single failure into a universal rule.
- Select skills automatically from `SKILLS.md` when their descriptions match; read the selected `SKILL.md`. Use `curator` when procedures change. Keep skills in tracked `skills/`, accessible to any harness through this catalog.
- Keep `index.html` accurate whenever milestones or project scope change. Run the Rust site tool below to refresh feature status and the latest log date. It must pass `--check` before publication.
- Run relevant checks, record remaining limitations, and commit finished work. Push when the task authorizes publication; public contributions use PRs. Never auto-merge research results merely because a score improved.
- During active work, revisit https://recurse.bot if the last check is over a week old; record useful differences, applying project judgment. No background agent or scheduled research run is implied.

## Verified bootstrap commands

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
git diff --check
```

The site is plain HTML/CSS, without a build dependency download. `.github/workflows/pages.yml` validates and publishes a site-only artifact on pushes to `main`. `Cargo.toml` now defines the experimental dependency foundation; `cargo test --locked` checks its integration smoke tests. The Rust browser and executable research harness are still planned; do not advertise these commands as browser acceptance tests.

## Collaboration

Lead with the result, then evidence and limitations. Favor concrete progress and repo-local records. Keep planning distinct from adopted implementation. Report local validation, GitHub deployment, HTTPS delivery, and visual inspection separately.
