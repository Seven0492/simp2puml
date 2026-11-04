"""Génère les icônes et portraits de l'interface graphique à partir de La Joconde.

Source : « Mona Lisa, by Leonardo da Vinci, from C2RMF retouched » (Wikimedia Commons, domaine public),
par exemple la vignette de 960 px de large. Le recadrage est exprimé en fractions de l'image : toute
résolution de cette reproduction convient.

Usage (outil de développement, Pillow requis et non dépendance d'exécution) :
    .venv/bin/python -m pip install pillow
    .venv/bin/python tools/build-icons.py chemin/vers/mona-lisa.jpg
"""
import argparse
from pathlib import Path
import sys

try:
    from PIL import Image, ImageDraw, ImageEnhance
except ImportError:  # pragma: no cover - outil de développement
    sys.exit("Pillow est requis : python -m pip install pillow")

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "src" / "simp2puml" / "assets"
# Fractions (gauche, haut, droite, bas) mesurées sur la reproduction C2RMF 960 × 1431.
PHI = (1 + 5 ** 0.5) / 2
FACE = (245 / 960, 125 / 1431, 675 / 960, 555 / 1431)
# Buste en rectangle d'or (largeur : hauteur = 1 : φ), centré sur le visage (x = 480).
_BUST_TOP, _BUST_BOTTOM = 90, 1080
_BUST_HALF = (_BUST_BOTTOM - _BUST_TOP) / PHI / 2
BUST = ((480 - _BUST_HALF) / 960, _BUST_TOP / 1431, (480 + _BUST_HALF) / 960, _BUST_BOTTOM / 1431)
ICON_SIZES = (16, 24, 32, 48, 64, 128, 256)
PORTRAIT_HEIGHTS = (55, 89, 144, 233)  # Suite de Fibonacci : rapports successifs proches de φ.
GOLD = (196, 160, 92, 255)
FRAME_DARK = (58, 44, 26, 255)


def crop(image: Image.Image, box: tuple[float, float, float, float]) -> Image.Image:
    w, h = image.size
    return image.crop((round(box[0] * w), round(box[1] * h), round(box[2] * w), round(box[3] * h)))


def icon(face: Image.Image, size: int) -> Image.Image:
    """Visage en carré arrondi, cadre doré dès 32 px ; légère accentuation pour les petites tailles."""
    scale = 4  # Suréchantillonnage pour des bords arrondis lisses.
    big = size * scale
    art = face.resize((big, big), Image.LANCZOS)
    if size <= 32:
        art = ImageEnhance.Contrast(art).enhance(1.15)
    radius = max(2, round(size * 0.2)) * scale
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), radius, fill=255)
    out = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    out.paste(art, (0, 0), mask)
    if size >= 32:
        draw = ImageDraw.Draw(out)
        width = max(1, round(size / 32)) * scale
        draw.rounded_rectangle((0, 0, big - 1, big - 1), radius, outline=FRAME_DARK, width=width)
        inset = width
        draw.rounded_rectangle((inset, inset, big - 1 - inset, big - 1 - inset), radius - inset,
                               outline=GOLD, width=max(scale, width // 2))
    return out.resize((size, size), Image.LANCZOS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", type=Path, help="reproduction de La Joconde (JPEG ou PNG)")
    args = parser.parse_args()
    image = Image.open(args.source).convert("RGB")
    ASSETS.mkdir(parents=True, exist_ok=True)
    face, bust = crop(image, FACE), crop(image, BUST)
    icons = [icon(face, size) for size in ICON_SIZES]
    for size, img in zip(ICON_SIZES, icons):
        img.save(ASSETS / f"icon-{size}.png", optimize=True)
    icons[-1].save(ASSETS / "simp2puml.ico", sizes=[(s, s) for s in ICON_SIZES])
    for height in PORTRAIT_HEIGHTS:
        width = round(height * bust.width / bust.height)
        bust.resize((width, height), Image.LANCZOS).save(ASSETS / f"portrait-{height}.png", optimize=True)
    total = sum(p.stat().st_size for p in ASSETS.iterdir())
    print(f"{len(list(ASSETS.iterdir()))} fichiers écrits dans {ASSETS} ({total // 1024} Kio)")

if __name__ == "__main__":
    main()
