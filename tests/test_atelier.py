import os
from pathlib import Path
import shutil
import subprocess

import pytest

from simp2puml.atelier import write_atelier
from simp2puml.cli import main
from simp2puml.model import (ConversionError, UMLAttribute, UMLClass, UMLDiagram, UMLNote,
                             UMLOperation, UMLParameter, UMLRelationship)
from simp2puml.puml import parse_puml

PUML = '''@startuml
title Démo
abstract class A {
  +{static} a: std::Thing[] [4]
  {abstract} f(p: Map<String, Callable(A, B)> [0..*], q [1]) [1]
}
interface "IJeu" as I
() "IJeu" as S
enum E {
  RED
  BLUE
  +count: int
}
enum Couleur {
  Rouge
}
class "Propriété" as P
class B
B "2"/part --* "1"/- A : ensemble
A "1"/owner -- "0..*"/items B : nom
A -- S
P --|> A
note as N
Deux
lignes
end note
N .. A
N .. P
@enduml
'''

# Validateur natif d'Atelier Diagrammes, facultatif : chemin explicite ou PATH.
CHECKER = os.environ.get('ATELIER_DIAGRAMMES_BIN') or shutil.which('atelier-diagrammes')


def _checked(text, tmp_path):
    if CHECKER is None:
        return
    source = tmp_path / 'sortie.atelier'
    source.write_text(text, encoding='utf-8', newline='')
    result = subprocess.run([CHECKER, 'check', str(source), '--dsl-version', '2.0'],
                            capture_output=True, text=True, encoding='utf-8')
    assert result.returncode == 0, result.stdout + result.stderr


def test_writes_classifiers_members_relations_and_notes(tmp_path):
    diagram = parse_puml(PUML)
    text = write_atelier(diagram)
    lines = text.splitlines()
    assert lines[:2] == ['dsl 2', '// Démo']
    assert 'abstract class A {' in lines
    assert '  field + static a : std::Thing[] [4]' in lines
    assert '  method abstract f(p : Map<String, Callable(A, B)> [0..*], q [1]) [1]' in lines
    assert 'interface IJeu' in lines and 'circle IJeu_2 "IJeu"' in lines
    assert 'class E <<enumeration>> {' in lines and '    RED' in lines
    assert 'enum Couleur {' in lines and '  Rouge' in lines
    assert 'class Propriete "Propriété"' in lines
    assert '::A "1" *-- "2" ::B [role="part"] : ensemble' in lines
    assert '::A "1" [role="owner"] -- "0..*" ::B [role="items"] : nom' in lines
    assert '::A -- ::IJeu_2' in lines and '::Propriete --|> ::A' in lines
    assert 'note on ::A, ::Propriete : "Deux\\nlignes"' in lines
    assert diagram.warnings == ['Visibilités d’extrémité de relation non représentables en DSL Atelier, omises']
    _checked(text, tmp_path)


def test_identifiers_are_unique_and_ambiguous_texts_are_quoted(tmp_path):
    diagram = UMLDiagram(classes=[
        UMLClass('x1', 'fn', attributes=[UMLAttribute('a', 'Dict{int}'), UMLAttribute('b', 'List<int'),
                                         UMLAttribute('d', 'class Foo')],
                 operations=[UMLOperation('g', [UMLParameter('x', 'rich"y"')])]),
        UMLClass('x2', 'Énumération', kind='enumeration', literals=['Avec espace', '// pas un commentaire']),
        UMLClass('x3', '123'), UMLClass('x4', 'Même'), UMLClass('x5', 'Même'),
    ], relationships=[UMLRelationship('x3', 'x4', name='texte {avec} "guillemets"')],
       notes=[UMLNote('n1', 'libre'), UMLNote('n2', '  espaces  ', ['x1'])])
    text = write_atelier(diagram)
    lines = text.splitlines()
    assert 'class fn_ "fn" {' in lines
    assert '  field "a : Dict{int}"' in lines and '  field "b : List<int"' in lines
    assert '  field d : class Foo' in lines and '  method "g(x : rich\\"y\\")"' in lines
    assert '  "// pas un commentaire"' in lines and '  Avec espace' in lines
    assert 'class C_123 "123"' in lines
    assert 'class Meme "Même"' in lines and 'class Meme_2 "Même"' in lines
    assert '::C_123 -- ::Meme : "texte {avec} \\"guillemets\\""' in lines
    assert 'note [at=(0pt,0pt)] : libre' in lines and 'note on ::fn_ : "  espaces  "' in lines
    assert any('position arbitraire' in w for w in diagram.warnings)
    _checked(text, tmp_path)


def test_unrepresentable_multiplicities_are_reported(tmp_path):
    diagram = UMLDiagram(classes=[UMLClass('a', 'A'), UMLClass('b', 'B')], relationships=[
        UMLRelationship('a', 'b', source_multiplicity='0..n', target_multiplicity='3..1'),
        UMLRelationship('a', 'b', 'inheritance', source_multiplicity='1'),
    ])
    text = write_atelier(diagram)
    assert '::A -- ::B' in text and '::A --|> ::B' in text
    assert sum('0..n' in w or '3..1' in w for w in diagram.warnings) == 2
    assert any('héritage' in w for w in diagram.warnings)
    _checked(text, tmp_path)


def test_overlong_text_is_rejected():
    diagram = UMLDiagram(classes=[UMLClass('a', 'A')], notes=[UMLNote('n', 'x' * 3000, ['a'])])
    with pytest.raises(ConversionError, match='2048'):
        write_atelier(diagram)


def test_cli_exports_atelier_from_both_formats(tmp_path, capsys):
    puml = tmp_path / 'demo.puml'
    puml.write_text(PUML, encoding='utf-8')
    assert main([str(puml), '--to', 'atelier']) == 0
    text = (tmp_path / 'demo.atelier').read_text(encoding='utf-8')
    assert text.startswith('dsl 2\n')
    _checked(text, tmp_path)

    simp = tmp_path / 'demo.simp'
    assert main([str(puml)]) == 0
    capsys.readouterr()
    output = tmp_path / 'depuis-simp.atelier'
    assert main([str(simp), '-o', str(output)]) == 0
    assert 'Sortie :' in capsys.readouterr().out
    assert output.read_text(encoding='utf-8').startswith('dsl 2\n')

    assert main([str(simp), '--to', 'atelier', '--check']) == 0
    assert not (tmp_path / 'demo.atelier.tmp').exists()


def test_cli_target_validation(tmp_path):
    puml = tmp_path / 'demo.puml'
    puml.write_text(PUML, encoding='utf-8')
    for argv in (['--to', 'puml'], ['--to', 'atelier', '-o', str(tmp_path / 'x.puml')]):
        with pytest.raises(SystemExit) as exc:
            main([str(puml), *argv])
        assert exc.value.code == 2
    existing = tmp_path / 'demo.atelier'
    existing.write_text('garder', encoding='utf-8')
    assert main([str(puml), '--to', 'atelier']) == 1
    assert existing.read_text(encoding='utf-8') == 'garder'


def test_strict_reports_atelier_losses(tmp_path, capsys):
    puml = tmp_path / 'demo.puml'
    puml.write_text(PUML, encoding='utf-8')
    assert main([str(puml), '--to', 'atelier', '--strict']) == 1
    assert 'mode strict' in capsys.readouterr().err
    assert not Path(tmp_path / 'demo.atelier').exists()
