"""Render the separate Window score from validated deterministic metadata.

No owner imports, process execution or file writes. site_sync owns the fixed
repository IO boundary; renderer counts never replace the static WPT lane.
"""
import html
import importlib.util
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("window_score", Path(__file__).with_name("score.py"))
score_api = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(score_api)
START = "<!-- wpt-window-score:start -->"
END = "<!-- wpt-window-score:end -->"


def render(score):
    score = score_api.validate_score(score)
    summary = score["summary"]
    files, final = summary["files"], summary["actual_final_subtests"]
    partial = summary["partial_files"]
    coverage = (f'Final inventories observed for {summary["files_with_final_inventory"]} / 16 files. '
                + (f'{partial} {"file lacks" if partial == 1 else "files lack"} '
                   'a final inventory; unobserved subtest total: unknown.'
                   if partial else 'Every selected file has an observed final inventory.'))
    upstream = html.escape(score["upstream_revision"], quote=True)
    source = html.escape(score["source_sha256"], quote=True)
    manifest = html.escape(score["manifest_sha256"], quote=True)
    return (
        '\n<p class="wpt-score"><strong>'
        f'{files["PASS"]} / 16 selected Window files passing</strong></p>\n'
        f'<p>File outcomes: Fail: {files["FAIL"]} · Timeouts: {files["TIMEOUT"]} · '
        f'Precondition failed: {files["PRECONDITION_FAILED"]} · Unsupported: {files["UNSUPPORTED"]} · '
        f'Errors: {files["ERROR"]} · Crashes: {files["CRASH"]} · Pending: {files["PENDING"]}.</p>\n'
        f'<p>Observed final subtests: Pass: {final["pass"]} / {final["total"]} observed final rows; '
        f'Fail: {final["fail"]} · Timeouts: {final["timeout"]} · Not run: {final["not_run"]} · '
        f'Precondition failed: {final["precondition_failed"]}.</p>\n'
        f'<p>{coverage} Source-declared calls: {summary["source_prospective_calls"]}; '
        'this is not a result denominator. Result prefixes without Completion '
        'are not counted as final inventory.</p>\n'
        '<p class="muted">A fixed upstream flexbox declared-value parsing cohort using '
        'the real Browser and unchanged Window testharness. These are current-source '
        'measurements, not a score for the installed release. The separate static '
        'reftest selection is not added to these totals. This is not the full WPT '
        'suite or complete Window compatibility.</p>\n'
        '<p><a href="https://github.com/pierce403/mgbrowser/blob/main/docs/WPT_WINDOW_PARSING.md">'
        'Window method and limitations ↗</a> · <a href="/wpt-window-results.json">'
        'Full Window report and tested commit ↗</a></p>\n'
        '<p class="muted">Case artifact paths and hashes in that report identify retained '
        'CI evidence; those paths are not public URLs.</p>\n'
        '<details><summary>Window reproducibility fingerprints</summary>\n'
        '<p>Upstream WPT: <a href="https://github.com/web-platform-tests/wpt/commit/'
        f'{upstream}"><code class="wpt-fingerprint">{upstream}</code></a></p>\n'
        f'<p>Window manifest SHA-256: <code class="wpt-fingerprint">{manifest}</code></p>\n'
        f'<p>Evaluated source SHA-256: <code class="wpt-fingerprint">{source}</code></p>\n'
        '</details>\n'
    )


def replace_region(document, generated):
    if document.count(START) != 1 or document.count(END) != 1:
        raise ValueError("Expected exactly one Window score marker pair")
    start = document.index(START) + len(START)
    end = document.index(END)
    if start > end:
        raise ValueError("Reversed Window score markers")
    return document[:start] + generated + document[end:]
