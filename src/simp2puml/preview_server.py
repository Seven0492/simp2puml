"""Aperçu de la nouvelle interface par le moteur officiel PlantUML.

Parle le protocole de `pumlrender --serve`, en réutilisant `render.py` : mêmes moteurs, même
profil SANDBOX et mêmes vérifications que l'interface classique. Le texte reçu est déjà résolu
par l'éditeur : ce processus n'a aucun fichier à lire. `--install` installe le moteur.
"""
from __future__ import annotations

import itertools
import re
import sys
from typing import BinaryIO
import xml.etree.ElementTree as ET

from . import __version__, render

KIND = "plantuml officiel"
_LINE = re.compile(r"ligne (\d+)")
_SVG = "{http://www.w3.org/2000/svg}"
_NUMBER = re.compile(r"-?\d+(?:\.\d+)?")
_PATH_TOKEN = re.compile(r"[A-Za-z]|-?\d+(?:\.\d+)?")
NOT_INSTALLED = "moteur PlantUML non installé : Préférences > Installer PlantUML"


def _clean(text: str) -> str:
    return " ".join(text.split())


def _start_lines(text: str) -> list[int]:
    """Numéro (à partir de 1) de la ligne @start de chaque bloc."""
    lines = [i + 1 for i, line in enumerate(text.splitlines()) if line.strip().lower().startswith("@start")]
    return lines or [1]


def _box(element: ET.Element) -> tuple[float, float, float, float] | None:
    """Boîte englobante d'un groupe SVG, d'après les formes qu'il contient."""
    xs, ys = [], []
    for node in element.iter():
        tag = node.tag.removeprefix(_SVG)
        get = lambda name: float(node.get(name, "0"))  # noqa: E731,B023 - utilisé aussitôt
        if tag == "rect":
            xs += [get("x"), get("x") + get("width")]
            ys += [get("y"), get("y") + get("height")]
        elif tag == "ellipse":
            xs += [get("cx") - get("rx"), get("cx") + get("rx")]
            ys += [get("cy") - get("ry"), get("cy") + get("ry")]
        elif tag in ("polygon", "path"):
            values = [float(v) for v in _NUMBER.findall(node.get("points") or node.get("d") or "")]
            # Les arcs mêlent rayons et drapeaux aux coordonnées : ignorés, le reste suffit à la boîte.
            if tag == "polygon" or "A" not in node.get("d", ""):
                xs += values[0::2]
                ys += values[1::2]
    if not xs or not ys:
        return None
    return min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)


def _normalized(text: str) -> str:
    return " ".join(text.replace("\u00a0", " ").split())


def _text_box(node: ET.Element, box_x: float | None, box_w: float | None) -> tuple[float, float, float, float]:
    """Boîte d'un texte ; dans un élément, toute sa largeur, comme une ligne de membre."""
    x, y = float(node.get("x", "0")), float(node.get("y", "0"))
    size = float(node.get("font-size", "13"))
    if box_x is not None and box_w is not None:
        return box_x, y - size, box_w, size * 1.3
    width = float(node.get("textLength") or len(node.text or "") * size * 0.6)
    return x, y - size, width, size * 1.3


class _SourceFinder:
    """Retrouve la ligne d'un texte affiché : PlantUML ne note que la ligne des éléments entiers."""

    def __init__(self, text: str):
        self.lines = [_normalized(line) for line in text.splitlines()]
        self.cursor = 0

    def in_element(self, shown: str, start: int) -> int | None:
        """Dans le corps d'un élément (jusqu'à son « } »), sinon une ligne « Nom : membre » ailleurs."""
        for index in range(start + 1, len(self.lines)):
            if shown in self.lines[index]:
                return index + 1
            if self.lines[index].startswith("}"):
                break
        for index, line in enumerate(self.lines):
            if " : " in line and shown in line.split(" : ", 1)[1]:
                return index + 1
        return None

    def in_order(self, shown: str) -> int | None:
        """Sans élément noté (activité, carte, Gantt…) : dans l'ordre du texte, puis n'importe où."""
        for index in [*range(self.cursor, len(self.lines)), *range(self.cursor)]:
            if shown in self.lines[index]:
                self.cursor = index
                return index + 1
        return None


def _flatten(path: str) -> list[tuple[float, float]]:
    """Points d'un tracé M/L/H/V/C/Q absolu, les courbes approchées par des segments."""
    tokens = _PATH_TOKEN.findall(path)
    points: list[tuple[float, float]] = []
    index, command = 0, ""

    def numbers(count: int) -> list[float]:
        nonlocal index
        values = [float(v) for v in tokens[index:index + count]]
        index += count
        return values

    def curve(controls: list[tuple[float, float]]) -> None:
        start = points[-1]
        for step in range(1, 13):
            t = step / 12
            nodes = [start, *controls]
            while len(nodes) > 1:
                nodes = [(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) for a, b in zip(nodes, nodes[1:])]
            points.append(nodes[0])

    while index < len(tokens):
        if tokens[index].isalpha():
            command = tokens[index]
            index += 1
        if command in ("M", "L"):
            x, y = numbers(2)
            points.append((x, y))
        elif command == "H" and points:
            points.append((numbers(1)[0], points[-1][1]))
        elif command == "V" and points:
            points.append((points[-1][0], numbers(1)[0]))
        elif command == "C" and points:
            v = numbers(6)
            curve([(v[0], v[1]), (v[2], v[3]), (v[4], v[5])])
        elif command == "Q" and points:
            v = numbers(4)
            curve([(v[0], v[1]), (v[2], v[3])])
        else:
            break  # Commande inattendue : garder le début du tracé.
    return points


def regions_and_links(svg: bytes, first_line: int, text: str = "") -> tuple[list, list]:
    """Zones cliquables et tracés des flèches. PlantUML note la ligne de chaque élément (depuis
    @start, à partir de 0) ; celle de ses textes (attributs, méthodes…) est retrouvée dans le source."""
    try:
        root = ET.fromstring(svg)
    except ET.ParseError:
        return [], []
    found: list = []
    links: list = []
    finder = _SourceFinder(text)
    groups = itertools.count()

    def visit(element: ET.Element, owner: int | None, box: tuple[float, float, float, float] | None) -> None:
        for child in element:
            tag = child.tag.removeprefix(_SVG)
            if tag == "g":
                line = child.get("data-source-line")
                if line is not None and line.isdigit():
                    source = first_line - 1 + int(line)  # indice dans le texte (à partir de 0)
                    is_link = child.get("class") == "link"
                    try:
                        own_box = None if is_link else _box(child)
                    except ValueError:
                        own_box = None
                    if own_box is not None:
                        found.append((source + 1, *own_box))
                    if is_link:
                        group = next(groups)  # rang du groupe : l'éditeur recolore ses formes au survol
                        # le premier tracé est le trait ; les suivants, les pointes
                        path = next(child.iter(f"{_SVG}path"), None)
                        if path is not None:
                            points = _flatten(path.get("d") or "")
                            if len(points) >= 2:
                                links.append((source + 1, len(found), points, group))
                    visit(child, source, own_box)
                else:
                    visit(child, owner, box)
            elif tag == "text":
                shown = _normalized("".join(child.itertext()))
                if len(shown) < 2 and owner is None:
                    continue  # « 1 », « 2 » : trop courts pour être retrouvés sans contexte
                line = finder.in_element(shown, owner) if owner is not None else finder.in_order(shown)
                if line is not None:
                    is_member = owner is not None and line - 1 != owner
                    found.append((line, *_text_box(child, box[0] if is_member and box else None,
                                                   box[2] if is_member and box else None)))
            else:
                visit(child, owner, box)

    try:
        visit(root, None, None)
    except ValueError:
        pass  # Nombre inattendu : garder ce qui est déjà trouvé.
    return found, links


def _error_page(message: str) -> bytes:
    from xml.sax.saxutils import escape

    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="640" height="60" viewBox="0 0 640 60">'
            f'<text x="10" y="30" font-size="13" fill="#B00020">{escape(message[:90])}</text></svg>').encode()


def reply(rid: str, rendering: render.Rendering, block: int, text: str) -> bytes:
    """Réponse du protocole pour le bloc demandé (le dernier s'il a disparu)."""
    pages = rendering.pages
    index = min(block, max(len(pages) - 1, 0))
    starts = _start_lines(text)
    first_line = starts[min(index, len(starts) - 1)]
    svg = pages[index] if pages else _error_page(rendering.error or "aucun rendu")
    width, height = render.svg_size(svg) or (0, 0)
    head = ["PUMLRENDER 1", f"ID\t{rid}", f"KIND\t{KIND}", f"BLOCKS\t{max(len(pages), 1)}", f"SIZE\t{width}\t{height}"]
    if rendering.error:
        match = _LINE.search(rendering.error)
        head.append(f"DIAG\t{match.group(1) if match else first_line}\terror\t{_clean(rendering.error)}")
    found, links = regions_and_links(svg, first_line, text)
    for line, x, y, w, h in found:
        head.append(f"REGION\t{line}\t{x:g}\t{y:g}\t{w:g}\t{h:g}")
    for line, after, points, group in links:
        path = " ".join(f"{x:g} {y:g}" for x, y in points)
        head.append(f"LINK\t{line}\t{after}\t{path}\t{group}")
    head.append(f"SVG\t{len(svg)}")
    return ("\n".join(head) + "\n").encode() + svg


def _failure(rid: str, message: str) -> bytes:
    return f"PUMLRENDER 1\nID\t{rid}\nERROR\t{_clean(message)}\n".encode()


def serve(stdin: BinaryIO, stdout: BinaryIO) -> int:
    """Une requête `RENDER <id> <bloc> <octets>` suivie du texte, jusqu'à la fin de l'entrée."""
    renderer = None
    while True:
        header = stdin.readline()
        if not header:
            break
        parts = header.decode("utf-8", "replace").split()
        if len(parts) != 4 or parts[0] != "RENDER" or not parts[2].isdigit() or not parts[3].isdigit():
            stdout.write(_failure(parts[1] if len(parts) > 1 else "0", "requête invalide"))
            stdout.flush()
            continue
        rid, block, size = parts[1], int(parts[2]), int(parts[3])
        text = stdin.read(size).decode("utf-8", "replace")
        try:
            if renderer is None:
                engine = render.find_engine()  # Recherché à chaque requête tant qu'il n'est pas installé.
                if engine is None:
                    raise render.RenderError(NOT_INSTALLED)
                renderer = render.preview_renderer(engine)
            stdout.write(reply(rid, renderer.svg_pages(text), block, text))
        except render.RenderError as exc:
            stdout.write(_failure(rid, str(exc)))
        stdout.flush()
    if renderer is not None:
        renderer.stop()
    return 0


def install() -> int:
    def progress(message: str, _fraction: float | None) -> None:
        print(message, flush=True)

    try:
        engine = render.install(progress)
    except render.RenderError as exc:
        print(exc, file=sys.stderr)
        return 1
    print(engine.label)
    return 0


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if args == ["--version"]:
        print(f"simp2puml-preview {__version__}")
        return 0
    if args == ["--install"]:
        return install()
    if args:
        print("Usage : simp2puml-preview [--install]", file=sys.stderr)
        return 2
    return serve(sys.stdin.buffer, sys.stdout.buffer)


if __name__ == "__main__":
    sys.exit(main())
