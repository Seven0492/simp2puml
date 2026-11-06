"""Aperçu de la nouvelle interface par le moteur officiel PlantUML.

Parle le protocole de `pumlrender --serve`, en réutilisant `render.py` : mêmes moteurs, même
profil SANDBOX et mêmes vérifications que l'interface classique. Le texte reçu est déjà résolu
par l'éditeur : ce processus n'a aucun fichier à lire. `--install` installe le moteur.
"""
from __future__ import annotations

import re
import sys
from typing import BinaryIO
import xml.etree.ElementTree as ET

from . import __version__, render

KIND = "plantuml officiel"
_LINE = re.compile(r"ligne (\d+)")
_SVG = "{http://www.w3.org/2000/svg}"
_NUMBER = re.compile(r"-?\d+(?:\.\d+)?")
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


def regions(svg: bytes, first_line: int) -> list[tuple[int, float, float, float, float]]:
    """Zones cliquables : PlantUML note la ligne d'origine (depuis @start, à partir de 0) de chaque élément."""
    try:
        root = ET.fromstring(svg)
    except ET.ParseError:
        return []
    found = []
    for group in root.iter(f"{_SVG}g"):
        line = group.get("data-source-line")
        if line is None or not line.isdigit():
            continue
        try:
            box = _box(group)
        except ValueError:
            continue
        if box is not None:
            found.append((first_line + int(line), *box))
    return found


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
    for line, x, y, w, h in regions(svg, first_line):
        head.append(f"REGION\t{line}\t{x:g}\t{y:g}\t{w:g}\t{h:g}")
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
