#!/usr/bin/env python3
"""Verify the website and install its advertised release in a fresh user prefix.
Run from the repository root after publication: python3 tools/public-release-smoke.py 0.1.1
The prefix is retained under ignored tmp/ for inspection. Requires no Rust toolchain.
"""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

expected = sys.argv[1]
assert re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", expected)
repo = Path.cwd()
(repo / "tmp").mkdir(exist_ok=True)
root = Path(tempfile.mkdtemp(prefix="public-release-install-", dir=repo / "tmp"))
for name in ("index.html", "install.sh", "assets/mgbrowser.svg", "assets/mgbrowser-32.png", "assets/mgbrowser-256.png"):
    url = "https://mgbrowser.org/" + ("" if name == "index.html" else name)
    body = subprocess.check_output(["curl", "-fsSL", url])
    assert body == (repo / name).read_bytes(), name
    print("PUBLIC_BYTES_MATCH", name, len(body), hashlib.sha256(body).hexdigest(), flush=True)
command = "curl -fsSL https://mgbrowser.org/install.sh | bash"
assert command in (repo / "index.html").read_text()
assert f"v{expected} Experimental Preview" in (repo / "index.html").read_text()
env = dict(os.environ, HOME=str(root), XDG_DATA_HOME=str(root / ".local/share"))
env.pop("MGBROWSER_INSTALL_DIR", None)
subprocess.run(["bash", "-o", "pipefail", "-c", command], env=env, check=True, timeout=180)
binary = root / ".local/bin/mgbrowser"
assert subprocess.check_output([binary, "--version"], text=True).strip() == f"mgbrowser {expected}"
with subprocess.Popen([binary, "--script-worker"], stdin=subprocess.PIPE, env=env) as worker:
    try:
        assert worker.poll() is None
        before = binary.stat().st_ino
        subprocess.run(["bash", "-o", "pipefail", "-c", command], env=env, check=True, timeout=180)
        assert binary.stat().st_ino != before
        assert worker.poll() is None
    finally:
        worker.terminate()
        worker.wait(timeout=3)
subprocess.run([binary, "--script-worker-selftest"], env=env, check=True, timeout=20)
assert subprocess.check_output([binary, "--version"], text=True).strip() == f"mgbrowser {expected}"
data = root / ".local/share"
desktop = data / "applications/mgbrowser.desktop"
assert f'Exec="{binary}" %u' in desktop.read_text()
if shutil.which("desktop-file-validate"):
    subprocess.run(["desktop-file-validate", desktop], check=True)
for name in ("icons/hicolor/scalable/apps/mgbrowser.svg", "icons/hicolor/256x256/apps/mgbrowser.png", "mgbrowser/LICENSE", "mgbrowser/THIRD_PARTY_LICENSES.txt"):
    assert (data / name).stat().st_size > 0
release_url = subprocess.check_output(["curl", "-fsSL", "-o", "/dev/null", "-w", "%{url_effective}", "https://github.com/pierce403/mgbrowser/releases/latest"], text=True)
assert release_url == f"https://github.com/pierce403/mgbrowser/releases/tag/v{expected}", release_url
subprocess.run(["curl", "-fsSL", "-o", "/dev/null", "https://github.com/pierce403/mgbrowser/releases/latest/download/mgbrowser-linux-x86_64.tar.gz"], check=True)
print("PUBLIC_INSTALL_AND_UPDATE_OK", expected, root, flush=True)
