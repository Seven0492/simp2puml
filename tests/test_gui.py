"""Logique de l'interface graphique ; la fenêtre Tk n'est testée que si un affichage existe."""
from pathlib import Path
import xml.etree.ElementTree as ET

import pytest

from simp2puml import gui

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
    result = gui.run(gui.Request(source, gui.default_target(source)))
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
    assert gui.run(gui.Request(write(tmp_path / 'x.puml', PUML), 'puml')).error


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
    assert gui.main(['--version']) == 0
    assert 'simp2puml-gui' in capsys.readouterr().out
