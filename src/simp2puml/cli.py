"""CLI de conversion entre SIMP et PlantUML, avec export vers le DSL Atelier Diagrammes."""
import argparse
from collections.abc import Callable
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

from . import __version__
from .atelier import write_atelier
from .model import ConversionError, UMLDiagram
from .puml import parse_puml, write_puml
from .simp import list_diagrams, parse_simp, write_simp

MAX_INPUT_SIZE = 20 * 1024 * 1024
EXPERIMENTAL_WARNING = "La génération SIMP est expérimentale et sa compatibilité n'est pas validée."
SOURCE_SUFFIXES = (".simp", ".puml")
TARGETS = {"puml": ".puml", "simp": ".simp", "atelier": ".atelier"}


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="simp2puml",
        description=("Convertit des diagrammes SIMP en PlantUML ; la conversion inverse est expérimentale. "
                     "Les deux formats s'exportent aussi vers le DSL Typst Atelier Diagrammes (diagramme-ad)."),
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.add_argument("source", type=Path, help="fichier source .simp ou .puml")
    parser.add_argument("-o", "--output", type=Path, help="chemin de sortie (extension du format cible requise)")
    parser.add_argument("--to", choices=sorted(TARGETS),
                        help="format cible : puml, simp ou atelier (défaut : format opposé, "
                             "ou extension de --output)")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--diagram", help="nom ou identifiant du diagramme (tout type SIMP)")
    selection.add_argument("--all", action="store_true", help="convertir tous les diagrammes SIMP en blocs PlantUML distincts")
    selection.add_argument("--list-diagrams", action="store_true", help="lister les identifiants, noms et types de diagrammes SIMP")
    parser.add_argument("--check", action="store_true", help="vérifier la conversion sans écrire de fichier")
    parser.add_argument("--strict", action="store_true", help="refuser la conversion en présence d'avertissements")

    parser.epilog = "La taille maximale du fichier source est de 20 MiB."
    return parser


def _same_file(source: Path, output: Path) -> bool:
    if source.resolve() == output.resolve():
        return True
    try:
        return os.path.samefile(source, output)
    except FileNotFoundError:
        return False


def _read_source(source: Path) -> str:
    if not stat.S_ISREG(source.stat().st_mode):
        raise ConversionError("la source doit être un fichier ordinaire")
    with source.open("rb") as stream:
        if os.fstat(stream.fileno()).st_size > MAX_INPUT_SIZE:
            raise ConversionError("fichier source trop volumineux (maximum : 20 MiB)")
        data = stream.read(MAX_INPUT_SIZE + 1)
    if len(data) > MAX_INPUT_SIZE:
        raise ConversionError("fichier source trop volumineux (maximum : 20 MiB)")
    return data.decode("utf-8-sig")


def _publish(output: Path, content: str | bytes) -> None:
    """Publie atomiquement ; `os.link` échoue si la sortie existe, sans jamais la remplacer."""
    temp_path = None
    text_mode = {} if isinstance(content, bytes) else {"encoding": "utf-8", "newline": ""}
    try:
        with tempfile.NamedTemporaryFile(mode="wb" if isinstance(content, bytes) else "w", **text_mode,
                                         dir=output.parent, prefix=f".{output.name}.", suffix=".tmp",
                                         delete=False) as temp:
            temp_path = Path(temp.name)
            temp.write(content)
            temp.flush()
            os.fsync(temp.fileno())
        os.link(temp_path, output)
    finally:
        if temp_path is not None:
            try:
                temp_path.unlink()
            except FileNotFoundError:
                pass


def convert(text: str, suffix: str, target: str, selectors: list[str | None],
            report: Callable[[UMLDiagram], None] | None = None) -> tuple[str, list[UMLDiagram]]:
    """Convertit en mémoire chaque sélection et renvoie le contenu joint avec les diagrammes lus.

    `suffix` est l'extension source (`.simp` ou `.puml`) et `target` une clé de `TARGETS`.
    `report` est appelé après chaque diagramme, avec ses avertissements renseignés.
    Aucune écriture : la publication reste à la charge de l'appelant.
    """
    diagrams: list[UMLDiagram] = []
    contents: list[str] = []
    for selector in selectors:
        diagram = parse_simp(text, diagram=selector) if suffix == ".simp" else parse_puml(text)
        diagrams.append(diagram)
        if target == "atelier":
            contents.append(write_atelier(diagram))
        elif target == "puml":
            contents.append(write_puml(diagram))
        else:
            contents.append(write_simp(diagram))
            diagram.warnings.append(EXPERIMENTAL_WARNING)
        if report is not None:
            report(diagram)
    return "\n".join(contents), diagrams


def summary_lines(diagrams: list[UMLDiagram]) -> list[str]:
    """Décomptes indicatifs d'une conversion ; ce n'est pas un inventaire des pertes."""
    lines: list[str] = []
    classes = [cls for d in diagrams for cls in d.classes]
    views = [d.source_view for d in diagrams if d.source_view is not None]
    elements = [e for v in views for e in v.elements.values()]
    lines.append(f"Classes : {sum(cls.kind == 'class' for cls in classes) + sum(e.get('type') == 'class' for e in elements)}")
    relation_count = sum(v.relationship_count for v in views)
    lines.append(f"Relations : {sum(len(d.relationships) for d in diagrams) + relation_count}")
    for kind, label in (('interface', 'Interfaces'), ('enumeration', 'Énumérations'),
                        ('interface_symbol', 'Symboles d’interface')):
        source_kind = {'interface': 'uml-interface-classifier', 'enumeration': 'enumeration', 'interface_symbol': 'uml-interface'}[kind]
        count = sum(cls.kind == kind for cls in classes) + sum(e.get('type') == source_kind for e in elements)
        if count:
            lines.append(f"{label} : {count}")
    note_count = sum(len(d.notes) for d in diagrams) + sum(e.get('type') == 'comment' for e in elements)
    if note_count:
        lines.append(f"Notes : {note_count}")
    if views:
        lines.append(f"Éléments des vues enrichies : {sum(len(v.elements) for v in views)}")
    lines.append(f"Avertissements : {sum(len(d.warnings) for d in diagrams)}")
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    source: Path = args.source
    suffix = source.suffix.lower()
    if suffix not in SOURCE_SUFFIXES:
        parser.error("l'extension source doit être .simp ou .puml")
    if (args.diagram is not None or args.all or args.list_diagrams) and suffix != ".simp":
        parser.error("--diagram, --all et --list-diagrams ne s'appliquent qu'aux sources .simp")
    if args.list_diagrams:
        if args.output or args.to or args.check or args.strict:
            parser.error("--list-diagrams ne s'utilise pas avec --output, --to, --check ou --strict")
        try:
            for ident, name, kind in list_diagrams(_read_source(source)):
                # JSON rend les tabulations et retours de ligne non ambigus.
                print(json.dumps({'id': ident, 'name': name, 'type': kind}, ensure_ascii=False))
        except (OSError, UnicodeError, ConversionError) as exc:
            print(f"Erreur : {exc}", file=sys.stderr)
            return 1
        return 0
    if args.check and args.output is not None:
        parser.error("--check ne s'utilise pas avec --output")
    target = args.to
    if target is None and args.output is not None:
        target = next((name for name, ext in TARGETS.items()
                       if ext == args.output.suffix.lower() and ext != suffix), None)
    if target is None:
        target = "puml" if suffix == ".simp" else "simp"
    expected_suffix = TARGETS[target]
    if args.all and target != "puml":
        parser.error("--all exige une sortie PlantUML")
    if expected_suffix == suffix:
        parser.error("le format cible doit différer du format source")
    output: Path = args.output if args.output is not None else source.with_suffix(expected_suffix)
    if output.suffix.lower() != expected_suffix:
        parser.error(f"l'extension de sortie doit être {expected_suffix}")
    try:
        if not args.check:
            if _same_file(source, output):
                raise ConversionError("la sortie ne peut pas être le fichier source")
            if output.exists() or output.is_symlink():
                raise ConversionError(f"la sortie existe déjà : {output}")
        text = _read_source(source)
        selectors = [ident for ident, _, _ in list_diagrams(text)] if args.all else [args.diagram]

        def report(diagram: UMLDiagram) -> None:
            for warning in diagram.warnings:
                context = f"{diagram.name} : " if args.all else ""
                print(f"Avertissement : {context}{warning}", file=sys.stderr)

        content, diagrams = convert(text, suffix, target, selectors, report)
        if args.strict and any(d.warnings for d in diagrams):
            raise ConversionError("mode strict : conversion refusée en présence d'avertissements")
        if not args.check:
            _publish(output, content)
    except (OSError, UnicodeError, ConversionError) as exc:
        print(f"Erreur : {exc}", file=sys.stderr)
        return 1

    print("Vérification terminée (aucun fichier écrit)" if args.check else "Conversion terminée")
    if args.all:
        print(f"Diagrammes : {len(diagrams)}")
    for line in summary_lines(diagrams):
        print(line)
    if not args.check:
        print(f"Sortie : {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
