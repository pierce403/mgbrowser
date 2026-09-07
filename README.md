# mgbrowser

A web browser written from the ground up in Rust, developed through reproducible experiments and open contribution.

**Status: pre-MVP research.** This repository contains the implementation plan, agent workflow, project website and an experimental Rust dependency foundation. There is no runnable browser or autoresearch executor yet.

- Website: https://mgbrowser.org
- [MVP and architecture plan](docs/MVP.md)
- [Feature specification and acceptance](FEATURES.md)
- [Next tasks](TASKS.md)
- [Autoresearch design](docs/AUTORESEARCH.md)
- [Contributing](CONTRIBUTING.md)
- [Agent instructions](AGENTS.md), [memory](MEMORY.md), and [skills](SKILLS.md)

## Direction

Own the browser engine: HTML parsing, DOM, CSS cascade, layout, painting, navigation, and eventually JavaScript. Start with a useful static-document browser on Linux, then expand compatibility behind explicit acceptance gates. See the plan for the Rust dependency boundary and deferred decisions.

TLS uses the experimental rustls-rustcrypto provider. Fonts and PNG decoding use Rust implementations with native backends disabled. Unsupported images are preferable to C bindings. See [dependency policy](docs/DEPENDENCIES.md); run `cargo test --locked` for dependency integration smoke tests. These are not browser acceptance tests.

## Website development

Open `index.html` directly, or serve the repository locally. After changing feature status or adding a daily log:

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
```

Edit descriptive content in `index.html`; the marked status section is generated from `FEATURES.md` and log filenames. GitHub Actions checks it and deploys on every push to `main`. This keeps updates tied to recorded work rather than automatically inventing progress.
