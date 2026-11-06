"""Préférences communes aux deux interfaces, dans un même fichier `clé = valeur`.

La nouvelle interface (`editor/src/model/preferences.rs`) relit et réécrit ce fichier en entier ;
ici, seule la clé `interface` est lue et remplacée, en conservant tout le reste.
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import stat
import sys
import tempfile

EDITOR = "editor"
CLASSIC = "classic"
INTERFACES = (EDITOR, CLASSIC)
DEFAULT_INTERFACE = EDITOR
_LINE = re.compile(r"\s*interface\s*=\s*(\S*)\s*$")
_HEADER = "# Préférences de simp2puml\n# interface : editor (nouvelle) ou classic\n"


def path() -> Path:
    """Même emplacement que `preferences_service.rs`."""
    configured = os.environ.get("XDG_CONFIG_HOME")
    if configured and Path(configured).is_absolute():
        base = Path(configured)
    elif sys.platform == "win32":
        base = Path(os.environ.get("APPDATA") or Path.home() / "AppData" / "Roaming")
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path.home() / ".config"
    return base / "simp2puml" / "preferences.conf"


def parse_interface(text: str) -> str:
    """Dernière valeur valide de `interface` ; une valeur inconnue est ignorée."""
    choice = DEFAULT_INTERFACE
    for line in text.splitlines():
        match = _LINE.fullmatch(line)
        if match and match.group(1) in INTERFACES:
            choice = match.group(1)
    return choice


def with_interface(text: str, choice: str) -> str:
    """Texte des préférences avec ce choix ; commentaires et autres réglages sont conservés."""
    if choice not in INTERFACES:
        raise ValueError(f"interface inconnue : {choice}")
    lines = [line for line in text.splitlines() if not _LINE.fullmatch(line)]
    if not lines:
        lines = _HEADER.splitlines()
    return "\n".join([*lines, f"interface = {choice}"]) + "\n"


def interface() -> str:
    """Interface choisie ; un fichier absent ou illisible ne doit jamais empêcher le lancement."""
    try:
        return parse_interface(path().read_text(encoding="utf-8"))
    except (OSError, UnicodeError):
        return DEFAULT_INTERFACE


def set_interface(choice: str) -> None:
    """Enregistre le choix par remplacement atomique (un lien symbolique reste un lien)."""
    target = path().resolve()
    try:
        current = target.read_text(encoding="utf-8")
        mode = stat.S_IMODE(target.stat().st_mode)
    except FileNotFoundError:
        current, mode = "", None
    content = with_interface(current, choice)
    target.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(dir=target.parent, prefix=f".{target.name}.", suffix=".tmp")
    try:
        with os.fdopen(handle, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        if mode is not None:
            os.chmod(temporary, mode)
        else:
            os.chmod(temporary, 0o644)
        os.replace(temporary, target)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise
