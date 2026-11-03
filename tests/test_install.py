"""Vérifications légères des points d'entrée, sans construction de wheel."""
import os
from pathlib import Path
import subprocess
import sys
import tomllib

from simp2puml import __version__


ROOT = Path(__file__).resolve().parents[1]


def run_entrypoint(args, cwd):
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT / "src")
    return subprocess.run([sys.executable, *args], cwd=cwd, env=env,
                          capture_output=True, text=True, check=False)


def test_module_version_and_help(tmp_path):
    result = run_entrypoint(["-m", "simp2puml", "--version"], tmp_path)
    assert result.returncode == 0
    assert result.stdout.strip() == f"simp2puml {__version__}"
    result = run_entrypoint(["-m", "simp2puml", "--help"], tmp_path)
    assert result.returncode == 0
    assert "SIMP en PlantUML" in result.stdout
    assert "expérimentale" in result.stdout


def test_manpage_is_configured_for_installation():
    with (ROOT / "pyproject.toml").open("rb") as config_file:
        config = tomllib.load(config_file)

    manpage = ROOT / "docs" / "simp2puml.1"
    assert config["tool"]["setuptools"]["data-files"]["share/man/man1"] == [
        "docs/simp2puml.1"
    ]
    assert manpage.is_file()
    assert manpage.read_text(encoding="utf-8").splitlines()[0] == ".TH SIMP2PUML 1"


def test_legacy_wrapper_without_installation(tmp_path):
    env = os.environ.copy()
    env.pop("PYTHONPATH", None)
    result = subprocess.run([sys.executable, str(ROOT / "main.py"), "--version"],
                            cwd=tmp_path, env=env, capture_output=True, text=True, check=False)
    assert result.returncode == 0
    assert result.stdout.strip() == f"simp2puml {__version__}"
