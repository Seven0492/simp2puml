"""Interface graphique Tkinter de simp2puml (bibliothèque standard uniquement).

La conversion réutilise `cli.convert` et les mêmes garde-fous que la CLI : lecture limitée
à 20 MiB, mode strict, refus d'écraser la source ou une sortie existante, publication atomique.
L'icône et le portrait reproduisent La Joconde de Léonard de Vinci (domaine public) ;
voir `tools/build-icons.py`.
"""
from __future__ import annotations

import base64
from dataclasses import dataclass, field
import hashlib
from pathlib import Path
import os
import queue
import re
import sys
import threading

from . import __version__, cli, interfaces, preferences, render
from .model import ConversionError
from .simp import list_diagrams

try:  # Certaines distributions livrent Python sans Tk.
    import tkinter as tk
    from tkinter import filedialog, font as tkfont, messagebox, ttk
except ImportError:  # pragma: no cover - dépend de l'installation Python
    tk = None  # type: ignore[assignment]

FORMAT_LABELS = {
    "puml": "PlantUML",
    "simp": "SIMP  · expérimental",
    "atelier": "Atelier Diagrammes",
}
FILE_TYPES = {
    "puml": ("PlantUML", "*.puml"),
    "simp": ("SIMP", "*.simp"),
    "atelier": ("Atelier Diagrammes", "*.atelier"),
}
ALL_DIAGRAMS = "Tous les diagrammes (un bloc PlantUML par vue)"

# Proportions : φ (nombre d'or) et √φ, appliquées là où elles servent la mise en page.
PHI = (1 + 5 ** 0.5) / 2
SQRT_PHI = PHI ** 0.5
# Espacements : suite de Fibonacci (deux termes voisins tendent vers φ), en pixels à 96 ppp.
SPACE = {"xxs": 2, "xs": 3, "s": 5, "m": 8, "l": 13, "xl": 21, "xxl": 34}
# Répartition verticale de l'espace libre selon la section dorée : 1 (haut) pour φ (bas).
GOLDEN_TOP, GOLDEN_BOTTOM = 1000, round(1000 * PHI)
PORTRAIT_HEIGHTS = (55, 89, 144, 233)  # Portraits en rectangle d'or (1 : φ).
# Au-delà (en pixels), l'aperçu montre d'abord une ébauche à demi-résolution (rastérisation 4× plus rapide).
DRAFT_PIXELS = 3_000_000
# Résolution minimale de l'aperçu : 10 ppp ≈ zoom de 10 % (le plus petit autorisé), pour qu'« Ajuster »
# montre vraiment tout un grand diagramme au lieu de s'arrêter à 25 %.
MIN_DPI = 10


def type_size(step: float, base: int = 10) -> int:
    """Échelle typographique modulaire de raison √φ : 8, 9, 10, 13, 16, 21, 26 points."""
    return round(base * SQRT_PHI ** step)


@dataclass(frozen=True)
class Request:
    """Paramètres d'une conversion ; `selector` est un identifiant SIMP ou None."""
    source: Path
    target: str
    selector: str | None = None
    all: bool = False
    strict: bool = False


@dataclass
class Result:
    request: Request
    content: str | None = None
    warnings: list[str] = field(default_factory=list)
    summary: list[str] = field(default_factory=list)
    error: str | None = None
    saved: Path | None = None

    @property
    def publishable(self) -> bool:
        return self.error is None and self.content is not None


def default_target(source: Path) -> str:
    """Une source est d'abord montrée en PlantUML : SIMP converti, ou texte .puml tel quel."""
    return "puml"


def run(request: Request) -> Result:
    """Convertit en mémoire, sans écrire ; en mode strict, le contenu reste consultable mais non publiable."""
    result = Result(request)
    suffix = request.source.suffix.lower()
    if suffix not in cli.SOURCE_SUFFIXES:
        result.error = "l'extension source doit être .simp ou .puml"
        return result
    if request.target not in cli.TARGETS or (cli.TARGETS[request.target] == suffix and suffix != ".puml"):
        result.error = "le format cible doit différer du format source"
        return result
    if suffix == ".puml" and request.target == "puml":
        # Pas de conversion : le texte reste entier, y compris ce que le convertisseur strict refuserait.
        try:
            result.content = cli._read_source(request.source)
        except (OSError, UnicodeError, ConversionError) as exc:
            result.error = str(exc)
        return result
    if request.all and (suffix != ".simp" or request.target != "puml"):
        result.error = "« tous les diagrammes » exige une source SIMP et une sortie PlantUML"
        return result
    try:
        text = cli._read_source(request.source)
        selectors = ([ident for ident, _, _ in list_diagrams(text)] if request.all
                     else [request.selector if suffix == ".simp" else None])
        content, diagrams = cli.convert(text, suffix, request.target, selectors)
    except (OSError, UnicodeError, ConversionError) as exc:
        result.error = str(exc)
        return result
    result.content = content
    result.warnings = [f"{d.name} : {w}" if request.all else w for d in diagrams for w in d.warnings]
    result.summary = ([f"Diagrammes : {len(diagrams)}"] if request.all else []) + cli.summary_lines(diagrams)
    if request.strict and result.warnings:
        result.error = "mode strict : conversion refusée en présence d'avertissements"
    return result


def save(request: Request, output: Path) -> Result:
    """Reconvertit la source actuelle puis publie ; ne remplace jamais un fichier existant."""
    expected = cli.TARGETS.get(request.target, "")
    if output.suffix.lower() != expected:
        return Result(request, error=f"l'extension de sortie doit être {expected}")
    try:
        if cli._same_file(request.source, output):
            raise ConversionError("la sortie ne peut pas être le fichier source")
        if output.exists() or output.is_symlink():
            raise ConversionError(f"la sortie existe déjà : {output}")
    except (OSError, ConversionError) as exc:
        return Result(request, error=str(exc))
    result = run(request)
    if not result.publishable:
        return result
    try:
        cli._publish(output, result.content or "")
    except FileExistsError:
        result.error = f"la sortie existe déjà : {output}"
    except OSError as exc:
        result.error = str(exc)
    else:
        result.saved = output
    return result


def preview_text(request: Request, result: Result | None = None) -> str:
    """Texte PlantUML du diagramme sélectionné, quel que soit le format cible choisi.

    Une source .puml est rendue telle quelle ; une source SIMP est convertie en mémoire.
    Le mode strict ne s'applique pas à l'aperçu, qui n'écrit rien.
    """
    if (result is not None and result.content and result.request.target == "puml"
            and result.request.source == request.source and result.request.selector == request.selector
            and result.request.all == request.all):
        return result.content
    if request.source.suffix.lower() == ".puml":
        return cli._read_source(request.source)
    plantuml = run(Request(request.source, "puml", request.selector, request.all))
    if plantuml.content is None:
        raise ConversionError(plantuml.error or "conversion PlantUML impossible")
    return plantuml.content


# Directives PlantUML proposées par le menu « Mise en page » : (libellé, ligne, lignes incompatibles).
LAYOUT_DIRECTIVES = {
    "vertical": ("Hiérarchie de gauche à droite", "left to right direction", ("top to bottom direction",)),
    "wrap": ("Couper les notes et textes longs (200 px)", "skinparam wrapWidth 200", ()),
    "hide_empty": ("Masquer les compartiments vides", "hide empty members", ()),
    # Le moteur intégré (smetana) ignore cette directive : elle n'agit qu'avec Graphviz.
    "ortho": ("Liens à angle droit (avec Graphviz)", "skinparam linetype ortho", ("skinparam linetype polyline",)),
}
EDIT_DELAY_MS = 300  # Aperçu relancé après cette pause de frappe.
# Indications de disposition SIMP écrites par la conversion : direction dans la flèche, liens invisibles.
_HINT_ARROW = re.compile(r"-(?:up|down|left|right)-|\.(?:up|down|left|right)\.")
_HINT_HIDDEN = re.compile(r"\s*E\d+ -\[hidden\](?:up|down|left|right)- E\d+\s*")
_RELATION_LINE = re.compile(r"\s*E\d+\b")


def has_directive(text: str, directive: str) -> bool:
    return any(line.strip() == directive for line in text.split("\n"))


def has_layout_hints(text: str) -> bool:
    """Le texte suit-il encore la disposition SIMP (directions de liens, liens invisibles) ?"""
    return any(_HINT_HIDDEN.fullmatch(line) or (_RELATION_LINE.match(line) and _HINT_ARROW.search(line))
               for line in text.split("\n"))


def strip_layout_hints(text: str) -> str:
    """Disposition libre : PlantUML place tout lui-même (utile avec « gauche à droite » pour une page portrait).

    Seules les lignes de relation (alias E1, E2…) sont touchées : le texte des notes et des classes reste intact.
    """
    out = []
    for line in text.split("\n"):
        if _HINT_HIDDEN.fullmatch(line):
            continue
        if _RELATION_LINE.match(line):
            line = _HINT_ARROW.sub(lambda m: m.group(0)[0] * 2, line, count=1)
        out.append(line)
    return "\n".join(out)


def portrait_layout(text: str) -> str:
    """Préréglage page portrait : disposition libre, hiérarchie de gauche à droite, notes coupées."""
    text = strip_layout_hints(text)
    for key in ("vertical", "wrap"):
        _, directive, conflicts = LAYOUT_DIRECTIVES[key]
        text = set_directive(text, directive, True, conflicts)
    return text


def set_directive(text: str, directive: str, enabled: bool, conflicts: tuple[str, ...] = ()) -> str:
    """Ajoute (après chaque @startuml) ou retire une directive ; les directives incompatibles sont retirées."""
    removed = {directive, *conflicts}
    lines = [line for line in text.split("\n") if line.strip() not in removed]
    if not enabled:
        return "\n".join(lines)
    out = []
    for line in lines:
        out.append(line)
        if re.match(r"\s*@startuml\b", line):
            out.append(directive)
    return "\n".join(out)


def save_text(request: Request, output: Path, content: str) -> Path:
    """Enregistre un résultat modifié à la main ; mêmes refus que la CLI (source, fichier existant)."""
    expected = cli.TARGETS.get(request.target, "")
    if output.suffix.lower() != expected:
        raise ConversionError(f"l'extension de sortie doit être {expected}")
    if cli._same_file(request.source, output):
        raise ConversionError("la sortie ne peut pas être le fichier source")
    if output.exists() or output.is_symlink():
        raise ConversionError(f"la sortie existe déjà : {output}")
    try:
        cli._publish(output, content)
    except FileExistsError as exc:
        raise ConversionError(f"la sortie existe déjà : {output}") from exc
    return output


def image_paths(output: Path, count: int) -> list[Path]:
    """Un fichier par page : `nom.ext`, ou `nom-1.ext`, `nom-2.ext`… s'il y en a plusieurs."""
    if count == 1:
        return [output]
    return [output.with_name(f"{output.stem}-{index}{output.suffix}") for index in range(1, count + 1)]


def save_images(pages: list[bytes], output: Path) -> list[Path]:
    """Publie chaque page atomiquement ; refuse tout si un fichier visé existe déjà."""
    paths = image_paths(output, len(pages))
    existing = [p for p in paths if p.exists() or p.is_symlink()]
    if existing:
        raise ConversionError(f"la sortie existe déjà : {existing[0]}")
    for path, data in zip(paths, pages):
        cli._publish(path, data)
    return paths


# Présentation ---------------------------------------------------------------------------------
# Palette « sfumato » : parchemin, terre d'ombre, vert du paysage et cadre doré.
C = {
    "bg": "#f2ece1", "surface": "#fffcf6", "surface_alt": "#f6f0e4", "border": "#e2d6c0",
    "border_strong": "#cdbd9f", "ink": "#2a241c", "muted": "#7b6f5f", "faint": "#b5a993",
    "accent": "#3d5a4c", "accent_hover": "#324b3f", "accent_press": "#283d33", "accent_soft": "#e4ebe5",
    "gold": "#b8914a", "gold_soft": "#f1e6cc", "frame": "#3a2c1a",
    "header": "#231d16", "header_fg": "#f5ead2", "header_sub": "#c9ad73", "header_chip": "#3a3024",
    "ok": "#3d6b4a", "warn": "#9a6411", "warn_soft": "#fbf1dd", "error": "#a3362a", "error_soft": "#f8e4df",
    "code_bg": "#1e1a15", "code_fg": "#e8dcc6", "gutter_bg": "#191511", "gutter_fg": "#6d604f",
    "code_sel": "#4a3f30", "code_thumb": "#4a4034", "code_thumb_hover": "#5f5243",
}
SYNTAX_COLORS = {
    "arrow": "#e0906a", "keyword": "#d9b26a", "ident": "#9fc0c9", "attr": "#9fb7c9",
    "stereo": "#c7a6d4", "string": "#a9c08a", "comment": "#82745f",
}
_PUML_KEYWORDS = (r"@startuml|@enduml|\b(?:class|abstract|interface|enum|annotation|title|note|end note|as|of|on"
                  r"|package|namespace|component|node|actor|usecase|state|participant|database|entity|object"
                  r"|artifact|rectangle|alt|else|opt|loop|par|break|critical|group|end|activate|deactivate"
                  r"|create|destroy|return|skinparam|hide|show|left|right|top|bottom|over|together|map)\b")
_ARROW = r"(?<![\w\"])(?:<\|[-.]+|[<*o#]?[-.]{2,}\|?>?[*o#>]?)(?![\w\"])"
SYNTAX = {
    "puml": [("arrow", _ARROW), ("keyword", _PUML_KEYWORDS), ("stereo", r"<<[^>\n]*>>"),
             ("string", r'"[^"\n]*"'), ("comment", r"^\s*'.*$")],
    "atelier": [("arrow", _ARROW), ("ident", r"::[\w.]+"),
                ("keyword", r"\b(?:dsl|class|interface|enum|field|method|note|circle|abstract|static|kind|literal)\b"),
                ("stereo", r"<<[^>\n]*>>"), ("string", r'"[^"\n]*"'), ("comment", r"//.*$")],
    "simp": [("keyword", r"</?[\w:.-]+|/?>|<\?xml|\?>"), ("attr", r"[\w:.-]+(?==)"),
             ("string", r'"[^"]*"'), ("comment", r"<!--.*?-->")],
}
HIGHLIGHT_LINES = 5000  # Au-delà, le texte reste lisible mais n'est plus coloré (fluidité).


ASSETS = Path(__file__).resolve().parent / "assets"


def _asset_path(name: str) -> Path | None:
    """Fichier d'image du paquet ; lecture directe (rapide), importlib.resources pour un paquet zippé."""
    direct = ASSETS / name
    if direct.is_file():
        return direct
    return None


def _asset(name: str) -> bytes | None:
    path = _asset_path(name)
    if path is not None:
        return path.read_bytes()
    try:
        from importlib import resources

        return resources.files(__package__).joinpath("assets").joinpath(name).read_bytes()
    except (OSError, ModuleNotFoundError):
        return None


def _size_text(length: int) -> str:
    if length < 1024:
        return f"{length} o"
    if length < 1024 * 1024:
        return f"{length / 1024:.1f} Kio".replace(".", ",")
    return f"{length / 1024 / 1024:.1f} Mio".replace(".", ",")


def _shorten(text: str, limit: int = 70) -> str:
    if len(text) <= limit:
        return text
    keep = (limit - 1) // 2
    return text[:keep] + "…" + text[-keep:]


class App:
    """Fenêtre principale : choix de la source, du diagramme et du format, aperçu puis enregistrement."""

    POLL_MS = 40
    TABS = ("Résultat", "Aperçu", "Avertissements", "Résumé")
    ZOOMS = (0.25, 0.33, 0.5, 0.67, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0)

    def __init__(self, root: "tk.Tk", source: Path | None = None) -> None:
        self.root = root
        self.diagrams: list[tuple[str, str, str]] = []
        self.result: Result | None = None
        self._generation = 0
        self._results: queue.Queue[tuple[int, Result]] = queue.Queue()
        self._busy = False
        self._images: list[tk.PhotoImage] = []
        self.scale = max(1.0, root.winfo_fpixels("1i") / 96)
        # Aperçu : moteur détecté une fois, rendus mis en cache par (texte, résolution).
        self.engine: render.Engine | None = None
        self._engine_checked = False
        self._installing = False
        self._preview_generation = 0
        self._preview_queue: queue.Queue[tuple[int, str, object]] = queue.Queue()
        self._preview_cache: dict[tuple[str, int], render.Rendering] = {}
        self._preview_key: tuple[str, int] | None = None
        self._preview_pages: list[tk.PhotoImage] = []
        self._preview_page = 0
        self.preview_zoom = 1.0
        self._zoom_job: str | None = None
        self._auto_fit = True
        self._preview_busy = False
        self._preview_sizes: dict[str, list[tuple[int, int]]] = {}  # Taille naturelle par texte (SVG).
        # Cache disque des aperçus (désactivable avec SIMP2PUML_NO_CACHE, par exemple pour des essais).
        self._disk_cache = None if os.environ.get("SIMP2PUML_NO_CACHE") else render.PreviewCache()

        root.title(f"simp2puml {__version__}")
        root.configure(bg=C["bg"])
        root.minsize(self.px(820), self.px(820 / PHI))
        # Rectangle d'or adapté à l'écran : 1280 × 791 au plus, sans dépasser un petit écran portable.
        screen_w, screen_h = root.winfo_screenwidth(), root.winfo_screenheight()
        width = min(self.px(1280), round(screen_w * 0.85))
        height = min(round(width / PHI), round(screen_h * 0.88))
        width = min(width, round(height * PHI))
        x = max(0, (screen_w - width) // 2)
        y = max(0, round((screen_h - height) / PHI ** 2))  # Centre optique : 38,2 % de l'espace au-dessus.
        root.geometry(f"{width}x{height}+{x}+{y}")

        self.source_var = tk.StringVar()
        self.diagram_var = tk.StringVar()
        self.target_var = tk.StringVar(value="puml")
        self.strict_var = tk.BooleanVar(value=False)
        self.status_var = tk.StringVar(value="Prêt. Ouvrez un fichier .simp ou .puml.")
        self.meta_var = tk.StringVar()
        self.tab_var = tk.StringVar(value=self.TABS[0])
        self.export_var = tk.StringVar(value=render.FORMATS["png"][0])
        self.zoom_var = tk.StringVar(value="100 %")
        self.page_var = tk.StringVar()
        self.preview_status_var = tk.StringVar()

        self._fonts()
        self._style()
        self._set_icons()
        self._build()
        self._bind_keys()
        self._refresh_controls()
        self._set_status(self.status_var.get(), "idle")
        root.after(self.POLL_MS, self._poll)
        root.protocol("WM_DELETE_WINDOW", self.close)
        self._detect_engine()
        if source is not None:
            self.load(source)

    def close(self) -> None:
        render.cancel_all()  # Pas de processus Java laissé en arrière-plan.
        self.root.destroy()

    def px(self, value: float) -> int:
        return round(value * self.scale)

    def sp(self, name: str) -> int:
        """Espacement de l'échelle de Fibonacci, mis à l'échelle de l'écran."""
        return self.px(SPACE[name])

    # Thème --------------------------------------------------------------------------------
    def _fonts(self) -> None:
        families = {name.lower(): name for name in tkfont.families(self.root)}

        def pick(*candidates: str, fallback: str) -> str:
            return next((families[c.lower()] for c in candidates if c.lower() in families), fallback)

        default = tkfont.nametofont("TkDefaultFont")
        ui = pick("Segoe UI Variable Text", "Segoe UI", "SF Pro Text", "Helvetica Neue", "Inter", "Cantarell",
                  "Noto Sans", "DejaVu Sans", fallback=default.actual("family"))
        serif = pick("Georgia", "Cambria", "Palatino Linotype", "Iowan Old Style", "Noto Serif", "DejaVu Serif",
                     fallback=ui)
        mono = pick("Cascadia Mono", "Cascadia Code", "Consolas", "SF Mono", "Menlo", "JetBrains Mono",
                    "DejaVu Sans Mono", fallback=tkfont.nametofont("TkFixedFont").actual("family"))
        for name in ("TkDefaultFont", "TkTextFont", "TkMenuFont"):
            tkfont.nametofont(name).configure(family=ui, size=type_size(0))
        self.f_ui = tkfont.Font(self.root, family=ui, size=type_size(0))
        self.f_ui_bold = tkfont.Font(self.root, family=ui, size=type_size(0), weight="bold")
        self.f_small = tkfont.Font(self.root, family=ui, size=type_size(-0.5))
        self.f_label = tkfont.Font(self.root, family=ui, size=type_size(-1), weight="bold")
        # Titre et sous-titre dans le rapport φ (21 / 13).
        self.f_title = tkfont.Font(self.root, family=serif, size=type_size(3), weight="bold")
        self.f_subtitle = tkfont.Font(self.root, family=serif, size=type_size(1), slant="italic")
        self.f_heading = tkfont.Font(self.root, family=serif, size=type_size(2))
        self.f_stat = tkfont.Font(self.root, family=serif, size=type_size(4))
        self.f_code = tkfont.Font(self.root, family=mono, size=type_size(0))
        self.f_code_italic = tkfont.Font(self.root, family=mono, size=type_size(0), slant="italic")
        self.root.option_add("*TCombobox*Listbox.font", self.f_ui)
        self.root.option_add("*TCombobox*Listbox.background", C["surface"])
        self.root.option_add("*TCombobox*Listbox.foreground", C["ink"])
        self.root.option_add("*TCombobox*Listbox.selectBackground", C["accent"])
        self.root.option_add("*TCombobox*Listbox.selectForeground", "#ffffff")
        self.root.option_add("*TCombobox*Listbox.borderWidth", 0)

    def _style(self) -> None:
        s = ttk.Style(self.root)
        s.theme_use("clam")
        flat = {"borderwidth": 0, "relief": "flat", "focusthickness": 0}
        s.configure(".", background=C["bg"], foreground=C["ink"], font=self.f_ui, bordercolor=C["border"],
                    lightcolor=C["surface"], darkcolor=C["surface"], troughcolor=C["surface_alt"],
                    focuscolor=C["accent"], selectbackground=C["accent"], selectforeground="#ffffff",
                    fieldbackground=C["surface"], insertcolor=C["ink"])
        s.configure("Card.TFrame", background=C["surface"])
        s.configure("Card.TLabel", background=C["surface"])
        s.configure("File.Card.TLabel", font=self.f_ui_bold)
        s.configure("Muted.Card.TLabel", foreground=C["muted"], font=self.f_small)
        s.configure("Field.Card.TLabel", foreground=C["muted"], font=self.f_label)
        s.configure("Card.TRadiobutton", background=C["surface"], foreground=C["ink"], font=self.f_ui)
        s.map("Card.TRadiobutton", background=[("active", C["surface"])],
              foreground=[("disabled", C["faint"])])

        pad = (self.sp("l"), self.sp("m"))
        primary_pad = (self.sp("xl"), self.sp("m"))
        s.configure("Primary.TButton", background=C["accent"], foreground="#ffffff", padding=primary_pad,
                    font=self.f_ui_bold, lightcolor=C["accent"], darkcolor=C["accent"],
                    bordercolor=C["accent"], **flat)
        s.map("Primary.TButton",
              background=[("disabled", "#c3cbc4"), ("pressed", C["accent_press"]), ("active", C["accent_hover"])],
              lightcolor=[("disabled", "#c3cbc4"), ("pressed", C["accent_press"]), ("active", C["accent_hover"])],
              darkcolor=[("disabled", "#c3cbc4"), ("pressed", C["accent_press"]), ("active", C["accent_hover"])],
              bordercolor=[("disabled", "#c3cbc4"), ("pressed", C["accent_press"]), ("active", C["accent_hover"])],
              foreground=[("disabled", "#f3f5f3")])
        s.configure("Secondary.TButton", background=C["surface"], foreground=C["ink"], padding=pad,
                    lightcolor=C["surface"], darkcolor=C["surface"], bordercolor=C["border_strong"],
                    borderwidth=1, relief="solid", focusthickness=0)
        s.map("Secondary.TButton",
              background=[("disabled", C["surface"]), ("pressed", C["gold_soft"]), ("active", C["surface_alt"])],
              lightcolor=[("pressed", C["gold_soft"]), ("active", C["surface_alt"])],
              darkcolor=[("pressed", C["gold_soft"]), ("active", C["surface_alt"])],
              bordercolor=[("disabled", C["border"]), ("active", C["gold"])],
              foreground=[("disabled", C["faint"])])
        s.configure("Ghost.TButton", background=C["surface"], foreground=C["accent"], padding=pad,
                    font=self.f_ui_bold, lightcolor=C["surface"], darkcolor=C["surface"],
                    bordercolor=C["surface"], **flat)
        s.map("Ghost.TButton",
              background=[("pressed", C["accent_soft"]), ("active", C["surface_alt"])],
              lightcolor=[("pressed", C["accent_soft"]), ("active", C["surface_alt"])],
              darkcolor=[("pressed", C["accent_soft"]), ("active", C["surface_alt"])],
              bordercolor=[("pressed", C["accent_soft"]), ("active", C["surface_alt"])],
              foreground=[("disabled", C["faint"])])

        # Menu déroulant au style des boutons discrets, sans indicateur (« ▾ » dans le libellé).
        s.layout("Ghost.TMenubutton", [("Menubutton.border", {"sticky": "nswe", "children": [
            ("Menubutton.padding", {"sticky": "nswe", "children": [("Menubutton.label", {"sticky": "nswe"})]})]})])
        s.configure("Ghost.TMenubutton", background=C["surface"], foreground=C["accent"], padding=pad,
                    font=self.f_ui_bold, lightcolor=C["surface"], darkcolor=C["surface"], bordercolor=C["surface"],
                    borderwidth=0, relief="flat")
        s.map("Ghost.TMenubutton", background=[("pressed", C["accent_soft"]), ("active", C["surface_alt"])],
              foreground=[("disabled", C["faint"])])
        s.layout("Segment.TRadiobutton", s.layout("Toolbutton"))
        s.configure("Segment.TRadiobutton", background=C["surface_alt"], foreground=C["muted"],
                    padding=(self.sp("l"), self.sp("m")), anchor="center", borderwidth=1, relief="solid",
                    bordercolor=C["border"], lightcolor=C["surface_alt"], darkcolor=C["surface_alt"],
                    font=self.f_ui)
        segment_bg = [("disabled", C["surface_alt"]), ("selected", C["accent"]), ("active", C["gold_soft"])]
        s.map("Segment.TRadiobutton", background=segment_bg, lightcolor=segment_bg, darkcolor=segment_bg,
              bordercolor=[("selected", C["accent"])],
              foreground=[("disabled", C["faint"]), ("selected", "#ffffff"), ("active", C["ink"])])

        s.layout("Tab.TRadiobutton", s.layout("Toolbutton"))
        s.configure("Tab.TRadiobutton", background=C["surface"], foreground=C["muted"],
                    padding=(self.sp("l"), self.sp("m")), borderwidth=0,
                    lightcolor=C["surface"], darkcolor=C["surface"], bordercolor=C["surface"], font=self.f_ui_bold)
        s.map("Tab.TRadiobutton", background=[("active", C["surface"])],
              foreground=[("selected", C["ink"]), ("active", C["ink"])])

        s.configure("Card.TCheckbutton", background=C["surface"], foreground=C["ink"],
                    indicatorbackground=C["surface"], indicatorforeground="#ffffff",
                    upperbordercolor=C["border_strong"], lowerbordercolor=C["border_strong"],
                    indicatormargin=(0, 0, self.sp("s"), 0), padding=self.sp("xxs"), focusthickness=0)
        s.map("Card.TCheckbutton", background=[("active", C["surface"])],
              indicatorbackground=[("selected", C["accent"]), ("active", C["surface_alt"])],
              upperbordercolor=[("selected", C["accent"])], lowerbordercolor=[("selected", C["accent"])])

        s.configure("Card.TCombobox", fieldbackground=C["surface"], background=C["surface"],
                    foreground=C["ink"], arrowcolor=C["muted"], bordercolor=C["border_strong"],
                    lightcolor=C["surface"], darkcolor=C["surface"], padding=(self.sp("m"), self.sp("s")),
                    arrowsize=self.px(14))
        s.map("Card.TCombobox",
              fieldbackground=[("readonly", C["surface"]), ("disabled", C["surface_alt"])],
              background=[("active", C["surface_alt"])],
              foreground=[("disabled", C["faint"])],
              selectbackground=[("readonly", C["surface"])], selectforeground=[("readonly", C["ink"])],
              bordercolor=[("focus", C["gold"]), ("active", C["gold"])],
              arrowcolor=[("disabled", C["faint"]), ("active", C["accent"])])

        for orient, trough, thumb in (("Vertical", "ns", "Vertical"), ("Horizontal", "ew", "Horizontal")):
            name = f"Code.{orient}.TScrollbar"
            s.layout(name, [(f"{orient}.Scrollbar.trough", {"sticky": trough, "children": [
                (f"{thumb}.Scrollbar.thumb", {"expand": "1", "sticky": "nswe"})]})])
            s.configure(name, troughcolor=C["code_bg"], background=C["code_thumb"], bordercolor=C["code_bg"],
                        lightcolor=C["code_thumb"], darkcolor=C["code_thumb"], gripcount=0,
                        arrowsize=self.px(10), borderwidth=0)
            s.map(name, background=[("active", C["code_thumb_hover"])],
                  lightcolor=[("active", C["code_thumb_hover"])], darkcolor=[("active", C["code_thumb_hover"])])
            light = f"Light.{orient}.TScrollbar"
            s.layout(light, s.layout(name))
            s.configure(light, troughcolor=C["surface"], background=C["surface_alt"], bordercolor=C["surface"],
                        lightcolor=C["surface_alt"], darkcolor=C["surface_alt"], gripcount=0,
                        arrowsize=self.px(8), borderwidth=0)
            s.map(light, background=[("active", C["border"])], lightcolor=[("active", C["border"])],
                  darkcolor=[("active", C["border"])])
        s.configure("Accent.Horizontal.TProgressbar", troughcolor=C["surface_alt"], background=C["gold"],
                    lightcolor=C["gold"], darkcolor=C["gold"], bordercolor=C["surface_alt"],
                    thickness=self.px(4))

    def _photo(self, name: str) -> "tk.PhotoImage | None":
        data = _asset(name)
        if data is None:
            return None
        try:
            image = tk.PhotoImage(master=self.root, data=base64.b64encode(data).decode("ascii"))
        except tk.TclError:
            return None
        self._images.append(image)
        return image

    def _portrait(self, height: int) -> "tk.PhotoImage | None":
        wanted = height * self.scale
        sizes = PORTRAIT_HEIGHTS
        best = next((h for h in sizes if h >= wanted * 0.95), sizes[-1])
        return self._photo(f"portrait-{best}.png")

    def _set_icons(self) -> None:
        ico = _asset_path("simp2puml.ico")
        if sys.platform == "win32" and ico is not None:
            # Windows : le .ico multi-tailles suffit (titre, barre des tâches) ; pas de PNG à décoder.
            try:
                self.root.iconbitmap(default=str(ico))
                return
            except tk.TclError:
                pass
        icons = [image for size in (256, 64, 32, 16) if (image := self._photo(f"icon-{size}.png"))]
        if icons:
            self.root.iconphoto(True, *icons)

    # Construction -------------------------------------------------------------------------
    def _card(self, parent) -> "tk.Frame":
        return tk.Frame(parent, bg=C["surface"], highlightthickness=1, highlightbackground=C["border"],
                        highlightcolor=C["border"], bd=0)

    def _golden(self, parent, bg: str) -> tuple["tk.Frame", "tk.Frame"]:
        """(conteneur, contenu) : centré en largeur, espace libre vertical réparti 1 : φ (section dorée).

        Les rangées vides absorbent l'espace puis s'effacent d'abord : le contenu n'est pas rogné.
        """
        holder = tk.Frame(parent, bg=bg)
        holder.columnconfigure(0, weight=1)
        holder.columnconfigure(2, weight=1)
        holder.rowconfigure(0, weight=GOLDEN_TOP)
        holder.rowconfigure(2, weight=GOLDEN_BOTTOM)
        content = tk.Frame(holder, bg=bg)
        content.grid(row=1, column=1)
        return holder, content

    def _framed(self, parent, image, bg: str) -> "tk.Frame":
        """Image dans un cadre doré, comme un tableau."""
        outer = tk.Frame(parent, bg=C["frame"], padx=1, pady=1)
        gold = tk.Frame(outer, bg=C["gold"], padx=self.sp("xs"), pady=self.sp("xs"))
        gold.pack()
        inner = tk.Frame(gold, bg=C["frame"], padx=1, pady=1)
        inner.pack()
        tk.Label(inner, image=image, bd=0, bg=bg).pack()
        return outer

    def _build(self) -> None:
        self._build_header()
        self._build_status()
        body = ttk.Frame(self.root, padding=(self.sp("xl"), self.sp("l")))
        body.pack(fill="both", expand=True)
        body.columnconfigure(0, weight=1)
        body.rowconfigure(1, weight=1)
        self._build_controls(body)
        self._build_output(body)

    def _build_header(self) -> None:
        header = tk.Frame(self.root, bg=C["header"], padx=self.sp("xl"), pady=self.sp("l"))
        header.pack(fill="x")
        tk.Frame(self.root, bg=C["gold"], height=max(1, self.px(2))).pack(fill="x")
        portrait = self._portrait(89)
        if portrait is not None:
            self._framed(header, portrait, C["header"]).pack(side="left", padx=(0, self.sp("xl")))
        titles = tk.Frame(header, bg=C["header"])
        titles.pack(side="left", fill="y")
        tk.Label(titles, text="simp2puml", font=self.f_title, fg=C["header_fg"], bg=C["header"]).pack(anchor="w")
        tk.Label(titles, text="De SIMP à PlantUML, et retour  ·  export Atelier Diagrammes", font=self.f_subtitle,
                 fg=C["header_sub"], bg=C["header"]).pack(anchor="w")
        tk.Label(header, text=f"v{__version__}", font=self.f_label, fg=C["header_sub"], bg=C["header_chip"],
                 padx=self.sp("m"), pady=self.sp("xs")).pack(side="right", anchor="n")
        self.preferences_button = tk.Button(header, text="⚙  Préférences", font=self.f_label, fg=C["header_fg"],
                                            bg=C["header_chip"], activebackground=C["header_chip"],
                                            activeforeground=C["header_fg"], relief="flat", borderwidth=0,
                                            highlightthickness=0, padx=self.sp("m"), pady=self.sp("xs"),
                                            cursor="hand2", command=self.open_preferences)
        self.preferences_button.pack(side="right", anchor="n", padx=(0, self.sp("m")))

    def _build_controls(self, body) -> None:
        card = self._card(body)
        card.grid(row=0, column=0, sticky="ew", pady=(0, self.sp("l")))
        inner = ttk.Frame(card, style="Card.TFrame", padding=(self.sp("xl"), self.sp("l")))
        inner.pack(fill="x")
        inner.columnconfigure(1, weight=1)
        gap = self.sp("m")

        ttk.Label(inner, text="SOURCE", style="Field.Card.TLabel").grid(row=0, column=0, sticky="w",
                                                                        padx=(0, self.sp("xl")))
        names = ttk.Frame(inner, style="Card.TFrame")
        names.grid(row=0, column=1, sticky="ew", pady=(0, gap))
        self.file_label = ttk.Label(names, text="Aucun fichier ouvert", style="File.Card.TLabel")
        self.file_label.pack(anchor="w")
        self.folder_label = ttk.Label(names, text="Choisissez un projet .simp ou un diagramme .puml",
                                      style="Muted.Card.TLabel")
        self.folder_label.pack(anchor="w")
        buttons = ttk.Frame(inner, style="Card.TFrame")
        buttons.grid(row=0, column=2, sticky="e", pady=(0, gap))
        self.reload_button = ttk.Button(buttons, text="↻  Recharger", style="Ghost.TButton", command=self.reload)
        self.reload_button.pack(side="left", padx=(0, self.sp("s")))
        ttk.Button(buttons, text="Ouvrir…", style="Secondary.TButton", command=self.browse).pack(side="left")

        ttk.Label(inner, text="DIAGRAMME", style="Field.Card.TLabel").grid(row=1, column=0, sticky="w",
                                                                           padx=(0, self.sp("xl")))
        self.diagram_box = ttk.Combobox(inner, textvariable=self.diagram_var, state="disabled",
                                        style="Card.TCombobox", font=self.f_ui)
        self.diagram_box.grid(row=1, column=1, columnspan=2, sticky="ew", pady=gap)
        self.diagram_box.bind("<<ComboboxSelected>>", lambda _e: self._options_changed())

        ttk.Label(inner, text="FORMAT CIBLE", style="Field.Card.TLabel").grid(row=2, column=0, sticky="w",
                                                                              padx=(0, self.sp("xl")), pady=(gap, 0))
        options = ttk.Frame(inner, style="Card.TFrame")
        options.grid(row=2, column=1, columnspan=2, sticky="ew", pady=(gap, 0))
        segments = ttk.Frame(options, style="Card.TFrame")
        segments.pack(side="left")
        self.target_buttons: dict[str, ttk.Radiobutton] = {}
        for key, label in FORMAT_LABELS.items():
            button = ttk.Radiobutton(segments, text=label, value=key, variable=self.target_var,
                                     style="Segment.TRadiobutton", command=self._options_changed)
            button.pack(side="left", padx=(0, 0 if key == "atelier" else self.sp("xs")))
            self.target_buttons[key] = button
        ttk.Checkbutton(options, text="Mode strict", variable=self.strict_var, style="Card.TCheckbutton",
                        command=self._options_changed).pack(side="left", padx=(self.sp("xl"), 0))
        ttk.Label(options, text="refuse l'enregistrement s'il y a des avertissements",
                  style="Muted.Card.TLabel").pack(side="left", padx=(self.sp("s"), 0))

    def _build_output(self, body) -> None:
        card = self._card(body)
        card.grid(row=1, column=0, sticky="nsew")
        card.rowconfigure(2, weight=1)
        card.columnconfigure(0, weight=1)
        self.output_card = card

        bar = tk.Frame(card, bg=C["surface"])
        bar.grid(row=0, column=0, sticky="ew", padx=self.sp("m"))
        self.tab_buttons: dict[str, ttk.Radiobutton] = {}
        self.tab_lines: dict[str, tk.Frame] = {}
        for name in self.TABS:
            column = tk.Frame(bar, bg=C["surface"])
            column.pack(side="left")
            button = ttk.Radiobutton(column, text=name, value=name, variable=self.tab_var,
                                     style="Tab.TRadiobutton", command=self._show_tab)
            button.pack()
            line = tk.Frame(column, height=max(2, self.sp("xs")), bg=C["surface"])
            line.pack(fill="x", padx=self.sp("l"))  # Soulignement aligné sur le texte de l'onglet.
            self.tab_buttons[name], self.tab_lines[name] = button, line
        actions = ttk.Frame(bar, style="Card.TFrame")
        actions.pack(side="right", pady=self.sp("s"))
        self.kind_label = tk.Label(actions, text="", font=self.f_label, fg=C["faint"], bg=C["surface"])
        self.kind_label.pack(side="left", padx=(0, self.sp("l")))
        self.copy_button = ttk.Button(actions, text="Copier", style="Ghost.TButton", command=self.copy)
        self.copy_button.pack(side="left", padx=(0, self.sp("s")))
        self.save_button = ttk.Button(actions, text="Enregistrer sous…", style="Primary.TButton",
                                      command=self.save_as)
        self.save_button.pack(side="left")
        tk.Frame(card, bg=C["border"], height=1).grid(row=1, column=0, sticky="ew")

        self.banner = tk.Frame(card, bg=C["error_soft"], padx=self.sp("xl"), pady=self.sp("m"))
        self.banner_icon = tk.Label(self.banner, text="✕", font=self.f_ui_bold, fg=C["error"], bg=C["error_soft"])
        self.banner_icon.pack(side="left", padx=(0, self.sp("m")))
        self.banner_text = tk.Label(self.banner, text="", font=self.f_ui, fg=C["error"], bg=C["error_soft"],
                                    justify="left", anchor="w")
        self.banner_text.pack(side="left", fill="x", expand=True)
        self.banner.bind("<Configure>", lambda e: self.banner_text.configure(wraplength=max(200, e.width - 80)))

        self.pages = tk.Frame(card, bg=C["surface"])
        self.pages.grid(row=3, column=0, sticky="nsew")
        card.rowconfigure(3, weight=1)
        card.rowconfigure(2, weight=0)
        self.pages.rowconfigure(0, weight=1)
        self.pages.columnconfigure(0, weight=1)
        self.page_frames = {
            "Résultat": self._build_code(self.pages),
            "Aperçu": self._build_preview(self.pages),
            "Avertissements": self._build_warnings(self.pages),
            "Résumé": self._build_summary(self.pages),
        }
        for frame in self.page_frames.values():
            frame.grid(row=0, column=0, sticky="nsew")
        self.empty = self._build_empty(card)
        self._show_tab()

    def _build_code(self, parent) -> "tk.Frame":
        outer = tk.Frame(parent, bg=C["surface"])
        outer.rowconfigure(2, weight=1)
        outer.columnconfigure(0, weight=1)

        # Barre de l'éditeur : mise en page PlantUML, retour à la conversion, état de modification.
        bar = ttk.Frame(outer, style="Card.TFrame", padding=(self.sp("l"), self.sp("s")))
        bar.grid(row=0, column=0, sticky="ew")
        self.layout_vars = {key: tk.BooleanVar(value=False) for key in LAYOUT_DIRECTIVES}
        self.layout_button = ttk.Menubutton(bar, text="Mise en page  ▾", style="Ghost.TMenubutton")
        menu = tk.Menu(self.layout_button, tearoff=False, bg=C["surface"], fg=C["ink"], font=self.f_ui,
                       activebackground=C["accent"], activeforeground="#ffffff", selectcolor=C["accent"],
                       bd=0, postcommand=self._sync_layout_menu)
        self.simp_layout_var = tk.BooleanVar(value=True)
        menu.add_checkbutton(label="Suivre la disposition SIMP", variable=self.simp_layout_var,
                             command=self.toggle_simp_layout)
        menu.add_command(label="Préréglage : page portrait (document)", command=self.apply_portrait)
        menu.add_separator()
        for key, (label, _, _) in LAYOUT_DIRECTIVES.items():
            menu.add_checkbutton(label=label, variable=self.layout_vars[key],
                                 command=lambda k=key: self.toggle_layout(k))
        self.layout_button.configure(menu=menu)
        self.layout_button.pack(side="left")
        self.revert_button = ttk.Button(bar, text="↺  Rétablir la conversion", style="Ghost.TButton",
                                        command=self.revert_edits)
        self.edit_state = tk.Label(bar, text="", font=self.f_small, fg=C["muted"], bg=C["surface"])
        self.edit_state.pack(side="right")
        tk.Frame(outer, bg=C["border"], height=1).grid(row=1, column=0, sticky="ew")

        frame = tk.Frame(outer, bg=C["code_bg"])
        frame.grid(row=2, column=0, sticky="nsew")
        frame.rowconfigure(0, weight=1)
        frame.columnconfigure(1, weight=1)
        common = {"bd": 0, "highlightthickness": 0, "font": self.f_code, "pady": self.sp("m"), "wrap": "none",
                  "spacing1": 1, "spacing3": 1, "cursor": "arrow"}
        self.gutter = tk.Text(frame, width=5, bg=C["gutter_bg"], fg=C["gutter_fg"], padx=self.sp("m"),
                              takefocus=0, **common)
        self.gutter.tag_configure("right", justify="right")
        self.gutter.tag_configure("error_line", foreground="#ffffff", background=C["error"])
        self.gutter.grid(row=0, column=0, sticky="ns")
        tab_width = self.f_code.measure("  ")
        self.output_text = tk.Text(frame, bg=C["code_bg"], fg=C["code_fg"], insertbackground="#f1d9a8",
                                   insertwidth=2, selectbackground=C["code_sel"], selectforeground=C["code_fg"],
                                   inactiveselectbackground=C["code_sel"], padx=self.sp("l"), undo=True,
                                   maxundo=-1, autoseparators=True, tabs=(tab_width,), **common)
        self.output_text.configure(cursor="xterm")
        self.output_text.grid(row=0, column=1, sticky="nsew")
        yscroll = ttk.Scrollbar(frame, orient="vertical", style="Code.Vertical.TScrollbar",
                                command=self._scroll_code)
        yscroll.grid(row=0, column=2, sticky="ns")
        xscroll = ttk.Scrollbar(frame, orient="horizontal", style="Code.Horizontal.TScrollbar",
                                command=self.output_text.xview)
        xscroll.grid(row=1, column=1, sticky="ew")

        def on_y(first, last):
            yscroll.set(first, last)
            self.gutter.yview_moveto(first)
            self._schedule("_highlight_job", 40, self._highlight_visible)  # Colorer ce qui défile.

        self.output_text.configure(yscrollcommand=on_y, xscrollcommand=xscroll.set)
        self.gutter.configure(state="disabled")
        self.gutter.bind("<MouseWheel>", lambda e: self.output_text.yview_scroll(-e.delta // 120 or -1, "units")
                         or "break")
        for tag, color in SYNTAX_COLORS.items():
            options = {"foreground": color}
            if tag == "comment":
                options["font"] = self.f_code_italic
            self.output_text.tag_configure(tag, **options)
        self.output_text.tag_configure("error_line", background="#4a2420")
        self.output_text.tag_lower("error_line")
        self.output_text.tag_raise("sel")
        editor = self.output_text
        editor.bind("<<Modified>>", self._on_modified)
        editor.bind("<Return>", self._newline)
        editor.bind("<Tab>", lambda _e: (editor.insert("insert", "  "), "break")[1])
        editor.bind("<Control-a>", lambda _e: (editor.tag_add("sel", "1.0", "end-1c"), "break")[1])
        # Raccourcis de la fenêtre qui ont un autre sens par défaut dans un Text (Ctrl+O insère une ligne).
        editor.bind("<Control-o>", lambda _e: (self.browse(), "break")[1])
        editor.bind("<Control-s>", lambda _e: (self.save_as(), "break")[1])
        self._generated = ""
        self._code_target = "puml"
        self._dirty = False
        self._filling = False
        self._gutter_lines = 0
        return outer

    def _schedule(self, name: str, delay: int, callback) -> None:
        """Regroupe les appels rapprochés (frappe, défilement) en un seul, après `delay` ms."""
        job = getattr(self, name, None)
        if job is not None:
            self.root.after_cancel(job)
        setattr(self, name, self.root.after(delay, lambda: (setattr(self, name, None), callback())))

    # Éditeur du résultat --------------------------------------------------------------------
    def _highlight(self, first: int, last: int) -> None:
        """Recolore les lignes `first` à `last` (1-based) selon le format du résultat."""
        text = self.output_text
        for tag in SYNTAX_COLORS:
            text.tag_remove(tag, f"{first}.0", f"{last}.end")
        chunk = text.get(f"{first}.0", f"{last}.end")
        for offset, line in enumerate(chunk.split("\n")):
            number = first + offset
            for tag, pattern in SYNTAX.get(self._code_target, ()):
                for match in re.finditer(pattern, line):
                    if match.end() > match.start():
                        text.tag_add(tag, f"{number}.{match.start()}", f"{number}.{match.end()}")

    def _highlight_visible(self) -> None:
        text = self.output_text
        first = int(text.index("@0,0").split(".")[0])
        last = int(text.index(f"@0,{max(1, text.winfo_height())}").split(".")[0])
        self._highlight(first, last)

    def _sync_gutter(self) -> None:
        text = self.output_text
        count = int(text.index("end-1c").split(".")[0])
        if text.get("end-2c", "end-1c") == "\n":
            count -= 1  # Retour à la ligne final : pas de ligne vide numérotée.
        count = max(count, 1 if text.get("1.0", "end-1c") else 0)
        if count == self._gutter_lines:
            return
        self._gutter_lines = count
        gutter = self.gutter
        gutter.configure(state="normal", width=max(3, len(str(count))) + 1)
        gutter.delete("1.0", "end")
        gutter.insert("1.0", "\n".join(str(i) for i in range(1, count + 1)), "right")
        gutter.configure(state="disabled")
        gutter.yview_moveto(text.yview()[0])

    def _on_modified(self, _event=None) -> None:
        text = self.output_text
        if not text.edit_modified():
            return
        text.edit_modified(False)
        if self._filling:
            return
        self._set_dirty(True)  # Confirmé (ou annulé si le texte redevient identique) après la pause.
        self._sync_gutter()
        self._schedule("_highlight_job", 30, self._highlight_visible)
        self._schedule("_edit_job", EDIT_DELAY_MS, self._after_edit)

    def _newline(self, _event=None) -> str:
        """Retour à la ligne avec l'indentation de la ligne courante (+2 après « { »)."""
        text = self.output_text
        line = text.get("insert linestart", "insert")
        indent = re.match(r"[ \t]*", line).group(0) + ("  " if line.rstrip().endswith("{") else "")
        text.insert("insert", "\n" + indent)
        text.see("insert")
        return "break"

    def editor_text(self) -> str:
        return self.output_text.get("1.0", "end-1c")

    def _after_edit(self) -> None:
        content = self.editor_text()
        self._set_dirty(content != self._generated)
        self._mark_error_line(None)
        self._update_meta(content)
        if self._code_target == "puml":
            self._request_preview()  # L'aperçu suit le texte modifié.

    def _set_dirty(self, dirty: bool, force: bool = False) -> None:
        if dirty == self._dirty and not force:
            return
        self._dirty = dirty
        self.tab_buttons["Résultat"].configure(text="Résultat  ●" if dirty else "Résultat")
        if dirty:
            self.revert_button.pack(side="left", padx=(self.sp("s"), 0))
            self.edit_state.configure(text="Modifié à la main · Ctrl+Z annule · enregistré tel quel",
                                      fg=C["warn"])
        else:
            self.revert_button.pack_forget()
            self.edit_state.configure(text="Modifiable · l'aperçu suit vos changements", fg=C["muted"])
        self._refresh_controls()

    def _edited_plantuml(self) -> str | None:
        """Texte PlantUML modifié à utiliser pour l'aperçu et l'export, sinon None (conversion)."""
        if self._dirty and self._code_target == "puml":
            return self.editor_text()
        return None

    def revert_edits(self) -> None:
        if self.result is not None:
            self._fill_code(self._generated, self._code_target)
            self._update_meta(self._generated)
            self._request_preview()

    def _sync_layout_menu(self) -> None:
        content = self.editor_text()
        for key, (_, directive, _) in LAYOUT_DIRECTIVES.items():
            self.layout_vars[key].set(has_directive(content, directive))
        self.simp_layout_var.set(has_layout_hints(content))

    def toggle_layout(self, key: str) -> None:
        """Ajoute ou retire une directive de mise en page ; une seule étape d'annulation (Ctrl+Z)."""
        _, directive, conflicts = LAYOUT_DIRECTIVES[key]
        before = self.editor_text()
        self._replace_text(set_directive(before, directive, self.layout_vars[key].get(), conflicts))

    def toggle_simp_layout(self) -> None:
        """Décocher : disposition libre de PlantUML. Recocher n'est possible qu'en rétablissant la conversion."""
        before = self.editor_text()
        if self.simp_layout_var.get():
            if not has_layout_hints(before):
                self.simp_layout_var.set(False)
                messagebox.showinfo(
                    "simp2puml", "Les positions SIMP ne peuvent pas être recalculées depuis le texte modifié.\n\n"
                                 "Annulez (Ctrl+Z) ou utilisez « Rétablir la conversion ».", parent=self.root)
            return
        self._replace_text(strip_layout_hints(before))

    def apply_portrait(self) -> None:
        """Page portrait pour un document : disposition libre, gauche à droite, notes coupées (un seul Ctrl+Z)."""
        self._replace_text(portrait_layout(self.editor_text()))

    def _replace_text(self, after: str) -> None:
        text = self.output_text
        if after == self.editor_text():
            return
        view, cursor = text.yview()[0], text.index("insert")
        text.configure(autoseparators=False)
        text.edit_separator()
        text.delete("1.0", "end")
        text.insert("1.0", after)
        text.edit_separator()
        text.configure(autoseparators=True)
        text.mark_set("insert", cursor)
        text.yview_moveto(view)
        self._highlight(1, min(HIGHLIGHT_LINES, int(text.index("end-1c").split(".")[0])))
        self._select_tab("Aperçu")  # Montrer l'effet tout de suite.

    def _mark_error_line(self, line: int | None) -> None:
        for widget in (self.output_text, self.gutter):
            widget.tag_remove("error_line", "1.0", "end")
        if line is not None and line >= 1:
            self.output_text.tag_add("error_line", f"{line}.0", f"{line}.0 lineend +1c")
            self.gutter.tag_add("error_line", f"{line}.0", f"{line}.0 lineend +1c")

    def _update_meta(self, content: str) -> None:
        lines = len(content.splitlines())
        self.meta_var.set(f"{lines} ligne{'s' if lines > 1 else ''}  ·  {_size_text(len(content.encode('utf-8')))}"
                          if content else "")

    def _scroll_code(self, *args) -> None:
        self.output_text.yview(*args)

    def _build_preview(self, parent) -> "tk.Frame":
        frame = tk.Frame(parent, bg=C["surface"])
        frame.rowconfigure(2, weight=1)
        frame.columnconfigure(0, weight=1)

        bar = ttk.Frame(frame, style="Card.TFrame", padding=(self.sp("l"), self.sp("s")))
        bar.grid(row=0, column=0, sticky="ew")
        self.page_controls = ttk.Frame(bar, style="Card.TFrame")
        self.prev_button = ttk.Button(self.page_controls, text="‹", width=2, style="Ghost.TButton",
                                      command=lambda: self._show_page(self._preview_page - 1))
        self.prev_button.pack(side="left")
        ttk.Label(self.page_controls, textvariable=self.page_var, style="Muted.Card.TLabel").pack(side="left")
        self.next_button = ttk.Button(self.page_controls, text="›", width=2, style="Ghost.TButton",
                                      command=lambda: self._show_page(self._preview_page + 1))
        self.next_button.pack(side="left", padx=(0, self.sp("l")))
        zoom = ttk.Frame(bar, style="Card.TFrame")
        zoom.pack(side="left")
        self.zoom_controls = zoom
        ttk.Button(zoom, text="−", width=2, style="Ghost.TButton", command=lambda: self._step_zoom(-1)).pack(
            side="left")
        ttk.Label(zoom, textvariable=self.zoom_var, style="Card.TLabel", width=6, anchor="center").pack(side="left")
        ttk.Button(zoom, text="+", width=2, style="Ghost.TButton", command=lambda: self._step_zoom(1)).pack(
            side="left")
        ttk.Button(zoom, text="Ajuster", style="Ghost.TButton", command=self._fit_zoom).pack(side="left")
        ttk.Button(zoom, text="Taille réelle", style="Ghost.TButton", command=lambda: self._set_zoom(1.0)).pack(
            side="left")
        tk.Label(bar, textvariable=self.preview_status_var, font=self.f_small, fg=C["muted"],
                 bg=C["surface"]).pack(side="left", padx=self.sp("l"))
        export = ttk.Frame(bar, style="Card.TFrame")
        export.pack(side="right")
        # Proposé seulement avec le moteur Java alors qu'un exécutable natif existe pour ce système.
        self.native_button = ttk.Button(bar, text="⚡  Accélérer l'aperçu", style="Ghost.TButton",
                                        command=self.upgrade_engine)
        self._upgrading = False
        self.export_box = ttk.Combobox(export, textvariable=self.export_var, state="readonly", width=24,
                                       style="Card.TCombobox", font=self.f_ui,
                                       values=[label for label, _, _ in render.FORMATS.values()])
        self.export_box.pack(side="left", padx=(0, self.sp("m")))
        self.export_button = ttk.Button(export, text="Exporter l'image…", style="Secondary.TButton",
                                        command=self.export_image)
        self.export_button.pack(side="left")
        self.preview_controls = (self.page_controls, zoom, export)
        tk.Frame(frame, bg=C["border"], height=1).grid(row=1, column=0, sticky="ew")

        stack = tk.Frame(frame, bg=C["surface_alt"])
        stack.grid(row=2, column=0, sticky="nsew")
        stack.rowconfigure(1, weight=1)
        stack.columnconfigure(0, weight=1)
        self.preview_notice = tk.Label(stack, text="", font=self.f_small, fg=C["warn"], bg=C["warn_soft"],
                                       anchor="w", justify="left", padx=self.sp("xl"), pady=self.sp("m"))
        self.preview_notice.bind("<Configure>",
                                 lambda e: self.preview_notice.configure(wraplength=max(200, e.width - 40)))
        self.canvas = tk.Canvas(stack, bg=C["surface_alt"], highlightthickness=0, bd=0)
        self.canvas.grid(row=1, column=0, sticky="nsew")
        yscroll = ttk.Scrollbar(stack, orient="vertical", style="Light.Vertical.TScrollbar",
                                command=self.canvas.yview)
        yscroll.grid(row=1, column=1, sticky="ns")
        xscroll = ttk.Scrollbar(stack, orient="horizontal", style="Light.Horizontal.TScrollbar",
                                command=self.canvas.xview)
        xscroll.grid(row=2, column=0, sticky="ew")
        self.canvas.configure(yscrollcommand=yscroll.set, xscrollcommand=xscroll.set)
        self.canvas.bind("<Configure>", lambda _e: self._place_image())
        self.canvas.bind("<ButtonPress-1>", lambda e: (self.canvas.scan_mark(e.x, e.y),
                                                       self.canvas.configure(cursor="fleur")))
        self.canvas.bind("<B1-Motion>", lambda e: self.canvas.scan_dragto(e.x, e.y, gain=1))
        self.canvas.bind("<ButtonRelease-1>", lambda _e: self.canvas.configure(cursor=""))
        self.canvas.bind("<MouseWheel>", self._wheel)
        self.canvas.bind("<Shift-MouseWheel>", lambda e: self.canvas.xview_scroll(-e.delta // 120 or -1, "units"))
        self.canvas.bind("<Control-MouseWheel>", lambda e: self._step_zoom(1 if e.delta > 0 else -1))
        self.canvas.bind("<Button-4>", lambda _e: self.canvas.yview_scroll(-3, "units"))
        self.canvas.bind("<Button-5>", lambda _e: self.canvas.yview_scroll(3, "units"))
        self.preview_message = tk.Label(stack, text="", font=self.f_ui, fg=C["muted"], bg=C["surface_alt"],
                                        justify="center")
        self.install_panel = self._build_install(stack)
        return frame

    def _build_install(self, parent) -> "tk.Frame":
        panel, holder = self._golden(parent, C["surface_alt"])
        card = tk.Frame(holder, bg=C["surface"], highlightthickness=1, highlightbackground=C["border"],
                        padx=self.sp("xxl"), pady=self.sp("xl"))
        card.pack(pady=self.sp("l"))
        tk.Label(card, text="Aperçu et export d'images", font=self.f_heading, fg=C["ink"],
                 bg=C["surface"]).pack(anchor="w")
        tk.Label(card, text="Les images sont dessinées sur cet ordinateur par PlantUML, qui fonctionne avec "
                            "Java.\nRien n'est envoyé en ligne. Il manque :", font=self.f_ui, fg=C["muted"],
                 bg=C["surface"], justify="left").pack(anchor="w", pady=(self.sp("s"), self.sp("m")))
        self.install_parts = tk.Label(card, text="", font=self.f_ui_bold, fg=C["ink"], bg=C["surface"],
                                      justify="left")
        self.install_parts.pack(anchor="w")
        tk.Label(card, text=f"Téléchargement unique, vérifié par somme SHA-256, dans :\n{render.data_dir()}",
                 font=self.f_small, fg=C["faint"], bg=C["surface"], justify="left").pack(
            anchor="w", pady=(self.sp("m"), self.sp("l")))
        row = tk.Frame(card, bg=C["surface"])
        row.pack(fill="x")
        self.install_button = ttk.Button(row, text="Installer l'aperçu", style="Primary.TButton",
                                         command=self.install_engine)
        self.install_button.pack(side="left")
        self.install_progress = ttk.Progressbar(row, mode="determinate", maximum=1.0, length=self.sp("xxl") * 7,
                                                style="Accent.Horizontal.TProgressbar")
        # Sur la même ligne que le bouton : rien n'est rogné quand la fenêtre est basse.
        self.install_label = tk.Label(row, text="", font=self.f_small, fg=C["muted"], bg=C["surface"],
                                      justify="left")
        self.install_label.pack(side="right", padx=(self.sp("l"), 0))
        return panel

    # Aperçu -------------------------------------------------------------------------------
    def _preview_state(self, state: str, message: str = "") -> None:
        """'image', 'message' ou 'install' : un seul panneau visible dans la zone d'aperçu."""
        self.install_panel.grid_remove()
        self.preview_message.grid_remove()
        if state == "install":
            self.install_panel.grid(row=0, column=0, rowspan=3, columnspan=2, sticky="nsew")
            self.install_panel.tkraise()
        elif state == "message":
            self.preview_message.configure(text=message)
            self.preview_message.grid(row=0, column=0, rowspan=3, columnspan=2, sticky="nsew")
            self.preview_message.tkraise()
        visible = state == "image"
        for control in self.preview_controls[1:]:
            for child in control.winfo_children():
                if isinstance(child, ttk.Button) or isinstance(child, ttk.Combobox):
                    child.state(["!disabled"] if visible else ["disabled"])

    def _dpi(self) -> int:
        return max(MIN_DPI, round(96 * self.scale * self.preview_zoom))

    def _detect_engine(self) -> None:
        """Au démarrage : trouve le moteur et ouvre ses processus, pour un premier aperçu immédiat."""
        put = self._preview_queue.put  # Pas de référence à la fenêtre depuis un autre fil.
        self._detecting = True

        def work() -> None:
            engine = render.find_engine()
            put((0, "detected", engine))  # Toujours signalé, même absent : les aperçus en attente repartent.
            if engine is not None:
                render.preview_renderer(engine).warm_up()

        threading.Thread(target=work, daemon=True).start()

    def _request_preview(self) -> None:
        """Rend le diagramme courant. Moteur connu : aussi en arrière-plan, l'onglet est prêt d'avance."""
        if self._installing:
            return
        visible = self.tab_var.get() == "Aperçu"
        if self.engine is None and getattr(self, "_detecting", False):
            # Détection du démarrage en cours : l'attendre plutôt que chercher le moteur une seconde fois.
            self._preview_waiting = True
            if visible:
                self.preview_status_var.set("Préparation du moteur…")
            return
        if not visible and self.engine is None:
            return  # Détection et proposition d'installation attendent l'ouverture de l'onglet.
        request = self._request()
        if request is None:
            return
        if self._preview_busy and self.engine is not None:
            render.preview_renderer(self.engine).interrupt()  # Le rendu en cours est devenu inutile.
        self._preview_generation += 1
        self._preview_busy = True
        self._preview_stale = True
        generation, result, engine = self._preview_generation, self.result, self.engine
        zoom = None if self._auto_fit else self.preview_zoom
        canvas = (self.canvas.winfo_width(), self.canvas.winfo_height())
        margin, scale = 2 * self.sp("xl"), self.scale
        cache, sizes_cache, disk = self._preview_cache, self._preview_sizes, self._disk_cache
        override = self._edited_plantuml()  # Texte modifié à la main, lu ici (fil principal).
        if visible:
            self.preview_status_var.set("Rendu en cours…")
        put = self._preview_queue.put

        def work() -> None:
            try:
                found = engine or render.find_engine()
                if found is None:
                    put((generation, "missing", render.missing_parts()))
                    return
                if engine is None:
                    put((generation, "engine", found))
                renderer = render.preview_renderer(found)
                text = override if override is not None else preview_text(request, result)
                digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
                sizes = sizes_cache.get(digest) or (disk.sizes(found, text) if disk else None)
                if sizes is None:  # Mise en page seule (SVG) : quelques dizaines de millisecondes.
                    sizes, error = renderer.sizes(text)
                    if not sizes:
                        put((generation, "syntax", error or "PlantUML n'a produit aucune image"))
                        return
                    if disk and not error:
                        disk.put_sizes(found, text, sizes)
                sizes_cache[digest] = sizes
                chosen = zoom
                width, height = sizes[0]
                if chosen is None:  # Premier aperçu d'un fichier : directement à la bonne taille.
                    fit = min((canvas[0] - margin) / (width * scale or 1), (canvas[1] - margin) / (height * scale or 1))
                    chosen = 1.0 if fit >= 1 else max(1 / PHI, fit)  # Pas sous 61,8 % : texte lisible.
                dpi = max(MIN_DPI, round(96 * scale * chosen))
                key = (digest, dpi)
                rendering = cache.get(key)
                if rendering is None and disk:  # Déjà dessiné lors d'une session précédente.
                    pages = disk.pages(found, text, dpi)
                    rendering = render.Rendering(pages) if pages else None
                if rendering is None:
                    if width * height * (dpi / 96) ** 2 > DRAFT_PIXELS:
                        # Grand diagramme : ébauche à demi-résolution (4× moins de pixels), puis version nette.
                        draft = renderer.png_pages(text, max(MIN_DPI, dpi // 2))
                        if draft.pages:
                            put((generation, "draft", (key, draft, chosen)))
                    rendering = renderer.png_pages(text, dpi)
                    if disk and rendering.pages and not rendering.error:
                        disk.put_pages(found, text, dpi, rendering.pages)
                put((generation, "image", (key, rendering, chosen)))
            except (OSError, UnicodeError, ConversionError, render.RenderError) as exc:
                put((generation, "error", str(exc)))

        threading.Thread(target=work, daemon=True).start()

    def _preview_message(self, generation: int, kind: str, payload) -> None:
        if kind == "progress":
            text, fraction = payload
            self.install_label.configure(text=text)
            if self._upgrading:  # Passage au moteur natif : la carte d'installation n'est pas affichée.
                percent = f" {round(fraction * 100)} %" if fraction is not None else ""
                self.preview_status_var.set(text.rstrip("…") + percent)
            if fraction is None:
                self.install_progress.configure(mode="indeterminate")
                self.install_progress.start(12)
            else:
                self.install_progress.stop()
                self.install_progress.configure(mode="determinate", value=fraction)
            return
        if kind == "installed":
            previous = self.engine
            self._installing = self._upgrading = False
            self.engine, self._engine_checked = payload, True
            self.install_progress.stop()
            self.install_progress.pack_forget()
            self.install_button.state(["!disabled"])
            if previous is not None and previous != payload:
                render.cancel_all()  # Les processus de l'ancien moteur (Java) ne servent plus.
            self._set_status("Moteur natif installé : aperçu accéléré." if payload.kind == "native"
                             else "Aperçu installé.", "ok")
            self.preview_status_var.set("")
            self._refresh_native_button()
            self._request_preview()
            return
        if kind == "install-error":
            self._installing = self._upgrading = False
            self.install_progress.stop()
            self.install_progress.pack_forget()
            self.install_button.state(["!disabled"])
            self.install_label.configure(text=f"Échec : {payload}", fg=C["error"])
            self.preview_status_var.set("")
            self._set_status(f"Installation impossible : {payload}", "error")
            self._refresh_native_button()
            return
        if kind == "exported":
            paths = payload
            names = "\n".join(str(p) for p in paths)
            self._set_status(f"Image exportée : {paths[0]}" + (f" (+{len(paths) - 1})" if len(paths) > 1 else ""),
                             "ok")
            self.export_button.state(["!disabled"])
            messagebox.showinfo("simp2puml", f"Image exportée :\n{names}", parent=self.root)
            return
        if kind == "export-error":
            self.export_button.state(["!disabled"])
            self._set_status(f"Export impossible : {payload}", "error")
            messagebox.showerror("simp2puml", f"Export impossible :\n{payload}", parent=self.root)
            return
        if kind == "engine":
            self.engine, self._engine_checked = payload, True
            self._refresh_native_button()
            return
        if kind == "edit-saved":
            self._set_status(f"Version modifiée enregistrée : {payload}", "ok")
            messagebox.showinfo("simp2puml", f"Version modifiée enregistrée :\n{payload}", parent=self.root)
            return
        if kind == "edit-save-error":
            self._set_status(f"Enregistrement impossible : {payload}", "error")
            messagebox.showerror("simp2puml", f"Enregistrement impossible :\n{payload}", parent=self.root)
            return
        if kind == "detected":  # Fin de la détection du démarrage (payload : moteur ou None).
            self._detecting = False
            waiting, self._preview_waiting = getattr(self, "_preview_waiting", False), False
            if self.engine is None and payload is not None:
                self.engine, self._engine_checked = payload, True
                self._refresh_native_button()
            if self.engine is not None and self.result is not None:
                self._request_preview()  # Aperçu préparé d'avance (ou attendu par l'onglet).
            elif waiting:
                self._request_preview()  # Aucun moteur : l'onglet affiche la proposition d'installation.
            return
        if generation != self._preview_generation:
            return
        if kind != "draft":
            self._preview_busy = False
            self._show_error_in_editor(payload[1].error if kind == "image" else payload if kind == "syntax" else None)
        if kind == "missing":
            self._engine_checked = True
            self.install_parts.configure(text="\n".join(f"•  {name}  (≈ {size} Mo)" for name, size in payload))
            self.install_label.configure(text="", fg=C["muted"])
            self.preview_status_var.set("")
            self._preview_state("install")
        elif kind == "error":
            self.preview_status_var.set("")
            self._show_notice(None)
            self._preview_pages = []
            self.canvas.delete("all")
            self._preview_state("message", f"Aperçu indisponible\n\n{payload}")
        elif kind == "syntax" or (kind == "image" and not payload[1].pages):
            # Erreur de syntaxe : garder la dernière image valide, comme un éditeur en direct.
            message = payload if kind == "syntax" else payload[1].error
            self.preview_status_var.set("")
            if self._preview_pages:
                self._show_notice(message)
                self._preview_state("image")
            else:
                self._preview_state("message", f"Aperçu indisponible\n\n{message}")
        elif kind in ("image", "draft"):
            key, rendering, zoom = payload
            if kind == "image":
                self._preview_stale = False  # À jour jusqu'à la prochaine demande (option, frappe, zoom).
                self._preview_cache[key] = rendering
                if len(self._preview_cache) > 24:
                    self._preview_cache.pop(next(iter(self._preview_cache)))
            if zoom != self.preview_zoom or self._auto_fit:
                self.preview_zoom = zoom
                self.zoom_var.set(f"{round(zoom * 100)} %")
            self._auto_fit = False
            same_diagram = self._preview_key is not None and self._preview_key[0] == key[0]
            self._preview_key = key
            pages = []
            for data in rendering.pages:
                try:
                    image = tk.PhotoImage(master=self.root, data=base64.b64encode(data).decode("ascii"))
                except tk.TclError:
                    continue
                # Ébauche à demi-résolution affichée à la taille finale le temps du rendu net.
                pages.append(image.zoom(2) if kind == "draft" else image)
            self._preview_pages = pages
            self.preview_status_var.set("Affinage…" if kind == "draft" else "")
            self._show_notice(rendering.error)
            self._preview_state("image")
            page = self._preview_page if same_diagram else 0
            self._show_page(min(page, max(0, len(self._preview_pages) - 1)), keep_view=same_diagram)

    def _show_error_in_editor(self, message: str | None) -> None:
        """Surligne dans l'éditeur la ligne signalée par PlantUML, si l'aperçu vient de ce texte."""
        match = re.search(r"à la ligne (\d+)", message or "")
        single = render.count_diagrams(self.editor_text()) == 1  # Numéros relatifs au bloc sinon.
        if match and single and self._code_target == "puml":
            self._mark_error_line(int(match.group(1)))
        else:
            self._mark_error_line(None)

    def _show_notice(self, message: str | None) -> None:
        if message:
            self.preview_notice.configure(text=f"▲  {message}")
            self.preview_notice.grid(row=0, column=0, columnspan=2, sticky="ew")
        else:
            self.preview_notice.grid_remove()

    def _show_page(self, index: int, keep_view: bool = False) -> None:
        count = len(self._preview_pages)
        if not count:
            return
        self._preview_page = max(0, min(index, count - 1))
        if count > 1:
            self.page_var.set(f"Page {self._preview_page + 1} / {count}")
            self.page_controls.pack(side="left", before=self.zoom_controls)
            self.prev_button.state(["!disabled"] if self._preview_page > 0 else ["disabled"])
            self.next_button.state(["!disabled"] if self._preview_page < count - 1 else ["disabled"])
        else:
            self.page_controls.pack_forget()
        if keep_view:  # Même diagramme (zoom, nouvelle option) : garder le même endroit en vue.
            center = (sum(self.canvas.xview()) / 2, sum(self.canvas.yview()) / 2)
        else:  # Nouveau diagramme plus grand que la zone : l'ouvrir sur son centre, pas sur un coin vide.
            center = (0.5, 0.5)
        self._place_image()
        self.canvas.update_idletasks()
        for view, moveto, first_last in ((center[0], self.canvas.xview_moveto, self.canvas.xview()),
                                         (center[1], self.canvas.yview_moveto, self.canvas.yview())):
            visible = first_last[1] - first_last[0]
            moveto(max(0.0, min(1.0 - visible, view - visible / 2)))

    def _place_image(self) -> None:
        self.canvas.delete("all")
        if not self._preview_pages:
            return
        image = self._preview_pages[self._preview_page]
        pad = self.sp("xl")
        width, height = image.width(), image.height()
        canvas_w, canvas_h = self.canvas.winfo_width(), self.canvas.winfo_height()
        x = max(pad, (canvas_w - width) // 2)
        y = pad + max(0, round((canvas_h - height - 2 * pad) / PHI ** 2))  # 1 : φ au-dessus et dessous.
        shadow = self.sp("xs")
        self.canvas.create_rectangle(x + shadow, y + shadow, x + width + shadow, y + height + shadow,
                                     fill=C["border"], outline="")
        self.canvas.create_rectangle(x - 1, y - 1, x + width, y + height, fill="#ffffff", outline=C["border_strong"])
        self.canvas.create_image(x, y, image=image, anchor="nw")
        self.canvas.configure(scrollregion=(0, 0, max(canvas_w, x + width + pad), max(canvas_h, y + height + pad)))

    def _wheel(self, event) -> None:
        if event.state & 0x0004:  # Ctrl : zoom (géré par la liaison dédiée sous Windows).
            return
        self.canvas.yview_scroll(-event.delta // 120 or (-1 if event.delta > 0 else 1), "units")

    def _set_zoom(self, zoom: float) -> None:
        self.preview_zoom = max(0.1, min(4.0, zoom))
        self.zoom_var.set(f"{round(self.preview_zoom * 100)} %")
        # Rendu à la nouvelle résolution après une courte pause (plusieurs clics = un seul rendu).
        if self._zoom_job is not None:
            self.root.after_cancel(self._zoom_job)
        self._zoom_job = self.root.after(250, self._zoom_render)

    def _zoom_render(self) -> None:
        self._zoom_job = None
        self._request_preview()

    def _step_zoom(self, direction: int) -> None:
        current = self.preview_zoom
        if direction > 0:
            target = next((z for z in self.ZOOMS if z > current + 0.01), self.ZOOMS[-1])
        else:
            target = next((z for z in reversed(self.ZOOMS) if z < current - 0.01), self.ZOOMS[0])
        self._set_zoom(target)

    def _fit_zoom(self, minimum: float = 0.1) -> None:
        if not self._preview_pages:
            return
        image = self._preview_pages[self._preview_page]
        natural_w = image.width() / self.preview_zoom
        natural_h = image.height() / self.preview_zoom
        pad = 2 * self.sp("xl")
        fit = min((self.canvas.winfo_width() - pad) / natural_w, (self.canvas.winfo_height() - pad) / natural_h)
        self._set_zoom(max(minimum, min(fit, 2.0)))

    def _refresh_native_button(self) -> None:
        """« Accélérer l'aperçu » : moteur Java en service alors qu'un PlantUML natif existe pour ce système."""
        offer = (self.engine is not None and self.engine.kind == "java" and render.native_build() is not None
                 and not self._installing)
        if offer and not self.native_button.winfo_ismapped():
            self.native_button.pack(side="right", padx=(0, self.sp("m")))
        elif not offer:
            self.native_button.pack_forget()

    def upgrade_engine(self) -> None:
        build = render.native_build()
        if build is None or self._installing:
            return
        if not messagebox.askyesno(
                "simp2puml", f"L'aperçu utilise PlantUML avec Java. La version native (sans Java) dessine les "
                             f"petits diagrammes environ 100 fois plus vite.\n\nTélécharger PlantUML natif "
                             f"(≈ {build[2]} Mo, somme SHA-256 vérifiée) ?", parent=self.root):
            return
        self._upgrading = True
        self.native_button.pack_forget()
        self.install_engine()

    def install_engine(self) -> None:
        if self._installing:
            return
        self._installing = True
        self.install_button.state(["disabled"])
        self.install_progress.pack(side="left", padx=(self.sp("l"), 0), before=self.install_label)
        self.install_label.configure(text="Préparation…", fg=C["muted"])
        generation, put = self._preview_generation, self._preview_queue.put

        def progress(text: str, fraction: float | None) -> None:
            put((generation, "progress", (text, fraction)))

        def work() -> None:
            try:
                engine = render.install(progress)
            except (OSError, render.RenderError) as exc:
                put((generation, "install-error", str(exc)))
            else:
                put((generation, "installed", engine))

        threading.Thread(target=work, daemon=True).start()

    def export_image(self) -> None:
        request = self._request()
        if request is None:
            return
        if self.engine is None:
            self._select_tab("Aperçu")
            return
        fmt = next(key for key, (label, _, _) in render.FORMATS.items() if label == self.export_var.get())
        label, extension, _ = render.FORMATS[fmt]
        name = filedialog.asksaveasfilename(
            parent=self.root, title="Exporter l'image du diagramme",
            initialdir=str(request.source.parent), initialfile=request.source.stem + extension,
            defaultextension=extension, filetypes=[(label, f"*{extension}")], confirmoverwrite=False)
        if not name:
            return
        output = Path(name)
        if output.suffix.lower() != extension:
            output = output.with_name(output.name + extension)
        engine, result, generation = self.engine, self.result, self._preview_generation
        put = self._preview_queue.put
        override = self._edited_plantuml()  # L'image exportée correspond au texte modifié.
        self.export_button.state(["disabled"])
        self._set_status("Export de l'image…", "busy")

        def work() -> None:
            try:
                text = override if override is not None else preview_text(request, result)
                if fmt == "svg":  # Processus SVG déjà ouvert pour l'aperçu : quelques dizaines de ms.
                    rendering = render.preview_renderer(engine).svg_pages(text)
                    if rendering.error:
                        raise render.RenderError(rendering.error)
                else:  # Processus isolé : un aperçu ne peut pas interrompre un export.
                    rendering = render.render(text, fmt, engine)
                paths = save_images(rendering.pages, output)
            except (OSError, UnicodeError, ConversionError, render.RenderError) as exc:
                put((generation, "export-error", str(exc)))
            else:
                put((generation, "exported", paths))

        threading.Thread(target=work, daemon=True).start()

    def _build_warnings(self, parent) -> "tk.Frame":
        frame = tk.Frame(parent, bg=C["surface"])
        frame.rowconfigure(0, weight=1)
        frame.columnconfigure(0, weight=1)
        self.warnings_text = tk.Text(frame, bg=C["surface"], fg=C["ink"], bd=0, highlightthickness=0,
                                     wrap="word", font=self.f_ui, padx=self.sp("xl"), pady=self.sp("l"),
                                     spacing1=self.sp("xs"), spacing3=self.sp("m"), cursor="arrow",
                                     selectbackground=C["gold_soft"], selectforeground=C["ink"])
        self.warnings_text.grid(row=0, column=0, sticky="nsew")
        scroll = ttk.Scrollbar(frame, orient="vertical", style="Light.Vertical.TScrollbar",
                               command=self.warnings_text.yview)
        scroll.grid(row=0, column=1, sticky="ns")
        self.warnings_text.configure(yscrollcommand=scroll.set, state="disabled")
        indent = self.px(26)
        self.warnings_text.tag_configure("item", lmargin1=0, lmargin2=indent)
        self.warnings_text.tag_configure("icon", foreground=C["warn"], font=self.f_ui_bold)
        self.warnings_text.tag_configure("scope", foreground=C["muted"], font=self.f_ui_bold)
        self.warnings_text.tag_configure("ok", foreground=C["ok"], font=self.f_ui_bold)
        self.warnings_text.tag_configure("note", foreground=C["muted"], font=self.f_small,
                                         spacing1=self.sp("l"))
        return frame

    def _build_summary(self, parent) -> "tk.Frame":
        frame = tk.Frame(parent, bg=C["surface"], padx=self.sp("xl"), pady=self.sp("xl"))
        self.tiles = tk.Frame(frame, bg=C["surface"])
        self.tiles.pack(fill="x", anchor="n")
        self.tiles.bind("<Configure>", self._size_tiles)
        self.output_path = tk.Label(frame, text="", font=self.f_small, fg=C["muted"], bg=C["surface"],
                                    justify="left", anchor="w")
        self.output_path.pack(fill="x", pady=(self.sp("l"), 0))
        self.summary_note = tk.Label(frame, font=self.f_small, fg=C["faint"], bg=C["surface"], justify="left",
                                     anchor="w", text="Décomptes indicatifs : le nombre d'avertissements "
                                                      "n'est pas le nombre d'éléments perdus.")
        self.summary_note.pack(fill="x", side="bottom")
        return frame

    def _build_empty(self, card) -> "tk.Frame":
        """Comme un cartel de musée : le tableau à gauche, le texte à droite, bloc en section dorée."""
        empty, content = self._golden(card, C["surface"])
        portrait = self._portrait(144)
        if portrait is not None:
            self._framed(content, portrait, C["surface"]).pack(side="left", padx=(0, self.sp("xxl")))
        text = tk.Frame(content, bg=C["surface"])
        text.pack(side="left")
        tk.Label(text, text="Ouvrez un diagramme pour commencer", font=self.f_heading, fg=C["ink"],
                 bg=C["surface"]).pack(anchor="w")
        tk.Label(text, text="Projet Software Ideas Modeler (.simp) ou PlantUML (.puml).\n"
                            "La conversion se fait en mémoire : rien n'est écrit sans votre accord.",
                 font=self.f_ui, fg=C["muted"], bg=C["surface"], justify="left").pack(
            anchor="w", pady=(self.sp("s"), self.sp("xl")))
        row = tk.Frame(text, bg=C["surface"])
        row.pack(anchor="w")
        ttk.Button(row, text="Ouvrir un fichier…", style="Primary.TButton", command=self.browse).pack(side="left")
        tk.Label(row, text="ou Ctrl+O", font=self.f_small, fg=C["faint"], bg=C["surface"]).pack(
            side="left", padx=(self.sp("l"), 0))
        return empty

    def _build_status(self) -> None:
        bar = tk.Frame(self.root, bg=C["surface"], padx=self.sp("xl"), pady=self.sp("m"))
        bar.pack(side="bottom", fill="x")
        tk.Frame(self.root, bg=C["border"], height=1).pack(side="bottom", fill="x")
        size = self.sp("m")
        self.dot = tk.Canvas(bar, width=size, height=size, bg=C["surface"], highlightthickness=0)
        self.dot_item = self.dot.create_oval(1, 1, size - 1, size - 1, fill=C["muted"], outline="")
        self.dot.pack(side="left", padx=(0, self.sp("m")))
        self.status = tk.Label(bar, textvariable=self.status_var, font=self.f_small, fg=C["ink"],
                               bg=C["surface"], anchor="w")
        self.status.pack(side="left", fill="x", expand=True)
        self.progress = ttk.Progressbar(bar, mode="indeterminate", length=self.sp("xxl") * 3,
                                        style="Accent.Horizontal.TProgressbar")
        tk.Label(bar, textvariable=self.meta_var, font=self.f_small, fg=C["muted"], bg=C["surface"]).pack(
            side="right")
        self.progress_slot = bar

    def _bind_keys(self) -> None:
        self.root.bind("<Control-o>", lambda _e: self.browse())
        self.root.bind("<Control-s>", lambda _e: self.save_as())
        self.root.bind("<F5>", lambda _e: self.reload())
        self.root.bind("<Control-Shift-C>", lambda _e: self.copy())
        self.root.bind("<Control-comma>", lambda _e: self.open_preferences())
        for index, name in enumerate(self.TABS, start=1):
            self.root.bind(f"<Control-Key-{index}>", lambda _e, n=name: self._select_tab(n))

    # Préférences ---------------------------------------------------------------------------
    def open_preferences(self) -> None:
        """Choix de l'interface : il s'applique aussitôt, comme dans la nouvelle interface."""
        existing = getattr(self, "preferences_window", None)
        if existing is not None and existing.winfo_exists():
            existing.lift()
            return
        window = tk.Toplevel(self.root, bg=C["surface"], padx=self.sp("xl"), pady=self.sp("l"))
        window.title("Préférences — simp2puml")
        window.transient(self.root)
        window.resizable(False, False)
        window.bind("<Escape>", lambda _e: window.destroy())
        self.preferences_window = window
        ttk.Label(window, text="INTERFACE", style="Field.Card.TLabel").pack(anchor="w", pady=(0, self.sp("s")))
        self.interface_var = tk.StringVar(value=preferences.CLASSIC)  # Celle de cette fenêtre.
        editor = interfaces.find_editor()
        new = ttk.Radiobutton(window, text="Nouvelle interface (éditeur avec aperçu en direct)",
                              value=preferences.EDITOR, variable=self.interface_var, style="Card.TRadiobutton",
                              command=lambda: self.switch_interface(preferences.EDITOR))
        new.pack(anchor="w")
        if editor is None:
            new.state(["disabled"])
        ttk.Radiobutton(window, text="Interface classique (convertisseur)", value=preferences.CLASSIC,
                        variable=self.interface_var, style="Card.TRadiobutton",
                        command=lambda: self.switch_interface(preferences.CLASSIC)).pack(anchor="w")
        note = ("La nouvelle interface n'est pas installée sur cet ordinateur : voir le README." if editor is None
                else "La conversion et le fichier de préférences sont communs aux deux interfaces.")
        ttk.Label(window, text=note, style="Muted.Card.TLabel").pack(anchor="w", pady=(self.sp("m"), 0))

    def switch_interface(self, choice: str) -> bool:
        """Change d'interface ; vrai si la nouvelle a pris le relais (cette fenêtre se ferme alors)."""
        if choice != preferences.EDITOR:
            self._save_interface(choice)
            return False
        editor = interfaces.find_editor()
        if editor is None:
            self._set_status("La nouvelle interface n'est pas installée sur cet ordinateur.", "error")
            return False
        if self._dirty and not messagebox.askyesno(
                "simp2puml", "Le résultat a été modifié à la main.\n\nChanger d'interface perdra ces modifications. "
                             "Continuer ?", icon="warning", default="no", parent=self.root):
            self.interface_var.set(preferences.CLASSIC)
            return False
        source = self.source_var.get()
        error = interfaces.start_editor(editor, [source] if source else [])
        if error is not None:
            self.interface_var.set(preferences.CLASSIC)
            self._set_status(f"Changement d'interface impossible : {_shorten(error, 120)}", "error")
            return False
        self._save_interface(choice)
        self.close()
        return True

    def _save_interface(self, choice: str) -> None:
        try:
            preferences.set_interface(choice)
        except OSError as exc:
            self._set_status(f"Préférence non enregistrée : {_shorten(str(exc), 120)}", "error")

    def _select_tab(self, name: str) -> None:
        self.tab_var.set(name)
        self._show_tab()

    def _show_tab(self) -> None:
        current = self.tab_var.get()
        for name, line in self.tab_lines.items():
            line.configure(bg=C["accent"] if name == current else C["surface"])
        self.page_frames[current].tkraise()
        if current == "Aperçu":
            if self._preview_busy:  # Rendu préparé en arrière-plan : il s'affichera dès qu'il sera prêt.
                self.preview_status_var.set("Rendu en cours…")
            elif getattr(self, "_preview_stale", True) or not self._preview_pages:
                self._request_preview()
            # Sinon l'aperçu préparé d'avance est à jour : rien à refaire.

    # Sélection de la source ---------------------------------------------------------------
    def browse(self) -> None:
        current = Path(self.source_var.get()) if self.source_var.get() else None
        name = filedialog.askopenfilename(
            parent=self.root, title="Ouvrir un diagramme",
            initialdir=str(current.parent) if current else None,
            filetypes=[("Diagrammes SIMP ou PlantUML", "*.simp *.puml"), FILE_TYPES["simp"],
                       FILE_TYPES["puml"], ("Tous les fichiers", "*.*")])
        if name:
            self.load(Path(name))

    def load(self, source: Path) -> None:
        """Charge la liste des diagrammes et choisit le format opposé par défaut."""
        if not self._confirm_discard():
            return
        self._set_dirty(False)  # Accord donné : la conversion suivante remplace le texte.
        self.source_var.set(str(source))
        self.root.title(f"{source.name} — simp2puml {__version__}")
        self.file_label.configure(text=source.name)
        self.folder_label.configure(text=_shorten(str(source.parent), 90))
        self.target_var.set(default_target(source))
        self._auto_fit = True
        self._read_diagrams(keep_selection=False)

    def reload(self) -> None:
        if self.source_var.get() and self._confirm_discard():
            self._set_dirty(False)
            self._read_diagrams(keep_selection=True)

    def _read_diagrams(self, keep_selection: bool) -> None:
        source = Path(self.source_var.get())
        previous = self._selected()
        self.diagrams = []
        if source.suffix.lower() == ".simp":
            try:
                self.diagrams = list_diagrams(cli._read_source(source))
            except (OSError, UnicodeError, ConversionError) as exc:
                self.diagram_box.configure(values=[])
                self._show(Result(Request(source, self.target_var.get()), error=str(exc)))
                return
        labels = [f"{name or '(sans nom)'}   ·   {kind}" for _, name, kind in self.diagrams]
        if len(self.diagrams) > 1:
            labels.append(ALL_DIAGRAMS)
        self.diagram_box.configure(values=labels)
        index = 0
        if keep_selection and previous is not None:
            selector, all_selected = previous
            if all_selected and len(self.diagrams) > 1:
                index = len(labels) - 1
            else:
                index = next((i for i, (ident, _, _) in enumerate(self.diagrams) if ident == selector), 0)
        self.diagram_var.set(labels[index] if labels else "")
        if labels:
            self.diagram_box.current(index)
        self._options_changed(confirmed=True)

    def _selected(self) -> tuple[str | None, bool] | None:
        """(identifiant, tous) pour la sélection courante, ou None sans diagramme."""
        if not self.diagrams:
            return None
        index = self.diagram_box.current()
        if index == len(self.diagrams):
            return None, True
        if 0 <= index < len(self.diagrams):
            return self.diagrams[index][0], False
        return None

    def _request(self) -> Request | None:
        if not self.source_var.get():
            return None
        selected = self._selected()
        selector, all_selected = selected if selected is not None else (None, False)
        return Request(Path(self.source_var.get()), self.target_var.get(), selector,
                       all_selected, self.strict_var.get())

    # Conversion ---------------------------------------------------------------------------
    def _options(self) -> tuple[str, int, bool]:
        return self.target_var.get(), self.diagram_box.current(), self.strict_var.get()

    def _confirm_discard(self) -> bool:
        """Avant une nouvelle conversion : ne jamais perdre des modifications sans l'accord de l'utilisateur."""
        if not self._dirty:
            return True
        return messagebox.askyesno(
            "simp2puml", "Le résultat a été modifié à la main.\n\nUne nouvelle conversion remplacera ces "
                         "modifications. Continuer ?\n\n(Pour les garder : Non, puis « Enregistrer sous… ».)",
            icon="warning", default="no", parent=self.root)

    def _options_changed(self, confirmed: bool = False) -> None:
        if not confirmed and not self._confirm_discard():
            target, index, strict = getattr(self, "_options_snapshot", self._options())
            self.target_var.set(target)
            self.strict_var.set(strict)
            if index >= 0 and self.diagrams:
                self.diagram_box.current(index)
            self._refresh_controls()
            return
        self._refresh_controls()
        self.convert()

    def _refresh_controls(self) -> None:
        source = self.source_var.get()
        suffix = Path(source).suffix.lower() if source else ""
        selected = self._selected()
        all_selected = bool(selected and selected[1])
        if all_selected and self.target_var.get() != "puml":
            self.target_var.set("puml")
        for key, button in self.target_buttons.items():
            # Une source .puml peut rester en PlantUML (texte tel quel) ; pas une source SIMP en SIMP.
            same = cli.TARGETS[key] == suffix and suffix != ".puml"
            disabled = not source or same or (all_selected and key != "puml")
            button.state(["disabled"] if disabled else ["!disabled"])
        self.diagram_box.configure(state="readonly" if self.diagrams else "disabled")
        if not self.diagrams:
            self.diagram_var.set("Sans objet pour une source PlantUML" if suffix == ".puml" else "—")
        self.reload_button.state(["!disabled"] if source and not self._busy else ["disabled"])
        dirty = getattr(self, "_dirty", False)
        publishable = self.result is not None and (self.result.publishable or dirty) and not self._busy
        self.save_button.state(["!disabled"] if publishable else ["disabled"])
        has_content = (self.result is not None and bool(self.result.content)) or dirty
        self.copy_button.state(["!disabled"] if has_content else ["disabled"])
        if source:
            self.empty.grid_remove()
        else:
            self.empty.grid(row=0, column=0, rowspan=4, sticky="nsew")
            self.empty.tkraise()

    def convert(self) -> None:
        request = self._request()
        if request is not None:
            self._start(lambda: run(request), "Conversion en cours…")

    def save_as(self) -> None:
        if self.result is None or self._busy:
            return
        edited = self.editor_text() if self._dirty else None
        if edited is None and not self.result.publishable:
            return
        request = self.result.request if edited is not None else self._request()
        if request is None:
            return
        extension = cli.TARGETS[request.target]
        name = filedialog.asksaveasfilename(
            parent=self.root, title="Enregistrer la version modifiée" if edited is not None else "Enregistrer la conversion",
            initialdir=str(request.source.parent), initialfile=request.source.stem + extension,
            defaultextension=extension, filetypes=[FILE_TYPES[request.target]],
            confirmoverwrite=False)
        if not name:
            return
        if edited is None:
            self._start(lambda: save(request, Path(name)), "Enregistrement…")
            return
        # Version modifiée à la main : enregistrée telle quelle (pas de reconversion, pas de mode strict).
        put = self._preview_queue.put
        self._set_status("Enregistrement de la version modifiée…", "busy")

        def work() -> None:
            try:
                put((0, "edit-saved", save_text(request, Path(name), edited)))
            except (OSError, ConversionError) as exc:
                put((0, "edit-save-error", str(exc)))

        threading.Thread(target=work, daemon=True).start()

    def copy(self) -> None:
        content = self.editor_text()  # Version affichée, modifications comprises.
        if content:
            self.root.clipboard_clear()
            self.root.clipboard_append(content)
            self.copy_button.configure(text="Copié ✓")
            self.root.after(1400, lambda: self.copy_button.configure(text="Copier"))
            self._set_status("Résultat copié dans le presse-papiers.", "ok")

    def _start(self, job, message: str) -> None:
        self._generation += 1
        generation = self._generation
        self._busy = True
        self._set_status(message, "busy")
        self.progress.pack(side="right", padx=(self.sp("l"), 0))
        self.progress.start(12)
        self._refresh_controls()
        # Valeurs Tk lues ici, dans le fil principal : Tk n'est pas sûr entre fils d'exécution.
        fallback = Request(Path(self.source_var.get()), self.target_var.get())
        put = self._results.put

        def work() -> None:
            try:
                result = job()
            except Exception as exc:  # pragma: no cover - filet de sécurité, affiché à l'utilisateur
                result = Result(fallback, error=f"erreur inattendue : {exc!r}")
            put((generation, result))

        threading.Thread(target=work, daemon=True).start()

    def _poll(self) -> None:
        try:
            while True:
                generation, result = self._results.get_nowait()
                if generation == self._generation:  # Ignorer les résultats d'options périmées.
                    self._busy = False
                    self.progress.stop()
                    self.progress.pack_forget()
                    self._show(result)
        except queue.Empty:
            pass
        try:
            while True:
                self._preview_message(*self._preview_queue.get_nowait())
        except queue.Empty:
            pass
        self.root.after(self.POLL_MS, self._poll)

    # Affichage ----------------------------------------------------------------------------
    def _show(self, result: Result) -> None:
        self.result = result
        content = result.content or ""
        self._fill_code(content, result.request.target)
        self._fill_warnings(result)
        self._fill_summary(result)
        count = len(result.warnings)
        self.tab_buttons["Avertissements"].configure(text=f"Avertissements  {count}" if count else "Avertissements")
        self.kind_label.configure(text=FORMAT_LABELS.get(result.request.target, "").split("·")[0].strip().upper()
                                  if result.content else "")
        self._update_meta(content)
        self._options_snapshot = self._options()

        if result.error is not None:
            self._banner(result.error[:1].upper() + result.error[1:], "error")
            self._set_status("La conversion n'a pas pu aboutir." if result.content is None
                             else "Résultat consultable, mais non enregistrable.", "error")
            if result.request.strict and result.warnings:
                self._select_tab("Avertissements")
        else:
            self._banner(None, None)
            if result.saved is not None:
                self._set_status(f"Enregistré : {result.saved}", "ok")
            elif result.warnings:
                self._set_status(f"Conversion terminée avec {count} avertissement{'s' if count > 1 else ''} — "
                                 "consultez l'onglet Avertissements.", "warn")
            else:
                self._set_status("Conversion terminée, aucun avertissement détecté.", "ok")
        self._refresh_controls()
        if result.saved is not None:
            messagebox.showinfo("simp2puml", f"Fichier enregistré :\n{result.saved}", parent=self.root)
        else:
            self._request_preview()  # Aperçu « live » : suit chaque changement d'option.

    def _banner(self, message: str | None, level: str | None) -> None:
        if message is None:
            self.banner.grid_remove()
            return
        self.banner_text.configure(text=message)
        self.banner.grid(row=2, column=0, sticky="ew")

    def _fill_code(self, content: str, target: str) -> None:
        """Nouveau résultat de conversion : remplace le texte, réinitialise annulation et modifications."""
        text = self.output_text
        self._filling = True
        try:
            text.delete("1.0", "end")
            text.insert("1.0", content)
            text.edit_reset()  # Ctrl+Z ne revient pas à la conversion précédente.
            text.edit_modified(False)
        finally:
            self._filling = False
        self._generated, self._code_target = content, target
        self._highlight(1, min(HIGHLIGHT_LINES, int(text.index("end-1c").split(".")[0])))
        self._gutter_lines = -1
        self._sync_gutter()
        self._mark_error_line(None)
        text.mark_set("insert", "1.0")
        text.yview_moveto(0)
        self.layout_button.state(["!disabled"] if target == "puml" and content else ["disabled"])
        self._set_dirty(False, force=True)

    def _fill_warnings(self, result: Result) -> None:
        widget = self.warnings_text
        widget.configure(state="normal")
        widget.delete("1.0", "end")
        if result.content is None and result.error is not None:
            widget.insert("end", "Aucune conversion à analyser.", "note")
        elif not result.warnings:
            widget.insert("end", "✓  ", ("item", "ok"))
            widget.insert("end", "Aucun avertissement détecté.\n", "item")
            widget.insert("end", "Vérifiez tout de même le rendu dans l'outil cible : seules les pertes "
                                 "détectées sont signalées.", "note")
        else:
            for warning in result.warnings:
                widget.insert("end", "▲   ", ("item", "icon"))
                if result.request.all and " : " in warning:
                    scope, rest = warning.split(" : ", 1)
                    widget.insert("end", f"{scope}  ", ("item", "scope"))
                    widget.insert("end", rest + "\n", "item")
                else:
                    widget.insert("end", warning + "\n", "item")
        widget.configure(state="disabled")

    def _fill_summary(self, result: Result) -> None:
        for child in self.tiles.winfo_children():
            child.destroy()
        stats = [line.split(" : ", 1) for line in result.summary if " : " in line]
        columns = 4
        for index, (label, value) in enumerate(stats):
            warn = label == "Avertissements" and value != "0"
            tile = tk.Frame(self.tiles, bg=C["warn_soft"] if warn else C["surface_alt"],
                            highlightthickness=1, highlightbackground=C["gold"] if warn else C["border"],
                            padx=self.sp("xl"), pady=self.sp("l"))
            tile.grid(row=index // columns, column=index % columns, sticky="nsew",
                      padx=(0, self.sp("l")), pady=(0, self.sp("l")))
            body = tk.Frame(tile, bg=tile["bg"])  # Centré verticalement dans la tuile.
            body.pack(expand=True, fill="x")
            tk.Label(body, text=value, font=self.f_stat, fg=C["warn"] if warn else C["accent"],
                     bg=tile["bg"]).pack(anchor="w")
            tk.Label(body, text=label, font=self.f_small, fg=C["muted"], bg=tile["bg"]).pack(anchor="w")
        for column in range(columns):
            self.tiles.columnconfigure(column, weight=1, uniform="tiles")
        self._tile_rows = (len(stats) + columns - 1) // columns
        self._size_tiles()
        if not stats:
            tk.Label(self.tiles, text="Aucun résumé disponible.", font=self.f_ui, fg=C["muted"],
                     bg=C["surface"]).grid(row=0, column=0, sticky="w")
        self.output_path.configure(text=f"Sortie : {result.saved}" if result.saved else "")

    def _size_tiles(self, _event=None) -> None:
        """Tuiles au rapport largeur : hauteur = φ², sans descendre sous la hauteur de leur contenu."""
        columns = 4
        width = self.tiles.winfo_width()
        if width <= 1:
            return
        tile_width = (width - columns * self.sp("l")) / columns
        height = round(tile_width / PHI ** 2) + self.sp("l")  # + marge basse de la rangée.
        for row in range(getattr(self, "_tile_rows", 0)):
            self.tiles.rowconfigure(row, minsize=height)

    def _set_status(self, message: str, level: str) -> None:
        colors = {"ok": C["ok"], "warn": C["warn"], "error": C["error"], "busy": C["gold"], "idle": C["faint"]}
        self.status_var.set(message)
        self.dot.itemconfigure(self.dot_item, fill=colors.get(level, C["muted"]))
        self.status.configure(fg=C["error"] if level == "error" else C["ink"])


def _prepare_windows() -> None:
    """Affichage net sur écrans haute densité et icône propre dans la barre des tâches."""
    if sys.platform != "win32":
        return
    try:
        import ctypes

        ctypes.windll.shcore.SetProcessDpiAwareness(1)
    except (AttributeError, OSError):
        pass
    try:
        import ctypes

        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(f"simp2puml.gui.{__version__}")
    except (AttributeError, OSError):
        pass


def main(argv: list[str] | None = None) -> int:
    """Ouvre l'interface classique ; `launcher.main` choisit l'interface au lancement."""
    args = list(sys.argv[1:] if argv is None else argv)
    if args and args[0] in ("-h", "--help", "--version"):
        from .launcher import main as launcher_main

        return launcher_main(args)
    if args[:2] == ["--interface", preferences.CLASSIC]:
        args = args[2:]
    if tk is None:
        print("Erreur : Tkinter n'est pas disponible dans cette installation Python.", file=sys.stderr)
        return 1
    _prepare_windows()
    try:
        root = tk.Tk(className="simp2puml")
    except tk.TclError as exc:
        print(f"Erreur : impossible d'ouvrir une fenêtre ({exc}).", file=sys.stderr)
        return 1
    App(root, Path(args[0]) if args else None)
    root.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
