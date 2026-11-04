"""Lance l'interface graphique depuis les sources, sans installation (utilisé par les lanceurs).

Avec `SIMP2PUML_LOG`, une erreur de démarrage est écrite dans ce fichier : sous Windows,
pythonw n'a pas de console pour l'afficher.
"""
import os
import sys
import traceback
from pathlib import Path

# Les sources voisines priment sur une éventuelle version installée de simp2puml.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))


def _report(details: str) -> None:
    log = os.environ.get("SIMP2PUML_LOG")
    if log:
        try:
            Path(log).write_text(details, encoding="utf-8")
        except OSError:
            pass
    if sys.stderr is not None:
        print(details, file=sys.stderr)


if __name__ == "__main__":
    try:
        from simp2puml.gui import main

        code = main()
    except Exception:  # Démarrage impossible : garder une trace lisible.
        _report(traceback.format_exc())
        code = 1
    else:
        if code:
            _report(f"simp2puml-gui s'est terminé avec le code {code}.")
    raise SystemExit(code)
