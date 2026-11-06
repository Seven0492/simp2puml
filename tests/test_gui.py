"""Logique de l'interface graphique ; la fenêtre Tk n'est testée que si un affichage existe."""
import io
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET

import pytest

from simp2puml import gui, launcher, preferences, preview_server, render

PUML = """@startuml
title Essai
class A {
  -valeur: int
}
class B
A -- B
@enduml
"""


def two_views() -> str:
    root = ET.fromstring('''<sim-project version="12.75" multi-file="no"><items/>
    <models><model><abstract-items/><diagram id="view" name="Composants" type="uml-component">
    <abstract-items><item id="a" type="component" name="A"/></abstract-items><layer/>
    </diagram></model></models></sim-project>''')
    second = ET.SubElement(root.find('./models/model'), 'diagram', id='second', name='États', type='uml-statechart')
    ET.SubElement(ET.SubElement(second, 'abstract-items'), 'item', id='s', type='state', name='Prêt')
    return ET.tostring(root, encoding='unicode')


def write(path: Path, text: str) -> Path:
    path.write_text(text, encoding='utf-8')
    return path


def test_run_converts_in_memory_without_writing(tmp_path):
    source = write(tmp_path / 'essai.puml', PUML)
    result = gui.run(gui.Request(source, 'simp'))
    assert result.publishable and '<sim-project' in result.content
    assert any('expérimentale' in w for w in result.warnings)
    assert 'Classes : 2' in result.summary
    assert sorted(p.name for p in tmp_path.iterdir()) == ['essai.puml']


def test_run_selects_one_or_all_simp_diagrams(tmp_path):
    source = write(tmp_path / 'projet.simp', two_views())
    assert gui.run(gui.Request(source, 'puml')).error  # Plusieurs vues sans sélection : refus, comme la CLI.
    one = gui.run(gui.Request(source, 'puml', selector='second'))
    assert one.publishable and one.content.count('@startuml') == 1 and 'Prêt' in one.content
    every = gui.run(gui.Request(source, 'puml', all=True))
    assert every.content.count('@startuml') == 2 and every.summary[0] == 'Diagrammes : 2'
    assert gui.run(gui.Request(source, 'atelier', all=True)).error


def test_strict_keeps_content_visible_but_not_publishable(tmp_path):
    source = write(tmp_path / 'essai.puml', PUML)
    result = gui.run(gui.Request(source, 'simp', strict=True))
    assert result.content and result.warnings and not result.publishable
    assert 'strict' in result.error
    output = tmp_path / 'sortie.simp'
    assert not gui.save(gui.Request(source, 'simp', strict=True), output).saved
    assert not output.exists()


def test_save_publishes_and_never_overwrites(tmp_path):
    source = write(tmp_path / 'essai.puml', PUML)
    request = gui.Request(source, 'atelier')
    output = tmp_path / 'essai.atelier'
    result = gui.save(request, output)
    assert result.saved == output and output.read_text(encoding='utf-8') == result.content
    again = gui.save(request, output)
    assert again.saved is None and 'existe déjà' in again.error
    assert output.read_text(encoding='utf-8') == result.content
    assert 'extension' in gui.save(request, tmp_path / 'essai.txt').error
    assert gui.save(gui.Request(source, 'simp'), source).error


def test_save_reconverts_current_source(tmp_path):
    source = write(tmp_path / 'essai.puml', PUML)
    write(source, PUML.replace('class B', 'class C').replace('A -- B', 'A -- C'))
    output = tmp_path / 'essai.atelier'
    assert gui.save(gui.Request(source, 'atelier'), output).saved
    assert 'C' in output.read_text(encoding='utf-8')


def test_preview_text_is_plantuml_whatever_the_target(tmp_path):
    puml = write(tmp_path / 'essai.puml', PUML)
    assert gui.preview_text(gui.Request(puml, 'atelier')).replace('\r\n', '\n') == PUML  # Source telle quelle.
    simp = write(tmp_path / 'projet.simp', two_views())
    text = gui.preview_text(gui.Request(simp, 'atelier', selector='second', strict=True))
    assert text.startswith('@startuml') and 'Prêt' in text
    assert gui.preview_text(gui.Request(simp, 'puml', all=True)).count('@startuml') == 2
    with pytest.raises(gui.ConversionError):
        gui.preview_text(gui.Request(simp, 'puml'))  # Plusieurs vues sans sélection.


def test_images_are_numbered_and_never_overwrite(tmp_path):
    single = gui.save_images([b'png'], tmp_path / 'vue.png')
    assert single == [tmp_path / 'vue.png'] and single[0].read_bytes() == b'png'
    many = gui.save_images([b'1', b'2'], tmp_path / 'projet.svg')
    assert [p.name for p in many] == ['projet-1.svg', 'projet-2.svg']
    with pytest.raises(gui.ConversionError, match='existe déjà'):
        gui.save_images([b'x', b'y'], tmp_path / 'projet.svg')
    assert many[1].read_bytes() == b'2'


def test_layout_directives_are_added_after_each_block_and_removed(tmp_path):
    two = PUML + PUML
    vertical = gui.set_directive(two, 'left to right direction', True, ('top to bottom direction',))
    assert vertical.count('@startuml\nleft to right direction\n') == 2
    assert gui.has_directive(vertical, 'left to right direction')
    assert gui.set_directive(vertical, 'left to right direction', False) == two
    # Une directive incompatible est remplacée, pas cumulée.
    tb = gui.set_directive(PUML, 'top to bottom direction', True)
    lr = gui.set_directive(tb, 'left to right direction', True, ('top to bottom direction',))
    assert 'top to bottom' not in lr and lr.count('left to right direction') == 1


def test_simp_layout_hints_can_be_removed_and_portrait_preset_applied():
    text = ('@startuml\nclass "A" as E1\nclass "B" as E2\nnote right of E1 : voir -up- et .left.\n'
            'E1 "1" *-down- "40" E2 : contient\nE2 .left.> E1\nE1 -[hidden]down- E2\n@enduml\n')
    assert gui.has_layout_hints(text)
    free = gui.strip_layout_hints(text)
    assert not gui.has_layout_hints(free)
    assert 'E1 "1" *-- "40" E2 : contient' in free and 'E2 ..> E1' in free and 'hidden' not in free
    assert 'note right of E1 : voir -up- et .left.' in free  # Le texte des notes n'est pas touché.
    portrait = gui.portrait_layout(text)
    assert portrait.startswith('@startuml\nskinparam wrapWidth 200\nleft to right direction\n')
    assert not gui.has_layout_hints(portrait)


def test_edited_text_is_saved_as_is_and_never_overwrites(tmp_path):
    source = write(tmp_path / 'essai.puml', PUML)
    request = gui.Request(source, 'atelier', strict=True)
    output = tmp_path / 'modifie.atelier'
    assert gui.save_text(request, output, 'dsl 2\n// à la main\n') == output
    assert output.read_text(encoding='utf-8') == 'dsl 2\n// à la main\n'
    with pytest.raises(gui.ConversionError, match='existe déjà'):
        gui.save_text(request, output, 'autre')
    with pytest.raises(gui.ConversionError, match='extension'):
        gui.save_text(request, tmp_path / 'x.txt', 'x')
    with pytest.raises(gui.ConversionError, match='source'):
        gui.save_text(gui.Request(source, 'puml'), source, 'x')


def test_editor_edits_drive_preview_save_and_revert(tmp_path, monkeypatch):
    tk = pytest.importorskip('tkinter')
    try:
        root = tk.Tk()
    except tk.TclError as exc:
        pytest.skip(f'affichage indisponible : {exc}')
    root.withdraw()
    try:
        source = write(tmp_path / 'projet.simp', two_views())
        app = gui.App(root, source)
        while app._busy:
            root.update()
            app._poll()
        generated = app.editor_text()
        assert generated.startswith('@startuml') and not app._dirty and app._edited_plantuml() is None
        app.output_text.insert('end', "' note ajoutée\n")
        root.update()
        app._after_edit()
        assert app._dirty and app._edited_plantuml().endswith("' note ajoutée\n")
        assert app.output_text.tag_ranges('comment')  # Coloration à jour après la frappe.
        assert app.save_button.instate(['!disabled'])
        app.layout_vars['vertical'].set(True)
        app.toggle_layout('vertical')
        assert 'left to right direction' in app.editor_text()
        app.output_text.edit_undo()  # Une seule étape pour annuler la directive.
        assert 'left to right direction' not in app.editor_text() and "note ajoutée" in app.editor_text()
        # Refuser l'abandon des modifications : l'option revient en arrière, le texte est conservé.
        monkeypatch.setattr(gui.messagebox, 'askyesno', lambda *a, **k: False)
        app.target_var.set('atelier')
        app._options_changed()
        assert app.target_var.get() == 'puml' and "note ajoutée" in app.editor_text()
        app.revert_edits()
        assert app.editor_text() == generated and not app._dirty
        app._mark_error_line(2)
        assert app.output_text.tag_ranges('error_line')
    finally:
        root.destroy()


def test_read_errors_are_reported(tmp_path):
    assert gui.run(gui.Request(tmp_path / 'absent.simp', 'puml')).error
    assert gui.run(gui.Request(write(tmp_path / 'x.txt', ''), 'puml')).error
    assert gui.run(gui.Request(write(tmp_path / 'x.simp', two_views()), 'simp')).error


def test_window_loads_converts_and_restricts_targets(tmp_path):
    tk = pytest.importorskip('tkinter')
    try:
        root = tk.Tk()
    except tk.TclError as exc:
        pytest.skip(f'affichage indisponible : {exc}')
    root.withdraw()
    try:
        source = write(tmp_path / 'projet.simp', two_views())
        app = gui.App(root, source)

        def settle():
            while app._busy:
                root.update()
                app._poll()

        settle()
        assert app._images, 'icônes et portrait embarqués introuvables'
        assert app.target_var.get() == 'puml' and app.result.publishable
        assert app.target_buttons['simp'].instate(['disabled'])  # Même format que la source.
        assert app.target_buttons['atelier'].instate(['!disabled'])
        app.diagram_box.current(len(app.diagrams))  # Tous les diagrammes.
        app.target_var.set('atelier')
        app._options_changed()
        settle()
        assert app.target_var.get() == 'puml'
        assert app.target_buttons['atelier'].instate(['disabled'])
        assert app.result.content.count('@startuml') == 2
    finally:
        root.destroy()


def test_main_without_window_options(capsys):
    assert launcher.main(['--version']) == 0
    assert 'simp2puml-gui' in capsys.readouterr().out


FIXTURE = Path(__file__).parent / 'data' / 'preferences.conf'


@pytest.fixture
def config(tmp_path, monkeypatch):
    monkeypatch.setenv('XDG_CONFIG_HOME', str(tmp_path / 'config'))
    return tmp_path / 'config' / 'simp2puml' / 'preferences.conf'


def test_new_interface_is_the_default_and_invalid_values_are_ignored(config):
    assert preferences.interface() == preferences.EDITOR
    config.parent.mkdir(parents=True)
    config.write_text('interface = violette\n', encoding='utf-8')
    assert preferences.interface() == preferences.EDITOR


def test_shared_fixture_and_other_settings_survive_a_change(config):
    text = FIXTURE.read_text(encoding='utf-8')
    assert preferences.parse_interface(text) == preferences.CLASSIC  # Même fichier que les tests Rust.
    config.parent.mkdir(parents=True)
    config.write_text(text, encoding='utf-8')
    preferences.set_interface(preferences.EDITOR)
    saved = config.read_text(encoding='utf-8')
    assert preferences.parse_interface(saved) == preferences.EDITOR
    assert saved.count('interface =') == 1
    assert 'theme = dark' in saved and 'code_size = 18' in saved and saved.startswith('# Lu par')
    assert [p.name for p in config.parent.iterdir()] == ['preferences.conf'], 'aucun fichier temporaire'


def test_choice_is_saved_through_a_symlink(config, tmp_path):
    real = tmp_path / 'reel.conf'
    real.write_text('theme = dark\n', encoding='utf-8')
    config.parent.mkdir(parents=True)
    config.symlink_to(real)
    preferences.set_interface(preferences.CLASSIC)
    assert config.is_symlink()
    assert 'interface = classic' in real.read_text(encoding='utf-8')


def test_launcher_opens_the_new_interface_by_default(config, monkeypatch):
    seen = []
    monkeypatch.setattr(launcher.interfaces, 'find_editor', lambda: Path('/x/puml-editor'))
    monkeypatch.setattr(launcher.interfaces, 'replace_process', lambda exe, files: seen.append((exe, files)) or True)
    monkeypatch.setattr(gui, 'main', lambda _args: pytest.fail('interface classique ouverte'))
    assert launcher.main(['a.puml', 'b.puml']) == 0
    assert seen == [(Path('/x/puml-editor'), ['a.puml', 'b.puml'])]


def test_launcher_uses_the_classic_interface_when_chosen_or_when_the_editor_is_missing(config, monkeypatch):
    opened, classic = [], []
    monkeypatch.setattr(gui, 'main', lambda args: classic.append(args) or 7)
    monkeypatch.setattr(launcher.interfaces, 'replace_process', lambda *_: opened.append('editeur') or True)

    monkeypatch.setattr(launcher.interfaces, 'find_editor', lambda: None)
    assert launcher.main(['a.puml']) == 7, 'éditeur absent : repli sur la classique'
    monkeypatch.setattr(launcher.interfaces, 'find_editor', lambda: Path('/x/puml-editor'))
    assert launcher.main(['--interface', 'classic', 'b.puml']) == 7, 'demande explicite'
    preferences.set_interface(preferences.CLASSIC)
    assert launcher.main([]) == 7, 'préférence'
    assert opened == [] and classic == [['a.puml'], ['b.puml'], []]
    assert launcher.main(['--interface', 'editor']) == 0, 'demande explicite plus forte que la préférence'
    assert opened == ['editeur']
    assert launcher.main(['--interface', 'violette']) == 2


def test_failed_editor_start_falls_back_to_classic(config, monkeypatch):
    monkeypatch.setattr(gui, 'main', lambda _args: 7)
    monkeypatch.setattr(launcher.interfaces, 'find_editor', lambda: Path('/x/puml-editor'))
    monkeypatch.setattr(launcher.interfaces, 'replace_process', lambda *_: False)
    assert launcher.main([]) == 7


def test_starting_the_new_interface_does_not_load_the_classic_one():
    # Le lancement par défaut ne doit payer ni Tkinter ni le convertisseur.
    code = ('import sys; from simp2puml import launcher; '
            'sys.exit(any(m in sys.modules for m in ("tkinter", "simp2puml.gui", "simp2puml.cli")))')
    assert subprocess.run([sys.executable, '-c', code], check=False).returncode == 0


def test_find_editor_looks_beside_the_link_without_path(tmp_path, monkeypatch):
    # Installation typique : ~/.local/bin/simp2puml-gui est un lien, puml-editor est à côté de lui.
    bin_dir, tool = tmp_path / 'bin', tmp_path / 'outil'
    bin_dir.mkdir()
    tool.mkdir()
    (tool / 'simp2puml-gui').write_text('', encoding='utf-8')
    (bin_dir / 'simp2puml-gui').symlink_to(tool / 'simp2puml-gui')
    editor = bin_dir / 'puml-editor'
    editor.write_text('', encoding='utf-8')
    editor.chmod(0o755)
    monkeypatch.delenv('PUML_EDITOR', raising=False)
    monkeypatch.setenv('PATH', '/nulle/part')
    monkeypatch.setattr(sys, 'argv', [str(bin_dir / 'simp2puml-gui')])
    assert gui.interfaces.find_editor() == editor


def test_find_editor_honours_the_override(tmp_path, monkeypatch):
    monkeypatch.setenv('PUML_EDITOR', str(tmp_path / 'absent'))
    assert gui.interfaces.find_editor() is None, 'une demande explicite invalide ne prend pas un autre binaire'
    exe = tmp_path / 'puml-editor'
    exe.write_text('', encoding='utf-8')
    monkeypatch.setenv('PUML_EDITOR', str(exe))
    assert gui.interfaces.find_editor() == exe


def test_start_editor_reports_an_immediate_failure(tmp_path):
    script = tmp_path / 'casse'
    script.write_text('#!/bin/sh\necho "panne" >&2\nexit 3\n', encoding='utf-8')
    script.chmod(0o755)
    assert gui.interfaces.start_editor(script, []) == 'panne'
    assert gui.interfaces.start_editor(tmp_path / 'absent', []).startswith('impossible de lancer')


def test_plantuml_source_is_shown_as_is(tmp_path):
    source = write(tmp_path / 'a.puml', '@startuml\npackage "P" {\nclass A\n}\n@enduml\n')
    assert gui.default_target(source) == 'puml'
    result = gui.run(gui.Request(source, 'puml'))
    assert result.publishable and 'package "P"' in result.content
    assert gui.run(gui.Request(write(tmp_path / 'b.simp', two_views()), 'simp')).error is not None
    output = tmp_path / 'copie.puml'
    assert gui.save(gui.Request(source, 'puml'), output).saved == output
    assert gui.save(gui.Request(source, 'puml'), source).error  # Jamais la source elle-même.


def test_classic_preferences_window_switches_to_the_new_interface(tmp_path, config, monkeypatch):
    tk = pytest.importorskip('tkinter')
    try:
        root = tk.Tk()
    except tk.TclError as exc:
        pytest.skip(f'affichage indisponible : {exc}')
    root.withdraw()
    closed = False
    try:
        source = write(tmp_path / 'a.puml', '@startuml\nclass A\n@enduml\n')
        app = gui.App(root, source)
        while app._busy:
            root.update()
            app._poll()
        assert 'class A' in app.editor_text()

        # Éditeur absent : la nouvelle interface est grisée, rien n'est enregistré.
        monkeypatch.setattr(gui.interfaces, 'find_editor', lambda: None)
        app.open_preferences()
        app.preferences_window.withdraw()
        assert not app.switch_interface(preferences.EDITOR)
        assert not config.exists()
        app.preferences_window.destroy()

        # Échec du démarrage : le choix ne change pas et la fenêtre reste.
        monkeypatch.setattr(gui.interfaces, 'find_editor', lambda: Path('/x/puml-editor'))
        monkeypatch.setattr(gui.interfaces, 'start_editor', lambda *_: 'panne')
        app.open_preferences()
        app.preferences_window.withdraw()
        assert not app.switch_interface(preferences.EDITOR)
        assert 'panne' in app.status_var.get() or not config.exists()
        assert not config.exists()

        started = []
        monkeypatch.setattr(gui.interfaces, 'start_editor', lambda exe, files: started.append(files))
        assert app.switch_interface(preferences.EDITOR)
        closed = True
        assert started == [[str(source)]], 'la nouvelle interface reçoit le fichier ouvert'
        assert preferences.interface() == preferences.EDITOR and config.exists()
    finally:
        if not closed:
            root.destroy()


SVG_PAGE = (b'<svg xmlns="http://www.w3.org/2000/svg" width="120px" height="80px" viewBox="0 0 120 80">'
            b'<g class="entity" data-source-line="1"><rect x="10" y="20" width="30" height="40"/></g></svg>')


class FakeRenderer:
    def __init__(self):
        self.seen = []

    def svg_pages(self, text):
        self.seen.append(text)
        if 'mauvais' in text:
            return render.Rendering([], 'Erreur PlantUML à la ligne 3 : Syntax Error?')
        return render.Rendering([SVG_PAGE, SVG_PAGE])

    def stop(self):
        pass


def serve(requests, monkeypatch, engine=object()):
    out = io.BytesIO()
    fake = FakeRenderer()
    monkeypatch.setattr(render, 'find_engine', lambda: engine)
    monkeypatch.setattr(render, 'preview_renderer', lambda _engine: fake)
    data = b''.join(b'RENDER %s %d %d\n%s' % (rid, block, len(text), text) for rid, block, text in requests)
    assert preview_server.serve(io.BytesIO(data), out) == 0
    return out.getvalue(), fake


def test_preview_server_speaks_the_renderer_protocol(monkeypatch):
    text = b'@startuml\nclass A\n@enduml\n'
    output, fake = serve([(b'7', 1, text), (b'8', 9, text)], monkeypatch)
    assert fake.seen == [text.decode(), text.decode()]
    assert output.startswith(b'PUMLRENDER 1\nID\t7\nKIND\tplantuml officiel\nBLOCKS\t2\nSIZE\t120\t80\n')
    assert b'ID\t8\n' in output, 'un bloc disparu ramène au dernier'
    # La ligne notée par PlantUML (depuis @start, à partir de 0) devient une ligne du document.
    assert b'REGION\t2\t10\t20\t30\t40\n' in output
    assert output.count(b'SVG\t%d\n' % len(SVG_PAGE)) == 2 and output.endswith(SVG_PAGE)


def test_preview_server_reports_syntax_errors_and_missing_engine(monkeypatch):
    output, _ = serve([(b'1', 0, b'mauvais')], monkeypatch)
    assert b'DIAG\t3\terror\tErreur PlantUML' in output and b'SVG\t' in output
    output, _ = serve([(b'2', 0, b'x')], monkeypatch, engine=None)
    assert output == b'PUMLRENDER 1\nID\t2\nERROR\t' + preview_server.NOT_INSTALLED.encode() + b'\n'


def test_preview_server_survives_an_invalid_request(monkeypatch):
    out = io.BytesIO()
    monkeypatch.setattr(render, 'find_engine', lambda: None)
    assert preview_server.serve(io.BytesIO(b'BONJOUR\nRENDER 3 x 1\n'), out) == 0
    assert out.getvalue().count('requête invalide'.encode()) == 2


def test_preferences_of_the_new_interface_survive_a_classic_switch(config):
    config.parent.mkdir(parents=True)
    config.write_text('preview_engine = official\nsecurity_profile = folder\n', encoding='utf-8')
    preferences.set_interface(preferences.CLASSIC)
    saved = config.read_text(encoding='utf-8')
    assert 'preview_engine = official' in saved and 'security_profile = folder' in saved


def test_official_regions_reach_members_and_elements_without_line_numbers():
    source = '@startuml\nclass A {\n  +x : int\n  +f()\n}\nA : +z : long\n@enduml\n'
    svg = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">'
           '<g class="entity" data-source-line="1"><rect x="10" y="10" width="80" height="60"/>'
           '<text x="20" y="20" font-size="10">A</text>'
           '<g data-visibility-modifier="PUBLIC_FIELD"><text x="30" y="40" font-size="10">x\u00a0: int</text></g>'
           '<text x="30" y="50" font-size="10">f()</text><text x="30" y="60" font-size="10">z : long</text></g>'
           '<g class="link" data-source-line="1"><path d="M10,70 L10,90"/></g></svg>').encode()
    found, links = preview_server.regions_and_links(svg, 1, source)
    lines = {line for line, *_ in found}
    assert {2, 3, 4, 6} <= lines
    member = next(r for r in found if r[0] == 3)
    assert member[1:] == (10.0, 30.0, 80.0, 13.0), 'toute la largeur de l\'élément'
    assert [(line, points, group) for line, _, points, group in links] == [(2, [(10.0, 70.0), (10.0, 90.0)], 0)], 'la flèche : son tracé et son groupe'
    # Sans aucune ligne notée (activité, Gantt…) : les textes dans l'ordre du source, même répétés.
    plain = ('<svg xmlns="http://www.w3.org/2000/svg"><text x="1" y="9" font-size="9" textLength="9">rien</text>'
             '<text x="1" y="9" font-size="9">Lire</text><text x="1" y="9" font-size="9">Fin</text></svg>').encode()
    assert [r[0] for r in preview_server.regions_and_links(plain, 1, '@startuml\n:Lire;\n:Fin;\n@enduml\n')[0]] == [2, 3]
    assert preview_server.regions_and_links(b'pas du xml', 1, '') == ([], [])


def test_official_arrow_curves_are_flattened():
    points = preview_server._flatten('M0,0 L10,0 C10,10 20,10 20,0 H30 V5')
    assert points[:2] == [(0.0, 0.0), (10.0, 0.0)]
    assert points[-2:] == [(30.0, 0.0), (30.0, 5.0)]
    assert len(points) == 2 + 12 + 2, 'une courbe : douze segments'
    assert max(y for _, y in points[2:14]) > 5, 'la courbe passe par son milieu, pas par une corde'
    assert preview_server._flatten('Z') == []
