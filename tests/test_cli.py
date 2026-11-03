from dataclasses import asdict
import os
from pathlib import Path
from types import SimpleNamespace
import xml.etree.ElementTree as ET

import pytest

from simp2puml.cli import main
from simp2puml import cli
from simp2puml.model import UMLClass, UMLDiagram
from simp2puml.puml import parse_puml
from simp2puml.simp import parse_simp, write_simp

PUML = """@startuml
title Essai
class A {
  -valeur: Map<String, Integer>
  executer(premier: B, second): Resultat
}
class B
A -- B
B --|> A
@enduml
"""


def _symlink(link, target):
    """Crée un lien symbolique, ou ignore le test si le système le refuse (Windows sans privilège)."""
    try:
        link.symlink_to(target)
    except (OSError, NotImplementedError) as exc:
        pytest.skip(f'liens symboliques indisponibles : {exc}')


def normalized_model(diagram):
    by_id = {cls.id: cls for cls in diagram.classes}
    keys = {}
    for cls in diagram.classes:
        providers = tuple(sorted((by_id[r.source].kind, by_id[r.source].name)
                                 for r in diagram.relationships
                                 if r.kind == 'provided_interface' and r.target == cls.id))
        keys[cls.id] = (cls.kind, cls.name, providers)
    classes = []
    for cls in diagram.classes:
        data = asdict(cls)
        data.pop('id')
        classes.append((keys[cls.id], data))
    relationships = []
    for relation in diagram.relationships:
        data = asdict(relation)
        data['source'], data['target'] = keys[relation.source], keys[relation.target]
        relationships.append(data)
    notes = [(note.text, sorted(keys[target] for target in note.targets)) for note in diagram.notes]
    return (diagram.name, sorted(classes, key=repr), sorted(relationships, key=repr), sorted(notes, key=repr))


def test_converts_both_directions(tmp_path, capsys):
    puml = tmp_path / "source.PuMl"
    puml.write_text(PUML, encoding="utf-8")
    assert main([str(puml)]) == 0
    simp = tmp_path / "source.simp"
    assert simp.exists()
    assert "Conversion terminée\nClasses : 2\nRelations : 2\nAvertissements : 1\n" in capsys.readouterr().out

    target = tmp_path / "result.PuMl"
    assert main([str(simp), "-o", str(target)]) == 0
    assert "class \"A\"" in target.read_text(encoding="utf-8")
    assert "Avertissements : 0" in capsys.readouterr().out

    assert normalized_model(parse_puml(target.read_text(encoding="utf-8"))) == normalized_model(parse_puml(PUML))


@pytest.mark.skipif(
    not (Path(__file__).resolve().parents[1] / 'Monopoly.simp').exists(),
    reason='Fixture réelle Monopoly.simp non redistribuée',
)
def test_converts_real_monopoly_without_modifying_source(tmp_path, capsys):
    source = Path(__file__).resolve().parents[1] / "Monopoly.simp"
    original = source.read_bytes()
    output = tmp_path / "Monopoly.puml"

    assert main([str(source), "-o", str(output)]) == 0
    report = capsys.readouterr()
    assert "Classes : 19\nRelations : 26\n" in report.out
    assert "Interfaces : 5" in report.out and "Énumérations : 2" in report.out
    assert "Symboles d’interface : 6" in report.out and "Notes : 17" in report.out
    assert "4db56d1e23f14bb398f93b5f1918504e" in report.err
    assert "orpheline" in report.err
    assert source.read_bytes() == original

    diagram = parse_puml(output.read_text(encoding="utf-8"))
    assert len(diagram.classes) == 32 and len(diagram.relationships) == 26
    assert len(diagram.notes) == 17
    by_name = {cls.name: cls for cls in diagram.classes if cls.kind != 'interface_symbol'}
    expected = parse_simp(original.decode('utf-8-sig'))
    assert normalized_model(diagram) == normalized_model(expected)
    restored_path = tmp_path / 'retour.simp'
    assert main([str(output), '-o', str(restored_path)]) == 0
    restored = parse_simp(restored_path.read_text(encoding='utf-8'))
    assert normalized_model(restored) == normalized_model(expected)
    assert source.read_bytes() == original
    assert by_name['Case'].is_abstract
    assert by_name['Couleur'].literals == ['Brun', 'BleuPale', 'Rose', 'Orange', 'Rouge', 'Jaune', 'Vert', 'BleuFonce']
    action = next(op for op in by_name["Case"].operations if op.name == "Action")
    assert [(p.name, p.type) for p in action.parameters] == [("j", "Joueur")]
    assert any(r.source == by_name["CheminDeFer"].id
               and r.target == by_name["Propriete"].id and r.kind == "inheritance"
               for r in diagram.relationships)


def test_refuses_existing_output_and_source_as_output(tmp_path, capsys):
    source = tmp_path / "input.puml"
    source.write_text(PUML, encoding="utf-8")
    output = tmp_path / "input.simp"
    output.write_text("keep", encoding="utf-8")
    assert main([str(source)]) == 1
    assert output.read_text(encoding="utf-8") == "keep"
    assert "FileExistsError" not in capsys.readouterr().err

    with pytest.raises(SystemExit) as exc:
        main([str(source), "-o", str(source)])
    assert exc.value.code == 2

    alias = tmp_path / "alias.simp"
    _symlink(alias, source)
    assert main([str(source), "-o", str(alias)]) == 1
    assert source.read_text(encoding="utf-8") == PUML
    assert "fichier source" in capsys.readouterr().err


def test_parse_error_does_not_create_output(tmp_path, capsys):
    source = tmp_path / "broken.puml"
    source.write_text("pas du PlantUML", encoding="utf-8")
    output = tmp_path / "out.simp"
    assert main([str(source), "-o", str(output)]) == 1
    assert not output.exists()
    assert "Erreur :" in capsys.readouterr().err


def test_extension_validation_and_multi_diagram_selection(tmp_path, capsys):
    source = tmp_path / "multi.simp"
    source.write_text('''<sim-project version="12.75" multi-file="no"><items/><models><model id="m"><abstract-items/><diagram id="d1" name="Premier" type="uml-class"><abstract-items/></diagram><diagram id="d2" name="Second" type="uml-class"><abstract-items/></diagram></model></models></sim-project>''', encoding="utf-8")
    assert main([str(source)]) == 1
    error = capsys.readouterr().err
    assert "Premier" in error and "Second" in error
    assert main([str(source), "--diagram", "Second"]) == 0
    assert (tmp_path / "multi.puml").exists()
    capsys.readouterr()

    bad_output = tmp_path / "wrong.txt"
    with pytest.raises(SystemExit) as exc:
        main([str(source), "--diagram", "Second", "-o", str(bad_output)])
    assert exc.value.code == 2
    assert not bad_output.exists()


def test_check_validates_conversion_without_touching_output(tmp_path, capsys):
    source = tmp_path / 'source.puml'
    source.write_text(PUML, encoding='utf-8')
    output = source.with_suffix('.simp')
    assert main([str(source), '--check']) == 0
    assert not output.exists()
    report = capsys.readouterr()
    assert 'Vérification terminée' in report.out
    assert 'Sortie :' not in report.out
    assert 'expérimentale' in report.err

    output.write_text('conserver', encoding='utf-8')
    assert main([str(source), '--check']) == 0
    assert output.read_text(encoding='utf-8') == 'conserver'


def test_check_rejects_unrepresentable_destination(tmp_path, capsys):
    source = tmp_path / 'source.puml'
    source.write_text('@startuml\nA "1" --|> B\n@enduml', encoding='utf-8')
    assert main([str(source), '--check']) == 1
    assert 'Détails de relation non représentables' in capsys.readouterr().err
    assert not source.with_suffix('.simp').exists()


def test_check_with_output_is_an_argument_error(tmp_path):
    with pytest.raises(SystemExit) as exc:
        main([str(tmp_path / 'source.puml'), '--check', '-o', str(tmp_path / 'out.simp')])
    assert exc.value.code == 2
    assert not list(tmp_path.iterdir())


@pytest.mark.parametrize('check', [False, True])
def test_strict_refuses_detected_losses_before_writing(tmp_path, capsys, check):
    source = tmp_path / 'source.simp'
    root = ET.fromstring(write_simp(UMLDiagram(classes=[UMLClass('a', 'A')])))
    local = root.find('./models/model/diagram/abstract-items')
    assert local is not None
    ET.SubElement(local, 'item', {'id': 'u', 'type': 'unknown'})
    source.write_text(ET.tostring(root, encoding='unicode'), encoding='utf-8')
    args = [str(source), '--strict', *(['--check'] if check else [])]
    assert main(args) == 1
    report = capsys.readouterr()
    assert 'représentation générique' in report.err and 'mode strict' in report.err
    assert 'Conversion terminée' not in report.out
    assert not source.with_suffix('.puml').exists()


def test_strict_accepts_conversion_without_warnings(tmp_path, capsys):
    source = tmp_path / 'source.simp'
    source.write_text(write_simp(UMLDiagram(classes=[UMLClass('a', 'A')])), encoding='utf-8')
    assert main([str(source), '--strict']) == 0
    assert parse_puml(source.with_suffix('.puml').read_text(encoding='utf-8')).classes[0].name == 'A'
    assert not capsys.readouterr().err


def test_strict_refuses_experimental_simp_export(tmp_path, capsys):
    source = tmp_path / 'source.puml'
    source.write_text(PUML, encoding='utf-8')
    assert main([str(source), '--strict']) == 1
    assert 'expérimentale' in capsys.readouterr().err
    assert not source.with_suffix('.simp').exists()


@pytest.mark.parametrize('stale_size', [False, True])
def test_input_size_limit_is_enforced_on_bytes_read(tmp_path, capsys, monkeypatch, stale_size):
    source = tmp_path / 'source.puml'
    text = '@startuml\nclass "Été" as A\n@enduml\n'
    data = text.encode('utf-8')
    source.write_bytes(data)
    monkeypatch.setattr(cli, 'MAX_INPUT_SIZE', len(data))
    assert main([str(source), '--check']) == 0
    capsys.readouterr()

    monkeypatch.setattr(cli, 'MAX_INPUT_SIZE', len(data) - 1)
    if stale_size:
        monkeypatch.setattr(cli.os, 'fstat', lambda fd: SimpleNamespace(st_size=0))
    assert main([str(source), '--check']) == 1
    assert 'trop volumineux' in capsys.readouterr().err
    assert not source.with_suffix('.simp').exists()


@pytest.mark.parametrize('data', [b'\xef\xbb\xbf@startuml\r\nclass A\r\n@enduml\r\n', b'\xff'])
def test_bom_and_invalid_utf8(tmp_path, capsys, data):
    source = tmp_path / 'source.puml'
    source.write_bytes(data)
    status = main([str(source), '--check'])
    assert status == (0 if data.startswith(b'\xef\xbb\xbf') else 1)
    assert not source.with_suffix('.simp').exists()
    if status:
        assert 'Erreur :' in capsys.readouterr().err


def test_directory_source_is_rejected(tmp_path, capsys):
    source = tmp_path / 'directory.puml'
    source.mkdir()
    assert main([str(source)]) == 1
    assert 'fichier ordinaire' in capsys.readouterr().err
    assert not source.with_suffix('.simp').exists()


def test_output_created_during_conversion_is_preserved(tmp_path, capsys, monkeypatch):
    source = tmp_path / 'source.puml'
    source.write_text(PUML, encoding='utf-8')
    output = source.with_suffix('.simp')
    link = os.link

    def competing_link(src, dst):
        output.write_text('créé entre-temps', encoding='utf-8')
        link(src, dst)

    monkeypatch.setattr(cli.os, 'link', competing_link)
    assert main([str(source)]) == 1
    assert output.read_text(encoding='utf-8') == 'créé entre-temps'
    assert not list(tmp_path.glob('.*.tmp'))
    assert 'Erreur :' in capsys.readouterr().err


def test_dangling_output_symlink_is_preserved(tmp_path, capsys):
    source = tmp_path / 'source.puml'
    source.write_text(PUML, encoding='utf-8')
    output = source.with_suffix('.simp')
    missing = tmp_path / 'missing.simp'
    _symlink(output, missing)
    assert main([str(source)]) == 1
    assert 'existe déjà' in capsys.readouterr().err
    assert output.is_symlink() and not missing.exists()
    assert not list(tmp_path.glob('.*.tmp'))
