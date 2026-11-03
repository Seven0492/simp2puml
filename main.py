"""Point d'entrée historique ; préférer la commande simp2puml."""
import sys
from pathlib import Path

# Utiliser les sources locales pour conserver l'exécution sans installation.
sys.path.insert(0, str(Path(__file__).resolve().parent / "src"))

from simp2puml.cli import main

if __name__ == "__main__":
    raise SystemExit(main())
