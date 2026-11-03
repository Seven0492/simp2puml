import pytest

from simp2puml.model import (ConversionError, UMLAttribute, UMLClass, UMLDiagram, UMLNote,
                   UMLOperation, UMLParameter, UMLRelationship)
from simp2puml.puml import parse_puml, write_puml


def test_classes_members_and_roundtrip():
    diagram = parse_puml('''@startuml
/' commentaire
multiligne '/
class "Une classe" as C
{
  +value: Map<String, List<Integer>>
  compute(arg: Map<String, Integer>, other): List<String>
}
class P
C --|> P
@enduml
''')
    cls = diagram.classes[0]
    assert (cls.id, cls.name) == ('C', 'Une classe')
    assert (cls.attributes[0].name, cls.attributes[0].type, cls.attributes[0].visibility) == (
        'value', 'Map<String, List<Integer>>', '+')
    op = cls.operations[0]
    assert [(p.name, p.type) for p in op.parameters] == [('arg', 'Map<String, Integer>'), ('other', None)]
    assert (op.return_type, op.visibility) == ('List<String>', None)
    assert (diagram.relationships[0].source, diagram.relationships[0].target, diagram.relationships[0].kind) == ('C', 'P', 'inheritance')
    assert parse_puml(write_puml(diagram)) == diagram


def test_parenthesized_types_roundtrip():
    diagram = parse_puml('''@startuml
class A {
  f: Callable(A, B)
  run(arg: Map<String, Callable(A, B)>, callback: Callable(X, Y)): Result
}
@enduml''')
    cls = diagram.classes[0]
    assert cls.attributes == [UMLAttribute(name='f', type='Callable(A, B)')]
    assert cls.operations == [UMLOperation(name='run', parameters=[
        UMLParameter(name='arg', type='Map<String, Callable(A, B)>'),
        UMLParameter(name='callback', type='Callable(X, Y)'),
    ], return_type='Result')]
    assert parse_puml(write_puml(diagram)) == diagram


@pytest.mark.parametrize('member', ['x: int = 5', 'f(x: int = 5)', 'x: Map<A, B=5>'])
def test_reject_default_values(member):
    with pytest.raises(ConversionError):
        parse_puml('@startuml\nclass A {\n' + member + '\n}\n@enduml')


def test_quoted_equals_in_type_and_writer_rejection():
    diagram = parse_puml('@startuml\nclass A {\n x: Literal<"a=b">\n}\n@enduml')
    assert parse_puml(write_puml(diagram)) == diagram
    diagram.classes[0].attributes[0].type = 'int = 5'
    with pytest.raises(ConversionError):
        write_puml(diagram)


def test_implicit_classes_and_warnings():
    diagram = parse_puml('''@startuml
A <|-- B
B --> C
class "Bee" as B
hide empty members
@enduml''')
    assert [(c.id, c.name) for c in diagram.classes] == [('A', 'A'), ('B', 'Bee'), ('C', 'C')]
    assert [(r.source, r.target, r.kind) for r in diagram.relationships] == [('B', 'A', 'inheritance'), ('B', 'C', 'association')]
    assert len(diagram.warnings) == 2


@pytest.mark.parametrize('statement', [
    'package P {', '!include external',
    'class A <<entity>>', 'class A\n{\n f(String value)\n}',
    'class A\n{\n x: Map<A, B\n}',
])
def test_reject_unsupported(statement):
    with pytest.raises(ConversionError):
        parse_puml('@startuml\n' + statement + '\n@enduml')


def test_writer_maps_ids_preserves_title_and_unicode_members():
    diagram = UMLDiagram(
        name='ClassDiagram1',
        classes=[
            UMLClass(id='a146c340-2e18-41c4-a612-31b575370452', name='Même nom',
                     attributes=[UMLAttribute(name='état', type='Texte')],
                     operations=[UMLOperation(name='calculerÉtat',
                                              parameters=[UMLParameter(name='entrée', type='Entier')])]),
            UMLClass(id='C1', name='Même nom'),
            UMLClass(id='autre id', name='Troisième'),
        ],
        relationships=[UMLRelationship(source='a146c340-2e18-41c4-a612-31b575370452',
                                       target='C1', kind='inheritance'),
                       UMLRelationship(source='C1', target='autre id')],
    )
    text = write_puml(diagram)
    assert text == write_puml(diagram)
    assert 'title ClassDiagram1\n' in text
    result = parse_puml(text)
    assert result.name == diagram.name
    assert [c.id for c in result.classes] == ['C2', 'C1', 'C3']
    assert [c.name for c in result.classes] == ['Même nom', 'Même nom', 'Troisième']
    assert result.classes[0].attributes == diagram.classes[0].attributes
    assert result.classes[0].operations == diagram.classes[0].operations
    old_indices = {c.id: i for i, c in enumerate(diagram.classes)}
    new_indices = {c.id: i for i, c in enumerate(result.classes)}
    def normalized(relations, indices):
        return [(indices[r.source], indices[r.target], r.kind) for r in relations]
    assert normalized(result.relationships, new_indices) == normalized(diagram.relationships, old_indices)


@pytest.mark.parametrize('name', ['ligne\nsuivante', ' espaces ', "l'apostrophe", ''])
def test_writer_rejects_unrepresentable_title(name):
    with pytest.raises(ConversionError):
        write_puml(UMLDiagram(name=name))


def test_title_read_and_duplicate_rejected():
    assert parse_puml('@startuml\ntitle Mon diagramme\n@enduml').name == 'Mon diagramme'
    with pytest.raises(ConversionError):
        parse_puml('@startuml\ntitle Un\ntitle Deux\n@enduml')


def test_extended_members_and_classifiers():
    diagram = parse_puml('''@startuml
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
  run(): T [*]
}
A -- S
@enduml''')
    a, interface, symbol, enum = diagram.classes
    assert a.is_abstract and interface.kind == 'interface' and symbol.kind == 'interface_symbol'
    assert enum.kind == 'enumeration' and enum.literals == ['RED', 'BLUE']
    assert a.attributes == [UMLAttribute('a', 'std::Thing[]', '+', True, '4')]
    assert a.operations[0].is_abstract and a.operations[0].multiplicity == '1'
    assert [(p.name, p.multiplicity) for p in a.operations[0].parameters] == [('p', '0..*'), ('q', '1')]
    assert diagram.relationships[0].kind == 'provided_interface'
    assert parse_puml(write_puml(diagram)) == diagram


def test_relations_reversed_and_forward_symbols():
    diagram = parse_puml('''@startuml
S -- A
B "2"/part --* "1"/- A : ensemble
B "*"/items --o "1"/+owner A
A "1"/owner -- "0..*"/items B : nom
A -- B
A -- B
() "IJeu" as S
@enduml''')
    first, comp, agg, assoc, *duplicates = diagram.relationships
    assert (first.source, first.target, first.kind) == ('A', 'S', 'provided_interface')
    assert (comp.source, comp.target, comp.source_multiplicity, comp.target_multiplicity,
            comp.source_visibility, comp.target_role, comp.name) == ('A', 'B', '1', '2', '-', 'part', 'ensemble')
    assert (agg.source, agg.target, agg.source_role, agg.target_role) == ('A', 'B', 'owner', 'items')
    assert (assoc.source_role, assoc.target_role) == ('owner', 'items')
    assert len(duplicates) == 2 and duplicates[0] == duplicates[1]
    assert parse_puml(write_puml(diagram)) == diagram


def test_notes_raw_and_aliases_roundtrip():
    diagram = UMLDiagram(classes=[UMLClass('C1', 'Même'), UMLClass('uuid classe', 'Même')],
                         notes=[UMLNote('note uuid', 'l\'ami "ok"\n\nfin', ['C1', 'uuid classe', 'C1'])])
    text = write_puml(diagram)
    result = parse_puml(text)
    assert result.notes[0].text == diagram.notes[0].text
    assert parse_puml(text.replace('\n', '\r\n')) == result
    indices = {cls.id: i for i, cls in enumerate(result.classes)}
    assert [indices[t] for t in result.notes[0].targets] == [0, 1, 0]
    assert len({c.id for c in result.classes} | {result.notes[0].id}) == 3


@pytest.mark.parametrize('text', ['end note', '@enduml', '@startuml', '!include file', ' !define X Y'])
def test_note_rejects_directives(text):
    with pytest.raises(ConversionError):
        write_puml(UMLDiagram(notes=[UMLNote('N', text)]))


def test_ambiguous_type_and_enum_attribute_rejected():
    with pytest.raises(ConversionError):
        write_puml(UMLDiagram(classes=[UMLClass('A', 'A', attributes=[UMLAttribute('a', 'T [1]')])]))
    with pytest.raises(ConversionError):
        write_puml(UMLDiagram(classes=[UMLClass('E', 'E', attributes=[UMLAttribute('x')], kind='enumeration')]))


@pytest.mark.parametrize('text, number, detail', [
    ('@startuml\nclass A {\n  x: int = 5\n}\n@enduml', 3, 'initialisateur'),
    ('@startuml\nclass A {\n  run(p: Map<X, Y)\n}\n@enduml', 3, 'Délimiteurs'),
    ('@startuml\nclass "inachevée\n@enduml', 2, 'guillemet'),
    ("@startuml\n/' commentaire\nsuite", 2, 'commentaire'),
    ('@startuml\ntitle\n@enduml', 2, 'Titre'),
    ('@startuml\nA/ -- B\n@enduml', 2, 'Rôle vide'),
    ('@startuml\nN .. Absent\nnote as N\ntexte\nend note\n@enduml', 2, 'cible de note'),
    ('@startuml\nnote as N\ntexte\n!include fichier\nend note\n@enduml', 4, 'Directive interdite'),
])
def test_parse_errors_identify_the_source_line(text, number, detail):
    with pytest.raises(ConversionError) as exc:
        parse_puml(text)
    assert f'Ligne {number}:' in str(exc.value)
    assert detail in str(exc.value)
