"""Lancement des deux interfaces graphiques, qui partagent préférences et logique de conversion.

`simp2puml-gui` aiguille vers l'interface choisie. La nouvelle (binaire `puml-editor`) n'est pas
livrée avec le paquet Python : si elle est absente, l'interface classique prend le relais.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

EDITOR_ENV = "PUML_EDITOR"
_EXE = "puml-editor.exe" if sys.platform == "win32" else "puml-editor"
# Un échec de démarrage se voit vite ; au-delà, la fenêtre est en train de s'ouvrir.
START_CHECK_SECONDS = 0.6


def find_editor() -> Path | None:
    """$PUML_EDITOR, à côté du script lancé, dans le dépôt, puis entrées absolues du PATH."""
    configured = os.environ.get(EDITOR_ENV)
    if configured:
        path = Path(configured)
        return path if path.is_file() else None
    # Le dossier du lien d'abord (~/.local/bin), puis celui de sa cible : le menu des
    # applications ne donne pas forcément ~/.local/bin dans le PATH.
    script = Path(sys.argv[0]) if sys.argv and sys.argv[0] else None
    candidates = [script.absolute().parent / _EXE, script.resolve().parent / _EXE] if script else []
    candidates.append(Path(__file__).resolve().parents[2] / "editor" / "target" / "release" / _EXE)
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    absolute = os.pathsep.join(d for d in os.environ.get("PATH", "").split(os.pathsep) if Path(d).is_absolute())
    found = shutil.which(_EXE, path=absolute)
    return Path(found) if found else None


def _console_python() -> str:
    """pythonw n'a pas de sortie d'erreur : l'éditeur a besoin des messages du convertisseur."""
    executable = Path(sys.executable)
    console = executable.with_name(executable.name.replace("pythonw", "python"))
    return str(console if console.is_file() else executable)


def editor_environment() -> dict[str, str]:
    """Donne à l'éditeur le Python et le paquet en cours : la même logique de conversion, même sans PATH."""
    environment = dict(os.environ, SIMP2PUML_PYTHON=_console_python())
    package_root = str(Path(__file__).resolve().parents[1])
    existing = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = os.pathsep.join([package_root, existing] if existing else [package_root])
    return environment


def replace_process(editor: Path, files: list[str]) -> bool:
    """Devient l'éditeur (lancement depuis le menu) ; `False` si son démarrage est impossible."""
    argv = [str(editor), *files]
    try:
        if sys.platform == "win32":
            subprocess.Popen(argv, env=editor_environment())
            return True
        os.execve(str(editor), argv, editor_environment())
    except OSError:
        return False
    return True  # pragma: no cover - execve ne revient qu'en cas d'erreur


def start_editor(editor: Path, files: list[str]) -> str | None:
    """Démarre l'éditeur à côté de la fenêtre actuelle ; renvoie le message d'erreur, sinon None."""
    try:
        process = subprocess.Popen([str(editor), *files], env=editor_environment(), stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True)
    except OSError as exc:
        return f"impossible de lancer {editor} : {exc}"
    deadline = time.monotonic() + START_CHECK_SECONDS
    while time.monotonic() < deadline:
        code = process.poll()
        if code is not None:
            if code == 0:
                return None
            detail = process.stderr.read().decode("utf-8", "replace").strip() if process.stderr else ""
            return detail or f"la nouvelle interface s'est arrêtée (code {code})"
        time.sleep(0.02)
    return None
