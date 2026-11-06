"""Point d'entrée `simp2puml-gui` : ouvre l'interface choisie dans les préférences.

Aucun import lourd ici : l'interface classique et le convertisseur ne sont chargés que si
elle est ouverte, pour ne pas retarder la nouvelle interface, qui est celle par défaut.
"""
from __future__ import annotations

import sys

from . import __version__, interfaces, preferences

USAGE = """Usage : simp2puml-gui [--interface editor|classic] [fichier.simp|fichier.puml]...

Ouvre l'interface choisie dans les préférences (la nouvelle par défaut). Si elle n'est pas
installée, l'interface classique prend le relais ; celle-ci n'ouvre que le premier fichier."""


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if args and args[0] in ("-h", "--help"):
        print(USAGE)
        return 0
    if args and args[0] == "--version":
        print(f"simp2puml-gui {__version__}")
        return 0
    forced = None
    if args and args[0] == "--interface":
        if len(args) < 2 or args[1] not in preferences.INTERFACES:
            print(USAGE, file=sys.stderr)
            return 2
        forced = args[1]
        args = args[2:]
    if (forced or preferences.interface()) == preferences.EDITOR:
        editor = interfaces.find_editor()
        if editor is not None and interfaces.replace_process(editor, args):
            return 0
        if forced:
            print("Erreur : la nouvelle interface n'est pas disponible (binaire puml-editor).", file=sys.stderr)
            return 1
    from . import gui  # Chargé seulement ici : voir la docstring du module.

    return gui.main(args)


if __name__ == "__main__":
    sys.exit(main())
