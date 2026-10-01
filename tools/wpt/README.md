# Pinned static WPT pilot

This directory holds a small, reproducible upstream Web Platform Tests baseline.
It uses the real Rust Chassis browser and preserves upstream test bytes. It is
not full `wptrunner`, WebDriver or JavaScript `testharness.js` support.

See [the method, limitations and improvement plan](../../docs/WPT.md).

## Files

- `corpus.json`: pinned revision, mechanical 24-test selection, reference edges,
  byte/hash inventory and license notes.
- `upstream/`: unchanged selected tests, references, support files and licenses.
- `run.py`: static prerequisite checks, loopback serving, real renderer processes,
  control checks, exact pixel comparisons, reports and regression gates.
- `expectations.json`: explicitly reviewed required passes and supported tests.
- `score.json`: deterministic current result summary used by the website.
- `site.py`: strict score validation and generated website section.
- `test_*.py`: harness regression tests, separate from scored upstream tests.
- [`../../examples/wpt_render.rs`](../../examples/wpt_render.rs): browser adapter.

## Commands

From the repository root:

```sh
cargo +1.91.1 build --locked --example wpt_render
python3 -m unittest discover -s tools/wpt -p 'test_*.py'
python3 tools/wpt/run.py --update-score --output tmp/wpt-UNIQUE
rustc +1.91.1 --edition=2024 tools/site.rs -o tmp/site
tmp/site
python3 tools/wpt/run.py --check-score --output tmp/wpt-check-UNIQUE
tmp/site --check
```

Use a new output directory each run. Python 3 and
`/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` are required by the default
command. The browser renderer stays Rust-only. `--font` allows an explicit
diagnostic override; reports retain its hash. The runner verifies Rust 1.91.1 and
rebuilds the locked adapter every time, even when a binary already exists.

Inspect `report.json`, test/reference PNGs, raw RGB files and page logs in the
output directory. Every test stays in the denominator, including unsupported
ones. The first completed baseline is **6 PASS, 1 FAIL, 17 UNSUPPORTED / 24**,
without errors, timeouts or crashes. The failing baseline-alignment test reports
a real layout fallback, which is not accepted as a pass.

Never hand-edit upstream files, remove failing cases, change pixel tolerances or
automatically bless weaker expectations to improve the score. `--update-score`
does not modify the regression ratchet. Corpus/protocol changes require an
explicit reviewed baseline update; browser behavior fixes require the standing
release/installer verification process.

The website publishes the fresh Pages-run report as `/wpt-results.json`, not the
entire artifact directory. Upstream BSD-3-Clause and embedded Ahem public-domain/
CC0 notices are preserved separately from the project's Apache-2.0 license.
