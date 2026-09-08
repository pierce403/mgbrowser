#!/usr/bin/env python3
"""Install the packaged payload in isolated prefixes, including bad checksum rejection."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

assets = Path(sys.argv[1]).resolve()
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
        assert version == "mgbrowser 0.1.0", version
        subprocess.run([binary, "--script-worker-selftest"], env=env, check=True, timeout=20)
        subprocess.run([binary, "--help"], env=dict(env, MGBROWSER_FONT="/missing-font"), check=True)
        data = home / ".local/share"
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
print("RELEASE_INSTALL_SMOKE_OK: default/custom paths, version, worker, desktop/icon, bad checksum")
