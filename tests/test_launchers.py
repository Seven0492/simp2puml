"""Lanceurs à double-cliquer : cohérence, syntaxe et démarrage depuis les sources."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

import pytest

from simp2puml import __version__

ROOT = Path(__file__).resolve().parents[1]
LAUNCHERS = ROOT / "launchers"


def pinned(path: Path, name: str) -> str:
    match = re.search(rf"\$?{name}\s*=\s*'?([0-9.]+)'?", path.read_text(encoding="utf-8-sig"))
    assert match, f"{name} absent de {path.name}"
    return match.group(1)


def test_shell_and_powershell_pin_the_same_versions():
    for sh_name, ps_name in (("UV_VERSION", "UvVersion"), ("PYTHON_VERSION", "PythonVersion")):
        assert pinned(LAUNCHERS / "bootstrap.sh", sh_name) == pinned(LAUNCHERS / "bootstrap.ps1", ps_name)


def test_entry_files_point_to_existing_bootstraps():
    cmd = (ROOT / "simp2puml-windows.cmd").read_text(encoding="ascii")
    assert 'launchers\\bootstrap.ps1' in cmd
    # Chemin rapide sans PowerShell : fichier écrit par bootstrap.ps1, lu par le .cmd.
    assert 'pythonw-cmd.txt' in cmd and 'launchers\\start.pyw' in cmd
    assert "'pythonw-cmd.txt'" in (LAUNCHERS / "bootstrap.ps1").read_text(encoding="utf-8-sig")
    for name in ("simp2puml-macos.command", "simp2puml-linux.sh"):
        text = (ROOT / name).read_text(encoding="utf-8")
        assert text.startswith("#!/bin/sh\n") and "launchers/bootstrap.sh" in text


def test_line_endings_suit_each_platform():
    assert b"\r\n" in (ROOT / "simp2puml-windows.cmd").read_bytes()
    assert (LAUNCHERS / "bootstrap.ps1").read_bytes().startswith(b"\xef\xbb\xbf")  # Accents sous PowerShell 5.1.
    for path in (LAUNCHERS / "bootstrap.sh", ROOT / "simp2puml-macos.command", ROOT / "simp2puml-linux.sh"):
        assert b"\r" not in path.read_bytes(), path.name


@pytest.mark.skipif(shutil.which("sh") is None, reason="sh indisponible")
def test_posix_bootstrap_parses():
    result = subprocess.run(["sh", "-n", str(LAUNCHERS / "bootstrap.sh")], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr


def test_posix_launchers_pass_shellcheck():
    shellcheck = shutil.which("shellcheck") or shutil.which("shellcheck", path=str(Path(sys.executable).parent))
    if shellcheck is None:
        pytest.skip("shellcheck indisponible")
    files = [LAUNCHERS / "bootstrap.sh", ROOT / "simp2puml-macos.command", ROOT / "simp2puml-linux.sh"]
    result = subprocess.run([shellcheck, "-s", "sh", *map(str, files)], capture_output=True, text=True)
    assert result.returncode == 0, result.stdout


def test_start_script_runs_from_sources(tmp_path):
    env = {k: v for k, v in os.environ.items() if k != "PYTHONPATH"}
    env["SIMP2PUML_LOG"] = str(tmp_path / "journal.log")
    result = subprocess.run([sys.executable, str(LAUNCHERS / "start.pyw"), "--version"], cwd=tmp_path, env=env,
                            capture_output=True, text=True, check=False)
    assert result.returncode == 0
    assert result.stdout.strip() == f"simp2puml-gui {__version__}"
    assert not (tmp_path / "journal.log").exists()
