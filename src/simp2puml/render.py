"""Rendu d'images PlantUML (aperçu et export) avec un moteur local, le plus rapide disponible.

Moteurs, par ordre de préférence :
1. PlantUML **natif** (exécutable compilé GraalVM publié par PlantUML : Windows x64, Linux x64/arm64,
   macOS arm64) : pas de Java, démarrage en ~0,1 s ;
2. Java + plantuml.jar (repli, par exemple macOS Intel).

Pour l'aperçu, deux processus restent ouverts en mode `-pipe` (`PreviewRenderer`) : un en SVG, qui
donne la mise en page et la taille en quelques dizaines de millisecondes, un en PNG, qui dessine
directement à la résolution affichée. Un petit diagramme est rendu en quelques millisecondes ;
la rastérisation PNG d'un grand diagramme reste le seul coût notable, d'où le rendu à la taille vue.

Rien n'est envoyé à un serveur. Profil de sécurité PlantUML `SANDBOX` : aucun fichier local ni URL
n'est lu (`SECURE` autorise encore `!include` de .puml locaux) ; la bibliothèque standard intégrée
(`!include <C4/...>`) reste disponible. Sans Graphviz, mise en page `smetana` intégrée.

`install` télécharge ce qui manque dans le dossier de données de l'utilisateur (partagé avec les
lanceurs) et vérifie les sommes SHA-256 : fixées ici pour PlantUML (natif et jar), fournies par
l'API Adoptium pour Java. Bibliothèque standard uniquement ; les modules de téléchargement et
d'archives ne sont importés qu'à l'installation (démarrage plus rapide).
"""
from __future__ import annotations

import atexit
from collections.abc import Callable
from dataclasses import dataclass, field
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading

PLANTUML_VERSION = "1.2026.9"
_RELEASE = f"https://github.com/plantuml/plantuml/releases/download/v{PLANTUML_VERSION}/"
PLANTUML_URL = f"{_RELEASE}plantuml-{PLANTUML_VERSION}.jar"
PLANTUML_SHA256 = "aa4a1d06f8911c4b842beff4828dd955be431692b631a312d04c0e9fab9581de"
PLANTUML_SIZE_MB = 29
# Exécutables natifs publiés avec la même version de PlantUML : (archive, SHA-256, taille en Mo).
NATIVE_BUILDS = {
    ("windows", "x64"): ("native-plantuml-windows-amd64-1.2026.9.zip",
                         "240d2cdf8e83dc3557dae771d08f04e6765a6889feba16433b34f2c866b6cc4a", 48),
    ("linux", "x64"): ("native-plantuml-linux-amd64-1.2026.9.zip",
                       "4bf6d1a3efee62e5e42a4f950ac4b6c11c6455a37fc1a0f784fb5867ca8c2ff0", 49),
    ("linux", "aarch64"): ("native-plantuml-linux-arm64-1.2026.9.zip",
                           "04d51abe63f33a18c6b180f8e6c4c65449c1422f47f4dd0dd30261a1e4aed5e2", 48),
    ("mac", "aarch64"): ("native-plantuml-macos-arm64-1.2026.9.zip",
                         "3315869dbfee61971f63af79afff0857308f29ea0f6bfca4c322d3d6a211d770", 52),
}
JAVA_FEATURE = 21
JAVA_MINIMUM = 11
JAVA_SIZE_MB = 48
ADOPTIUM_API = ("https://api.adoptium.net/v3/assets/latest/{feature}/hotspot"
                "?image_type=jre&vendor=eclipse&os={os}&architecture={arch}")
TIMEOUT = 90
LIMIT_SIZE = "16384"
DELIMITER = b"--simp2puml-fin-de-page-5c0f1e--"

# Format PlantUML, libellé, extension, résolution (None : défaut PlantUML).
FORMATS = {
    "png": ("PNG", ".png", None),
    "png2x": ("PNG haute résolution (×2)", ".png", 192),
    "svg": ("SVG (vectoriel)", ".svg", None),
    "eps": ("EPS (vectoriel)", ".eps", None),
    "txt": ("Texte ASCII", ".txt", None),
}
Progress = Callable[[str, float | None], None]
_ACTIVE: set[subprocess.Popen] = set()
_LOCK = threading.Lock()
_KILLED: set[subprocess.Popen] = set()


class RenderError(Exception):
    """Moteur absent, rendu impossible ou téléchargement invalide."""


@dataclass(frozen=True)
class Engine:
    """Commande de base pour lancer PlantUML ; `kind` vaut « native » ou « java »."""
    argv: tuple[str, ...]
    kind: str

    @classmethod
    def java(cls, java: Path, jar: Path) -> "Engine":
        return cls((str(java), "-Djava.awt.headless=true", "-Dfile.encoding=UTF-8",
                    "-DPLANTUML_SECURITY_PROFILE=SANDBOX", f"-DPLANTUML_LIMIT_SIZE={LIMIT_SIZE}",
                    "-jar", str(jar)), "java")

    @classmethod
    def native(cls, executable: Path) -> "Engine":
        return cls((str(executable), "-DPLANTUML_SECURITY_PROFILE=SANDBOX",
                    f"-DPLANTUML_LIMIT_SIZE={LIMIT_SIZE}"), "native")

    @property
    def label(self) -> str:
        return f"PlantUML {PLANTUML_VERSION} natif" if self.kind == "native" else f"PlantUML {PLANTUML_VERSION} (Java)"


@dataclass
class Rendering:
    """Pages produites (une par bloc @startuml) ; `error` signale un diagramme invalide."""
    pages: list[bytes]
    error: str | None = None
    sizes: list[tuple[int, int]] = field(default_factory=list)


def data_dir() -> Path:
    """Dossier de données partagé avec les lanceurs (`launchers/`)."""
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local")
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local" / "share")
    return base / "simp2puml"


def _environment() -> dict[str, str]:
    # Le profil SANDBOX est aussi passé en propriété : les deux formes sont reconnues par PlantUML.
    return dict(os.environ, PLANTUML_SECURITY_PROFILE="SANDBOX", PLANTUML_LIMIT_SIZE=LIMIT_SIZE)


def _no_window() -> dict:
    """Pas de fenêtre de console sous Windows quand l'interface tourne avec pythonw."""
    return {"creationflags": subprocess.CREATE_NO_WINDOW} if sys.platform == "win32" else {}


def _platform() -> tuple[str, str | None]:
    import platform

    system = {"win32": "windows", "darwin": "mac"}.get(sys.platform, "linux")
    machine = platform.machine().lower()
    return system, {"amd64": "x64", "x86_64": "x64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine)


# Détection -----------------------------------------------------------------------------------
def java_version(output: str) -> int | None:
    """Version majeure lue dans la sortie de `java -version` (« 1.8.0_x » vaut 8)."""
    match = re.search(r'version "(\d+)(?:\.(\d+))?', output)
    if not match:
        return None
    major = int(match.group(1))
    return int(match.group(2) or 0) if major == 1 else major


def _runs(argv: list[str], check: Callable[[str], bool]) -> bool:
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=30, env=_environment(),
                                **_no_window())
    except (OSError, subprocess.SubprocessError):
        return False
    return result.returncode == 0 and check(result.stdout + result.stderr)


def _usable_java(path: Path) -> bool:
    return _runs([str(path), "-version"], lambda out: (java_version(out) or 0) >= JAVA_MINIMUM)


def _java_name() -> str:
    return "java.exe" if sys.platform == "win32" else "java"


def _managed_javas() -> list[Path]:
    root = data_dir() / "java"
    if not root.is_dir():
        return []
    found = []
    for home in sorted(root.iterdir(), reverse=True):
        for candidate in (home / "bin" / _java_name(), home / "Contents" / "Home" / "bin" / _java_name()):
            if candidate.is_file():
                found.append(candidate)
    return found


def find_java() -> Path | None:
    candidates: list[Path] = []
    if os.environ.get("SIMP2PUML_JAVA"):
        candidates.append(Path(os.environ["SIMP2PUML_JAVA"]))
    candidates += _managed_javas()
    if os.environ.get("JAVA_HOME"):
        candidates.append(Path(os.environ["JAVA_HOME"]) / "bin" / _java_name())
    on_path = shutil.which("java")
    # Sur macOS sans JDK, /usr/bin/java ouvre une boîte de dialogue d'installation : l'éviter.
    if on_path and not (sys.platform == "darwin" and on_path == "/usr/bin/java" and not _macos_has_jdk()):
        candidates.append(Path(on_path))
    return next((c for c in candidates if c.is_file() and _usable_java(c)), None)


def _macos_has_jdk() -> bool:
    try:
        return subprocess.run(["/usr/libexec/java_home"], capture_output=True, timeout=10).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def managed_jar() -> Path:
    return data_dir() / "plantuml" / f"plantuml-{PLANTUML_VERSION}.jar"


def find_jar() -> Path | None:
    configured = os.environ.get("PLANTUML_JAR")
    for candidate in ([Path(configured)] if configured else []) + [managed_jar()]:
        if candidate.is_file():
            return candidate
    return None


def native_build() -> tuple[str, str, int] | None:
    """Archive native publiée pour cette plateforme, s'il y en a une."""
    return NATIVE_BUILDS.get(_platform())


def native_dir() -> Path:
    return data_dir() / f"plantuml-natif-{PLANTUML_VERSION}"


def _native_executable(root: Path) -> Path | None:
    name = "plantuml.exe" if sys.platform == "win32" else "plantuml"
    return next((p for p in sorted(root.rglob(name)) if p.is_file()), None) if root.is_dir() else None


_VERIFIED: dict[str, bool] = {}


def _usable_native(executable: Path) -> bool:
    """Vérifie que l'exécutable démarre (`-version`, ~0,2 s), une seule fois tant qu'il ne change pas.

    Le résultat positif est gardé à côté de lui (taille, date, version) : les lancements suivants
    de l'interface n'attendent plus cette vérification.
    """
    key = str(executable)
    if key in _VERIFIED:
        return _VERIFIED[key]
    marker = executable.parent / ".simp2puml-verifie"
    try:
        stat = executable.stat()
        signature = f"{stat.st_size}:{stat.st_mtime_ns}:{PLANTUML_VERSION}"
    except OSError:
        signature = None
    try:
        if signature and marker.read_text(encoding="ascii") == signature:
            _VERIFIED[key] = True
            return True
    except OSError:
        pass  # Pas encore vérifié (ou marqueur illisible) : vérification ci-dessous.
    _VERIFIED[key] = _runs([key, "-version"], lambda out: "PlantUML version" in out)
    if _VERIFIED[key] and signature:
        try:
            marker.write_text(signature, encoding="ascii")
        except OSError:
            pass  # Dossier en lecture seule : on revérifiera au prochain lancement.
    return _VERIFIED[key]


def find_native() -> Path | None:
    configured = os.environ.get("SIMP2PUML_PLANTUML")
    candidates = ([Path(configured)] if configured else []) + [_native_executable(native_dir())]
    return next((c for c in candidates if c and c.is_file() and _usable_native(c)), None)


def find_engine() -> Engine | None:
    if os.environ.get("PLANTUML_JAR") and not os.environ.get("SIMP2PUML_PLANTUML"):
        # Un jar explicitement configuré (tests, installation existante) reste prioritaire.
        java, jar = find_java(), find_jar()
        if java and jar:
            return Engine.java(java, jar)
    native = find_native()
    if native:
        return Engine.native(native)
    java, jar = find_java(), find_jar()
    return Engine.java(java, jar) if java and jar else None


def missing_parts() -> list[tuple[str, int]]:
    """Composants à télécharger, avec leur taille approximative en Mo."""
    if find_native() is not None:
        return []
    build = native_build()
    if build is not None:
        return [(f"PlantUML {PLANTUML_VERSION} natif, sans Java", build[2])]
    parts = []
    if find_java() is None:
        parts.append((f"Java {JAVA_FEATURE} (Eclipse Temurin)", JAVA_SIZE_MB))
    if find_jar() is None:
        parts.append((f"PlantUML {PLANTUML_VERSION}", PLANTUML_SIZE_MB))
    return parts


# Téléchargement ----------------------------------------------------------------------------
def _download(url: str, destination: Path, sha256: str, label: str, progress: Progress | None) -> None:
    """Télécharge vers `destination` seulement si la somme SHA-256 correspond."""
    import urllib.request

    destination.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256()
    temp = destination.with_name(destination.name + ".part")
    request = urllib.request.Request(url, headers={"User-Agent": "simp2puml"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response, temp.open("wb") as stream:
            total = int(response.headers.get("Content-Length") or 0)
            done = 0
            while chunk := response.read(256 * 1024):
                stream.write(chunk)
                digest.update(chunk)
                done += len(chunk)
                if progress:
                    progress(f"Téléchargement de {label}…", done / total if total else None)
        if digest.hexdigest() != sha256.lower():
            raise RenderError(f"somme de contrôle invalide pour {label} : téléchargement refusé")
        os.replace(temp, destination)
    except OSError as exc:
        raise RenderError(f"téléchargement de {label} impossible : {exc}") from exc
    finally:
        temp.unlink(missing_ok=True)


def _safe_members(names: list[str]) -> None:
    for name in names:
        path = PurePosixPath(name.replace("\\", "/"))
        if path.is_absolute() or ".." in path.parts:
            raise RenderError(f"archive refusée : chemin suspect {name}")


def _extract(archive: Path, target: Path) -> None:
    import tarfile
    import zipfile

    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            _safe_members(bundle.namelist())
            bundle.extractall(target)
    else:
        with tarfile.open(archive) as bundle:
            _safe_members(bundle.getnames())
            if hasattr(tarfile, "data_filter"):
                bundle.extractall(target, filter="data")
            else:  # pragma: no cover - Python 3.11 sans filtre
                bundle.extractall(target)


def _install_archive(url: str, sha256: str, label: str, final: Path, progress: Progress | None) -> None:
    """Télécharge, vérifie et extrait une archive, puis la publie d'un bloc dans `final`."""
    final.parent.mkdir(parents=True, exist_ok=True)
    archive = final.parent / url.rsplit("/", 1)[-1]
    _download(url, archive, sha256, label, progress)
    if progress:
        progress(f"Installation de {label}…", None)
    staging = Path(tempfile.mkdtemp(prefix=".extract-", dir=final.parent))
    try:
        _extract(archive, staging)
        if final.exists():
            shutil.rmtree(final)
        os.replace(staging, final)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
        archive.unlink(missing_ok=True)


def install_native(progress: Progress | None = None) -> Path | None:
    """Installe l'exécutable natif ; None s'il n'existe pas pour cette plateforme ou ne démarre pas."""
    build = native_build()
    if build is None:
        return None
    name, sha256, _ = build
    _install_archive(_RELEASE + name, sha256, f"PlantUML {PLANTUML_VERSION} natif", native_dir(), progress)
    executable = _native_executable(native_dir())
    if executable is None:
        return None
    if sys.platform != "win32":  # zipfile ne conserve pas le bit d'exécution.
        for path in native_dir().rglob("*"):
            if path.is_file() and (path == executable or path.suffix in ("", ".so", ".dylib")):
                path.chmod(path.stat().st_mode | 0o111)
    _VERIFIED.pop(str(executable), None)
    return executable if _usable_native(executable) else None


def install_java(progress: Progress | None = None) -> Path:
    import json
    import urllib.request

    system, arch = _platform()
    if arch is None:
        raise RenderError("architecture non prise en charge pour Java")
    url = ADOPTIUM_API.format(feature=JAVA_FEATURE, os=system, arch=arch)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "simp2puml"})
        with urllib.request.urlopen(request, timeout=30) as response:
            package = json.load(response)[0]["binary"]["package"]
    except (OSError, ValueError, KeyError, IndexError) as exc:
        raise RenderError(f"impossible de trouver Java {JAVA_FEATURE} pour {system}/{arch} : {exc}") from exc
    root = data_dir() / "java"
    staging_target = root / f".{package['name']}"
    _install_archive(package["link"], package["checksum"], f"Java {JAVA_FEATURE}", staging_target, progress)
    homes = [p for p in staging_target.iterdir() if p.is_dir()]
    if len(homes) != 1:
        shutil.rmtree(staging_target, ignore_errors=True)
        raise RenderError("archive Java inattendue")
    final = root / homes[0].name
    if final.exists():
        shutil.rmtree(final)
    os.replace(homes[0], final)
    shutil.rmtree(staging_target, ignore_errors=True)
    java = next((j for j in _managed_javas() if _usable_java(j)), None)
    if java is None:
        raise RenderError("Java installé mais inutilisable")
    return java


def install(progress: Progress | None = None) -> Engine:
    """Installe le moteur le plus rapide disponible : natif si possible, sinon Java + jar."""
    native = find_native()
    if native is None and native_build() is not None:
        native = install_native(progress)
    if native is not None:
        engine = Engine.native(native)
    else:
        java = find_java() or install_java(progress)
        jar = find_jar()
        if jar is None:
            _download(PLANTUML_URL, managed_jar(), PLANTUML_SHA256, f"PlantUML {PLANTUML_VERSION}", progress)
            jar = managed_jar()
        engine = Engine.java(java, jar)
    if progress:
        progress("Moteur d'aperçu prêt.", 1.0)
    return engine


# Rendu ponctuel (exports, repli) --------------------------------------------------------------
def _layout_args() -> list[str]:
    args = ["-charset", "UTF-8", "-nometadata"]
    if not (os.environ.get("GRAPHVIZ_DOT") or shutil.which("dot")):
        args.append("-Playout=smetana")
    return args


def command(engine: Engine, fmt: str, source: Path, output_dir: Path, dpi: int | None = None) -> list[str]:
    plantuml_format = "png" if fmt.startswith("png") else fmt
    args = [*engine.argv, f"-t{plantuml_format}", *_layout_args(), "-o", str(output_dir)]
    if dpi and dpi != 96:  # Résolution passée en option : le texte et ses numéros de ligne restent intacts.
        args.append(f"-Sdpi={int(dpi)}")
    return args + [str(source)]


def _clean_message(raw: str) -> str:
    """Message PlantUML sans le chemin du fichier temporaire."""
    lines = []
    for line in raw.splitlines():
        match = re.match(r"\s*Error line (\d+) in file:", line)
        if match:
            lines.append(f"Erreur de syntaxe PlantUML à la ligne {match.group(1)} (détail dans l'image).")
        elif line.strip() and "Some diagram description contains errors" not in line:
            lines.append(line.strip())
    return "\n".join(lines)


def _page_index(path: Path) -> int:
    match = re.search(r"_(\d{3})$", path.stem)
    return int(match.group(1)) if match else 0


def render(text: str, fmt: str = "png", engine: Engine | None = None, dpi: int | None = None) -> Rendering:
    """Rend chaque bloc @startuml dans un processus dédié ; un diagramme invalide produit l'image d'erreur."""
    if fmt not in FORMATS:
        raise RenderError(f"format inconnu : {fmt}")
    engine = engine or find_engine()
    if engine is None:
        raise RenderError("moteur d'aperçu absent")
    extension = {"txt": ".atxt"}.get(fmt, FORMATS[fmt][1])  # Extension écrite par PlantUML.
    dpi = dpi or FORMATS[fmt][2]
    with tempfile.TemporaryDirectory(prefix="simp2puml-render-", ignore_cleanup_errors=True) as work:
        work_dir = Path(work)
        source = work_dir / "diagramme.puml"
        source.write_text(text, encoding="utf-8")
        output_dir = work_dir / "sortie"
        output_dir.mkdir()
        try:
            process = subprocess.Popen(command(engine, fmt, source, output_dir, dpi), stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, cwd=work_dir, env=_environment(), **_no_window())
        except OSError as exc:
            raise RenderError(f"PlantUML ne démarre pas : {exc}") from exc
        with _LOCK:
            _ACTIVE.add(process)
        try:
            stdout, stderr = process.communicate(timeout=TIMEOUT)
        except subprocess.TimeoutExpired as exc:
            process.kill()
            process.communicate()
            raise RenderError(f"rendu trop long (plus de {TIMEOUT} s)") from exc
        finally:
            with _LOCK:
                _ACTIVE.discard(process)
                cancelled = process in _KILLED
                _KILLED.discard(process)
        if cancelled:
            raise RenderError("rendu annulé")
        files = sorted(output_dir.glob(f"*{extension}"), key=_page_index)
        pages = [f.read_bytes() for f in files]
        message = _clean_message((stderr or stdout).decode("utf-8", "replace"))
        if not pages:
            raise RenderError(message or f"PlantUML n'a produit aucune image (code {process.returncode})")
        error = None
        if process.returncode != 0:
            error = message or "PlantUML signale une erreur de syntaxe (voir l'image)."
        return Rendering(pages, error)


# Rendu continu (aperçu) -----------------------------------------------------------------------
_START = re.compile(rb"^[ \t]*@start\w+", re.M)


def count_diagrams(text: str) -> int:
    return len(_START.findall(text.encode("utf-8")))


def svg_size(svg: bytes) -> tuple[int, int] | None:
    """Taille naturelle (96 ppp) lue dans l'en-tête SVG produit par PlantUML."""
    head = svg[:2000].decode("utf-8", "replace")
    width = re.search(r'\bwidth="(\d+(?:\.\d+)?)px"', head)
    height = re.search(r'\bheight="(\d+(?:\.\d+)?)px"', head)
    return (round(float(width.group(1))), round(float(height.group(1)))) if width and height else None


def inject_dpi(text: str, dpi: int) -> tuple[str, list[int]]:
    """Ajoute `skinparam dpi` après chaque @startuml ; renvoie aussi les lignes insérées (pour les erreurs)."""
    if dpi == 96:
        return text, []
    out, inserted = [], []
    for line in text.split("\n"):
        out.append(line)
        if re.match(r"\s*@startuml\b", line):
            out.append(f"skinparam dpi {dpi}")
            inserted.append(len(out))  # Numéro (1-based) de la ligne ajoutée.
    return "\n".join(out), inserted


class PipeServer:
    """Processus PlantUML `-pipe` gardé ouvert : un rendu par bloc, sans redémarrage."""

    def __init__(self, engine: Engine, fmt: str):
        self.engine, self.fmt = engine, fmt
        self.process: subprocess.Popen | None = None
        self.responses: queue.Queue[bytes | None] = queue.Queue()
        self.lock = threading.Lock()

    def _start(self) -> subprocess.Popen:
        argv = [*self.engine.argv, "-pipe", f"-t{self.fmt}", "-pipeNoStderr", *_layout_args(),
                "-pipedelimitor", DELIMITER.decode("ascii")]
        process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env=_environment(), **_no_window())
        responses: queue.Queue[bytes | None] = queue.Queue()

        def reader() -> None:
            buffer = b""
            stream = process.stdout
            while True:
                chunk = stream.read1(1 << 16) if stream else b""
                if not chunk:
                    responses.put(None)
                    return
                buffer += chunk
                while DELIMITER in buffer:
                    page, _, buffer = buffer.partition(DELIMITER)
                    responses.put(page)

        threading.Thread(target=reader, daemon=True).start()
        self.process, self.responses = process, responses
        return process

    def alive(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def warm_up(self) -> None:
        """Démarre le processus et prépare le moteur (premier rendu plus lent)."""
        try:
            self.request("@startuml\nclass A\n@enduml\n")
        except RenderError:
            pass

    def request(self, text: str) -> list[bytes | str]:
        """Une entrée par bloc : les octets de l'image, ou le message d'erreur PlantUML (str)."""
        count = count_diagrams(text)
        if count == 0:
            raise RenderError("aucun bloc @startuml dans le texte")
        with self.lock:
            process = self.process if self.alive() else self._start()
            try:
                process.stdin.write(text.encode("utf-8") + (b"" if text.endswith("\n") else b"\n"))
                process.stdin.flush()
            except OSError as exc:
                self.stop()
                raise RenderError(f"PlantUML s'est arrêté : {exc}") from exc
            pages: list[bytes | str] = []
            for _ in range(count):
                try:
                    page = self.responses.get(timeout=TIMEOUT)
                except queue.Empty:
                    self.stop()
                    raise RenderError(f"rendu trop long (plus de {TIMEOUT} s)") from None
                if page is None:
                    self.stop()
                    raise RenderError("rendu annulé ou PlantUML arrêté")
                page = page.lstrip(b"\r\n") if not page.startswith(b"\x89PNG") else page
                if page.startswith(b"ERROR"):
                    parts = page.decode("utf-8", "replace").splitlines()
                    line = parts[1].strip() if len(parts) > 1 else "?"
                    pages.append(f"{line}\n" + " ".join(p.strip() for p in parts[2:] if p.strip()))
                else:
                    pages.append(page.rstrip(b"\r\n") if self.fmt == "svg" else page)
            return pages

    def stop(self) -> None:
        process, self.process = self.process, None
        if process is not None:
            try:
                process.kill()
            except OSError:
                pass


class PreviewRenderer:
    """Aperçu rapide : SVG pour la taille, PNG à la résolution affichée ; deux processus ouverts."""

    def __init__(self, engine: Engine):
        self.engine = engine
        self.svg = PipeServer(engine, "svg")
        self.png = PipeServer(engine, "png")

    def warm_up(self) -> None:
        self.svg.warm_up()
        self.png.warm_up()

    def sizes(self, text: str) -> tuple[list[tuple[int, int]], str | None]:
        """Tailles naturelles des pages (rapide) et éventuelle erreur de syntaxe."""
        sizes, error = [], None
        for page in self.svg.request(text):
            if isinstance(page, str):
                error = error or _pipe_error(page, [])
            else:
                sizes.append(svg_size(page) or (0, 0))
        return sizes, error

    def png_pages(self, text: str, dpi: int) -> Rendering:
        injected, inserted = inject_dpi(text, dpi)
        pages, errors = [], []
        for page in self.png.request(injected):
            (errors if isinstance(page, str) else pages).append(page)
        return Rendering(pages, _pipe_error(errors[0], inserted) if errors else None)

    def svg_pages(self, text: str) -> Rendering:
        pages, errors = [], []
        for page in self.svg.request(text):
            (errors if isinstance(page, str) else pages).append(page)
        return Rendering(pages, _pipe_error(errors[0], []) if errors else None)

    def interrupt(self) -> None:
        """Abandonne un rendu PNG en cours (devenu inutile) ; seul l'exécutable natif redémarre vite."""
        if self.engine.kind == "native" and self.png.lock.locked():
            self.png.stop()

    def stop(self) -> None:
        self.svg.stop()
        self.png.stop()


def _pipe_error(message: str, inserted: list[int]) -> str:
    line_text, _, detail = message.partition("\n")
    try:
        line = int(line_text) + 1  # En mode -pipe, PlantUML compte les lignes à partir de 0.
    except ValueError:
        return f"Erreur PlantUML : {detail or line_text}"
    line -= sum(1 for added in inserted if added < line)  # Lignes `skinparam dpi` ajoutées avant l'erreur.
    return f"Erreur PlantUML à la ligne {line} : {detail}".rstrip(" :")


class PreviewCache:
    """Cache disque des aperçus : rouvrir un diagramme déjà dessiné est immédiat, même après redémarrage.

    La clé couvre le texte exact, la résolution et le moteur (version de PlantUML, natif ou Java,
    Graphviz ou smetana) : une image n'est jamais réutilisée pour un rendu qui pourrait différer.
    Les erreurs de syntaxe ne sont pas gardées. Écriture atomique ; au-delà de `max_bytes`, les images
    les moins récemment utilisées sont supprimées.
    """

    def __init__(self, root: Path | None = None, max_bytes: int = 200 * 1024 * 1024):
        self.root = root or data_dir() / "cache" / "apercu"
        self.max_bytes = max_bytes

    @staticmethod
    def _engine_tag(engine: Engine) -> str:
        layout = "dot" if (os.environ.get("GRAPHVIZ_DOT") or shutil.which("dot")) else "smetana"
        return f"{PLANTUML_VERSION}|{engine.kind}|{layout}"

    def _key(self, engine: Engine, text: str, dpi: int | None) -> str:
        material = f"{self._engine_tag(engine)}|{dpi or 'taille'}|".encode("utf-8") + text.encode("utf-8")
        return hashlib.sha256(material).hexdigest()

    def _read(self, path: Path) -> bytes | None:
        try:
            data = path.read_bytes()
            os.utime(path)  # Récemment utilisé : gardé lors du nettoyage.
            return data
        except OSError:
            return None

    def _write(self, path: Path, data: bytes) -> None:
        try:
            self.root.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(dir=self.root, prefix=".", suffix=".tmp", delete=False) as temp:
                temp.write(data)
            os.replace(temp.name, path)
        except OSError:
            return  # Cache facultatif : un disque plein ou protégé n'empêche pas l'aperçu.
        self._prune()

    def _prune(self) -> None:
        try:
            files = [(p.stat().st_mtime, p.stat().st_size, p) for p in self.root.iterdir() if p.is_file()]
        except OSError:
            return
        total = sum(size for _, size, _ in files)
        for _, size, path in sorted(files):
            if total <= self.max_bytes * 3 // 4:
                break
            try:
                path.unlink()
                total -= size
            except OSError:
                pass

    def sizes(self, engine: Engine, text: str) -> list[tuple[int, int]] | None:
        raw = self._read(self.root / f"{self._key(engine, text, None)}.json")
        try:
            return [tuple(pair) for pair in json.loads(raw)] if raw else None
        except (ValueError, TypeError):
            return None

    def put_sizes(self, engine: Engine, text: str, sizes: list[tuple[int, int]]) -> None:
        self._write(self.root / f"{self._key(engine, text, None)}.json", json.dumps(sizes).encode("ascii"))

    def pages(self, engine: Engine, text: str, dpi: int) -> list[bytes] | None:
        key = self._key(engine, text, dpi)
        count = self._read(self.root / f"{key}.pages")
        if not count or not count.isdigit():
            return None
        pages = [self._read(self.root / f"{key}.{index}.png") for index in range(int(count))]
        return pages if all(page and page.startswith(b"\x89PNG") for page in pages) else None

    def put_pages(self, engine: Engine, text: str, dpi: int, pages: list[bytes]) -> None:
        key = self._key(engine, text, dpi)
        for index, page in enumerate(pages):
            self._write(self.root / f"{key}.{index}.png", page)
        self._write(self.root / f"{key}.pages", str(len(pages)).encode("ascii"))  # Écrit en dernier : entrée complète.


_RENDERERS: list[PreviewRenderer] = []


def preview_renderer(engine: Engine) -> PreviewRenderer:
    """Renderer partagé par moteur (processus réutilisés entre les aperçus)."""
    with _LOCK:
        for renderer in _RENDERERS:
            if renderer.engine == engine:
                return renderer
        renderer = PreviewRenderer(engine)
        _RENDERERS.append(renderer)
        return renderer


def cancel_all() -> None:
    """Arrête rendus et processus ouverts (fermeture de l'interface) : aucun processus orphelin."""
    with _LOCK:
        processes = list(_ACTIVE)
        _KILLED.update(processes)
        renderers = list(_RENDERERS)
        _RENDERERS.clear()
    for process in processes:
        try:
            process.kill()
        except OSError:
            pass
    for renderer in renderers:
        renderer.stop()


# Processus ouverts arrêtés aussi à la sortie de Python (tests, scripts), pas seulement à la fermeture de la fenêtre.
atexit.register(cancel_all)
