#!/usr/bin/env python3
"""Install the packaged payload in isolated prefixes, including bad checksum rejection."""
import os
import json
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def verify_boa_worker(binary):
    """Exercise the installed default engine, not an optional legacy build."""
    request = {"url": "https://example.test/installed-boa", "html": """
<!doctype html><html><head><title>Readable fallback</title></head><body>
<script>
const key = {};
const values = new Map([[key, 40]]);
class Counter { constructor(value) { this.value = value; } answer() { return this.value + 2; } }
Promise.resolve(new Counter(values.get(key)).answer()).then(answer => {
  const proof = document.createElement('p');
  proof.id = 'boa-installed-proof'; proof.textContent = `Installed Boa ${answer}`;
  document.body.appendChild(proof); document.title = 'Installed Boa ready';
});
</script></body></html>"""}
    result = subprocess.run([binary, "--script-worker"],
                            input=json.dumps(request).encode(), env={},
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            check=True, timeout=5)
    assert len(result.stdout) <= 4 * 1024 * 1024 and len(result.stderr) <= 65536
    reply = json.loads(result.stdout)
    assert reply["applied"] and not reply["errors"] and reply["scripts_executed"] == 1, reply
    assert "Installed Boa 42" in reply["html"] and "Installed Boa ready" in reply["html"], reply
    assert reply["boa"]["jobs_executed"] >= 1 and reply["boa"]["fatal_reason"] is None, reply
    assert reply["allocations"] is None, "Boa must not masquerade as original allocation accounting"
    subprocess.run([binary, "--script-session-selftest"], env={}, check=True, timeout=20)


assets = Path(sys.argv[1]).resolve()
expected_version = re.search(r'^version = "([^"]+)"', Path("Cargo.toml").read_text(), re.M).group(1)
assert f"v{expected_version} Experimental Preview" in Path("index.html").read_text()
with tempfile.TemporaryDirectory(prefix="mgbrowser-release-", dir="tmp") as temporary:
    root = Path(temporary).resolve()
    transport = root / "transport"
    transport.mkdir()
    shutil.copy("tools/fixtures/release-curl", transport / "curl")
    (transport / "curl").chmod(0o755)
    env = dict(os.environ, PATH=f"{transport}:{os.environ['PATH']}",
               MGBROWSER_TEST_ASSETS=str(assets))
    env.pop("MGBROWSER_INSTALL_DIR", None)
    for name, custom in [("default", False), ("space prefix", True)]:
        home = root / name
        env.update(HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"))
        binary_dir = home / ("custom bin" if custom else ".local/bin")
        if custom:
            env["MGBROWSER_INSTALL_DIR"] = str(binary_dir)
        subprocess.run(["bash", "install.sh"], env=env, check=True)
        binary = binary_dir / "mgbrowser"
        version = subprocess.check_output([binary, "--version"], env=env, text=True).strip()
        assert version == f"mgbrowser {expected_version}", version
        about = subprocess.check_output([binary, "--about"], env=env, text=True)
        assert f"mgbrowser {expected_version}" in about and "Compiled:" in about and "GMT" in about and "Commit:" in about
        assert (binary_dir / ".mgbrowser-auto-update").read_text() == "enabled\n"
        # Reinstall while the old executable is running: replacing its inode
        # must work without truncating a live binary (ETXTBSY).
        old_inode = binary.stat().st_ino
        with subprocess.Popen([binary, "--script-worker"], stdin=subprocess.PIPE, env=env) as worker:
            try:
                assert worker.poll() is None
                subprocess.run(["bash", "install.sh"], env=env, check=True)
                assert binary.stat().st_ino != old_inode
                assert worker.poll() is None
            finally:
                worker.terminate()
                worker.wait(timeout=3)
        # Open old processes must restart after replacement; test the new path.
        subprocess.run([binary, "--script-worker-selftest"], env=env, check=True, timeout=20)
        verify_boa_worker(binary)
        assert subprocess.check_output([binary, "--version"], text=True).strip() == version
        subprocess.run([binary, "--help"], env=dict(env, MGBROWSER_FONT="/missing-font"), check=True)
        data = home / ".local/share"
        assert (data / "mgbrowser/LICENSE").read_bytes() == Path("LICENSE").read_bytes()
        assert (data / "mgbrowser/NOTICE").read_bytes() == Path("NOTICE").read_bytes()
        desktop = data / "applications/mgbrowser.desktop"
        assert f'Exec="{binary}" %u' in desktop.read_text()
        assert (data / "icons/hicolor/scalable/apps/mgbrowser.svg").stat().st_size > 0
        assert (data / "icons/hicolor/256x256/apps/mgbrowser.png").stat().st_size > 0
        if shutil.which("desktop-file-validate"):
            subprocess.run(["desktop-file-validate", desktop], check=True)
    bad = root / "bad-assets"
    bad.mkdir()
    shutil.copy(assets / "mgbrowser-linux-x86_64.tar.gz", bad)
    (bad / "mgbrowser-linux-x86_64.tar.gz.sha256").write_text(
        "0" * 64 + "  mgbrowser-linux-x86_64.tar.gz\n")
    rejected = root / "must-not-install"
    env.update(MGBROWSER_TEST_ASSETS=str(bad), MGBROWSER_INSTALL_DIR=str(rejected))
    result = subprocess.run(["bash", "install.sh"], env=env)
    assert result.returncode != 0 and not rejected.exists()
print("RELEASE_INSTALL_SMOKE_OK: default/custom paths, version, running-binary update, Boa page/Promise/session, worker, desktop/icon, bad checksum")
