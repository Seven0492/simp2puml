"""Moteur d'aperçu : logique sans réseau ; rendu réel seulement avec PLANTUML_JAR et Java."""
import hashlib
from pathlib import Path
import zipfile

import pytest

from simp2puml import render

DIAGRAM = "@startuml\nclass A\nA --> B\n@enduml\n"


@pytest.mark.parametrize("output, expected", [
    ('openjdk version "11.0.27" 2025-04-15 LTS', 11),
    ('java version "1.8.0_402"', 8),
    ('openjdk version "21" 2023-09-19', 21),
    ("pas de version", None),
])
def test_java_version(output, expected):
    assert render.java_version(output) == expected


def test_command_is_sandboxed_and_uses_smetana_without_graphviz(monkeypatch, tmp_path):
    monkeypatch.delenv("GRAPHVIZ_DOT", raising=False)
    monkeypatch.setattr(render.shutil, "which", lambda name: None)
    engine = render.Engine.java(Path("java"), Path("plantuml.jar"))
    args = render.command(engine, "png2x", tmp_path / "d.puml", tmp_path, dpi=192)
    assert "-DPLANTUML_SECURITY_PROFILE=SANDBOX" in args and "-tpng" in args
    assert "-Playout=smetana" in args and "-Sdpi=192" in args and "-nometadata" in args
    assert "-Sdpi=96" not in render.command(engine, "svg", tmp_path / "d.puml", tmp_path, dpi=96)


def test_download_checks_sha256_before_publishing(tmp_path):
    source = tmp_path / "source.bin"
    source.write_bytes(b"contenu")
    good = hashlib.sha256(b"contenu").hexdigest()
    destination = tmp_path / "out" / "fichier.jar"
    with pytest.raises(render.RenderError, match="somme"):
        render._download(source.as_uri(), destination, "0" * 64, "test", None)
    assert not destination.exists() and not list(destination.parent.glob("*.part"))
    seen = []
    render._download(source.as_uri(), destination, good, "test", lambda text, fraction: seen.append(fraction))
    assert destination.read_bytes() == b"contenu" and seen


def test_archive_with_parent_path_is_refused(tmp_path):
    archive = tmp_path / "java.zip"
    with zipfile.ZipFile(archive, "w") as bundle:
        bundle.writestr("../evasion.txt", "x")
    with pytest.raises(render.RenderError, match="suspect"):
        render._extract(archive, tmp_path / "cible")
    assert not (tmp_path / "evasion.txt").exists()


def test_managed_locations_follow_data_dir(monkeypatch, tmp_path):
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
    monkeypatch.delenv("PLANTUML_JAR", raising=False)
    assert render.find_jar() is None
    jar = render.managed_jar()
    jar.parent.mkdir(parents=True)
    jar.write_bytes(b"")
    assert render.find_jar() == jar


def test_message_hides_temporary_path():
    raw = "Error line 3 in file: C:\\Temp\\x\\diagramme.puml\nSome diagram description contains errors\n"
    assert render._clean_message(raw) == "Erreur de syntaxe PlantUML à la ligne 3 (détail dans l'image)."


def test_pipe_helpers_count_pages_sizes_and_shift_error_lines():
    text = "@startuml\nclass A\n@enduml\n  @startuml\nclass B {\n@enduml\n"
    assert render.count_diagrams(text) == 2 and render.count_diagrams("class A") == 0
    injected, inserted = render.inject_dpi(text, 144)
    assert injected.count("skinparam dpi 144") == 2 and inserted == [2, 6]
    assert render.inject_dpi(text, 96) == (text, [])
    # PlantUML (-pipe, base 0) indique 6 : ligne 7 du texte modifié, donc ligne 5 du texte d'origine.
    assert render._pipe_error("6\nSyntax Error?", inserted) == "Erreur PlantUML à la ligne 5 : Syntax Error?"
    svg = b'<svg xmlns="http://www.w3.org/2000/svg" width="10793px" height="1736px" viewBox="0 0 10793 1736">'
    assert render.svg_size(svg) == (10793, 1736) and render.svg_size(b"<svg>") is None


def test_native_build_is_pinned_for_supported_platforms(monkeypatch):
    for key, (name, sha256, size) in render.NATIVE_BUILDS.items():
        assert render.PLANTUML_VERSION in name and len(sha256) == 64 and size > 0
    monkeypatch.setattr(render, "_platform", lambda: ("mac", "x64"))  # Pas d'exécutable natif publié.
    assert render.native_build() is None


def test_installed_native_engine_needs_nothing_else(monkeypatch, tmp_path):
    executable = tmp_path / "plantuml.exe"
    executable.write_bytes(b"")
    monkeypatch.setattr(render, "find_native", lambda: executable)
    monkeypatch.setattr(render, "find_jar", lambda: None)
    monkeypatch.setattr(render, "find_java", lambda: None)
    monkeypatch.delenv("PLANTUML_JAR", raising=False)
    assert render.missing_parts() == []
    assert render.find_engine() == render.Engine.native(executable)


PNG = b"\x89PNG\r\n\x1a\nfaux"


def test_native_check_runs_once_until_the_executable_changes(monkeypatch, tmp_path):
    executable = tmp_path / "plantuml.exe"
    executable.write_bytes(b"binaire")
    calls = []
    monkeypatch.setattr(render, "_runs", lambda argv, check: calls.append(argv) or True)
    monkeypatch.setattr(render, "_VERIFIED", {})
    assert render._usable_native(executable) and len(calls) == 1
    monkeypatch.setattr(render, "_VERIFIED", {})  # Nouvelle session de l'interface.
    assert render._usable_native(executable) and len(calls) == 1  # Marqueur : pas de nouveau -version.
    executable.write_bytes(b"autre binaire")  # Exécutable remplacé : revérifié.
    monkeypatch.setattr(render, "_VERIFIED", {})
    assert render._usable_native(executable) and len(calls) == 2


def test_preview_cache_keys_on_text_resolution_and_engine(tmp_path):
    cache = render.PreviewCache(tmp_path)
    native = render.Engine.native(Path("plantuml.exe"))
    java = render.Engine.java(Path("java"), Path("plantuml.jar"))
    assert cache.pages(native, DIAGRAM, 96) is None and cache.sizes(native, DIAGRAM) is None
    cache.put_sizes(native, DIAGRAM, [(120, 80)])
    cache.put_pages(native, DIAGRAM, 96, [PNG, PNG + b"2"])
    assert cache.sizes(native, DIAGRAM) == [(120, 80)]
    assert cache.pages(native, DIAGRAM, 96) == [PNG, PNG + b"2"]
    # Autre résolution, autre texte ou autre moteur : jamais l'image d'un autre rendu.
    assert cache.pages(native, DIAGRAM, 144) is None
    assert cache.pages(native, DIAGRAM + "' modifié\n", 96) is None
    assert cache.pages(java, DIAGRAM, 96) is None
    # Une entrée abîmée est ignorée, pas affichée.
    next(tmp_path.glob("*.1.png")).write_bytes(b"pas une image")
    assert cache.pages(native, DIAGRAM, 96) is None


def test_preview_cache_drops_least_recently_used_beyond_its_limit(tmp_path):
    cache = render.PreviewCache(tmp_path, max_bytes=3000)
    engine = render.Engine.native(Path("plantuml.exe"))
    big = PNG + b"x" * 1000
    cache.put_pages(engine, "@startuml\nclass Ancien\n@enduml\n", 96, [big])
    import os, time
    old = time.time() - 3600
    for path in tmp_path.iterdir():
        os.utime(path, (old, old))
    cache.put_pages(engine, "@startuml\nclass Recent\n@enduml\n", 96, [big])
    cache.put_pages(engine, "@startuml\nclass Dernier\n@enduml\n", 96, [big])
    assert cache.pages(engine, "@startuml\nclass Dernier\n@enduml\n", 96) == [big]
    assert cache.pages(engine, "@startuml\nclass Ancien\n@enduml\n", 96) is None
    assert sum(p.stat().st_size for p in tmp_path.iterdir()) <= 3000


@pytest.fixture
def engine():
    found = render.find_engine()
    if found is None:
        pytest.skip("Java et PLANTUML_JAR requis pour le rendu réel")
    return found


def test_real_rendering_formats_pages_and_errors(engine):
    two = render.render(DIAGRAM + "@startuml\nstate X\n@enduml\n", "png", engine)
    assert len(two.pages) == 2 and two.error is None
    assert all(page.startswith(b"\x89PNG") for page in two.pages)
    assert render.render(DIAGRAM, "svg", engine).pages[0].lstrip().startswith(b"<")
    assert b"A" in render.render(DIAGRAM, "txt", engine).pages[0]
    broken = render.render("@startuml\nclass A {\n@enduml\n", "png", engine)
    assert broken.pages and "ligne 2" in broken.error


def test_preview_renderer_keeps_processes_open_and_reports_errors(engine, tmp_path):
    renderer = render.PreviewRenderer(engine)
    try:
        sizes, error = renderer.sizes(DIAGRAM + DIAGRAM)
        assert len(sizes) == 2 and all(w > 0 and h > 0 for w, h in sizes) and error is None
        first = renderer.png_pages(DIAGRAM, 96)
        process = renderer.png.process
        sharp = renderer.png_pages(DIAGRAM, 192)  # Même processus, résolution doublée.
        assert renderer.png.process is process and first.pages[0].startswith(b"\x89PNG")
        assert len(sharp.pages[0]) > len(first.pages[0])
        broken_text = "@startuml\nclass A\nclass B\nA --> --> B ??\nclass C\n@enduml\n"
        for dpi in (96, 144):  # Avec ou sans ligne `skinparam dpi` injectée : le même numéro (4).
            broken = renderer.png_pages(broken_text, dpi)
            assert not broken.pages and "à la ligne 4 " in broken.error, broken.error
        assert "à la ligne 4 " in renderer.sizes(broken_text)[1]
        secret = tmp_path / "secret.puml"
        secret.write_text("class Confidentiel\n", encoding="utf-8")
        blocked = renderer.svg_pages(f"@startuml\n!include {secret.as_posix()}\n@enduml\n")
        assert not blocked.pages and "include" in blocked.error
        assert renderer.png_pages(DIAGRAM, 96).pages  # Toujours utilisable après une erreur.
    finally:
        renderer.stop()


def test_sandbox_blocks_local_files_but_keeps_stdlib(engine, tmp_path):
    # Le profil SECURE laisserait passer ce !include d'un .puml local : SANDBOX le refuse.
    secret = tmp_path / "secret.puml"
    secret.write_text("class Confidentiel\n", encoding="utf-8")
    result = render.render(f"@startuml\n!include {secret.as_posix()}\nclass Visible\n@enduml\n", "txt", engine)
    assert result.error and b"Confidentiel" not in result.pages[0]
    c4 = render.render('@startuml\n!include <C4/C4_Container>\nPerson(u, "Utilisateur")\n@enduml\n', "png", engine)
    assert c4.error is None
