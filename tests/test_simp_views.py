"""Fixtures synthétiques, plus validations facultatives sans redistribuer SIMP."""
import json
import os
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

import pytest

from simp2puml.atelier import write_atelier
from simp2puml.cli import main
from simp2puml.model import ConversionError
from simp2puml.puml import write_puml
from simp2puml.simp import list_diagrams, parse_simp, write_simp


def project(kind, items, graphics='', version='12.75'):
    return f'''<sim-project version="{version}" multi-file="no"><items/>
    <models><model><abstract-items/><diagram id="view" name="Test" type="{kind}">
    <abstract-items>{items}</abstract-items><layer>{graphics}</layer>
    </diagram></model></models></sim-project>'''


def item(ident, kind, name='', body='', **attrs):
    node = ET.Element('item', {'id': ident, 'type': kind, 'name': name, **attrs})
    if body:
        node.extend(ET.fromstring('<body>' + body + '</body>'))
    return ET.tostring(node, encoding='unicode')


def convert(kind, items, graphics='', version='12.75'):
    diagram = parse_simp(project(kind, items, graphics, version))
    return write_puml(diagram), diagram.warnings


def validate_with_plantuml(text, tmp_path):
    jar = os.environ.get('PLANTUML_JAR')
    if not jar:
        return
    source = tmp_path / 'check.puml'
    source.write_text(text, encoding='utf-8')
    result = subprocess.run(['java', '-DPLANTUML_SECURITY_PROFILE=SECURE', '-jar', jar,
                             '-charset', 'UTF-8', '-checkonly', str(source)],
                            capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, result.stdout + result.stderr + '\n' + text.split("' simp2puml-source:")[0]


@pytest.mark.parametrize('kind', [
    'uml-class', 'uml-usecase', 'uml-component', 'uml-deployment', 'uml-object',
    'uml-package', 'uml-composite-structure', 'entity-relationship', 'flowchart',
    'bpmn', 'sysml-block', 'archimate', 'mind-map', 'user-interface', 'future-kind',
])
def test_every_structural_family_converts_or_reports_generic(kind, tmp_path):
    a = item('a', 'class' if kind == 'uml-class' else 'component', 'A')
    b = item('b', 'class' if kind == 'uml-class' else 'component', 'B')
    edge = item('r', 'dependency', 'uses', **{'from': 'a', 'to': 'b'})
    text, warnings = convert(kind, a + b + edge)
    assert '..>' in text and 'uses' in text
    assert 'A' in text and 'B' in text
    if kind not in ('uml-class', 'uml-usecase', 'uml-component', 'uml-deployment',
                    'uml-object', 'uml-package', 'uml-composite-structure', 'entity-relationship'):
        assert any('représentation générique' in w for w in warnings)
    validate_with_plantuml(text, tmp_path)


def test_rich_classes_defaults_stereotypes_roles_and_dependencies(tmp_path):
    members = '''<stereotypes><stereotype name="entity"/></stereotypes>
    <attributes><attribute id="attr" name="value" type="Integer" visibility="private" default-value="42" read-only="true"/></attributes>
    <operations><operation id="op" name="f" type="String" static="true"><parameters>
    <parameter id="param" name="arg" type="Integer" direction="inout" default-value="1"/>
    </parameters></operation></operations>'''
    a = item('a', 'class', 'A', members)
    b = item('b', 'uml-interface-classifier', 'I')
    rel = item('r', 'interface-realization', **{'from': 'a', 'to': 'b'})
    assoc = item('assoc', 'relationship', body='''<relation from="a" to="b" derived="true">
    <start-role name="owner" multiplicity="1" association-type="composition"/>
    <end-role name="items" multiplicity="*" navigability="true"/></relation>''')
    text, _ = convert('uml-class', a + b + rel + assoc)
    assert '<<entity>>' in text and '= 42' in text and 'readOnly' in text
    assert '{static}' in text and 'inout arg: Integer = 1' in text
    assert '..|>' in text and '*-->' in text
    assert 'owner' in text and 'items' in text
    validate_with_plantuml(text, tmp_path)


def test_member_edit_dates_are_metadata_not_annotations(tmp_path):
    # Forme observée dans un projet enregistré au profil 15.1 : chaque membre porte creation-date.
    members = '''<attributes><attribute id="at" name="valeur" creation-date="10/28/2020 11:29:03"/></attributes>
    <operations><operation id="op" name="Get" type="Integer" visibility="public" creation-date="10/28/2020 11:30:00">
    <parameters><parameter id="pa" name="x" type="Integer" creation-date="10/28/2020 11:31:00"/></parameters>
    </operation></operations>'''
    text, warnings = convert('uml-class', item('a', 'class', 'A', members, **{'creation-date': '1/1/2020'}),
                             version='15.1')
    assert 'creation-date' not in text.split("' simp2puml-source")[0]
    assert not any('propriétés supplémentaires' in w for w in warnings)
    flagged = members.replace('name="Get"', 'name="Get" is-query="true"')
    text, warnings = convert('uml-class', item('a', 'class', 'A', flagged), version='15.1')
    assert 'Get.is-query = true' in text and any('propriétés supplémentaires' in w for w in warnings)
    validate_with_plantuml(text, tmp_path)


def test_nested_packages_duplicate_names_and_endpoint_from_graphic(tmp_path):
    items = item('p', 'package', 'P') + item('a', 'component', 'Same', owner='p')
    items += item('b', 'component', 'Same') + item('r', 'dependency')
    graphics = '<item id="r"><relation from="a" to="b"/></item>'
    text, _ = convert('uml-component', items, graphics)
    assert 'package "P" as E1 {\ncomponent "Same" as E2' in text
    assert 'E2 ..> E3' in text
    validate_with_plantuml(text, tmp_path)


def test_state_transitions_guards_actions_and_decisions(tmp_path):
    items = item('start', 'initial-state') + item('s', 'state', 'Ready', '<actions><action type="entry" name="reset()"/></actions>')
    items += item('d', 'decision') + item('end', 'final-state')
    items += item('r1', 'general-transition', 'go', '<transition guard="x &gt; 0" behaviourExpression="save()"/>', **{'from': 'start', 'to': 's'})
    items += item('r2', 'general-transition', **{'from': 's', 'to': 'd'})
    items += item('r3', 'general-transition', **{'from': 'd', 'to': 'end'})
    text, _ = convert('uml-statechart', items)
    assert '<<start>>' in text and '<<end>>' in text and '<<choice>>' in text
    assert 'reset()' in text and 'save()' in text and 'x ' in text
    validate_with_plantuml(text, tmp_path)


def test_activity_graph_does_not_invent_structured_branching(tmp_path):
    items = item('a', 'action', 'Do') + item('b', 'decision', 'OK?')
    items += item('r', 'control-flow', 'again', **{'from': 'b', 'to': 'a'})
    text, warnings = convert('uml-activity', items)
    assert '<<choice>>' in text and 'E2 --> E1 : again' in text
    assert any('notation d’états' in w for w in warnings)
    validate_with_plantuml(text, tmp_path)


def test_sequence_order_owner_async_and_return(tmp_path):
    items = item('a', 'actor', 'User') + item('b', 'lifeline', 'System')
    items += item('act', 'activation-bar', owner='b')
    items += item('later', 'sequence-action', body='<sequence-action type="return" from="act" to="a" message="result"/>')
    items += item('first', 'sequence-action', body='<sequence-action type="call" from="a" to="act" message="call" is-asynchronous="true"/>')
    graphics = '''<item id="a" type="lifeline"><layout x="0"/></item><item id="b" type="lifeline"><layout x="200"/></item>
    <item id="act"><layout y="20" height="100"/></item>
    <item id="first"><layout y1="30"/></item><item id="later"><layout y1="100"/></item>'''
    text, _ = convert('uml-sequence', items, graphics)
    assert text.index('E1 ->> E2 : call') < text.index('E2 --> E1 : result')
    assert 'activate E2' in text and 'deactivate E2' in text
    validate_with_plantuml(text, tmp_path)


def test_sequence_fragments_and_guards(tmp_path):
    items = item('a', 'lifeline', 'A') + item('b', 'lifeline', 'B')
    items += item('f', 'fragment', 'Choice', '<fragment operator="alt"><operands><operand uid="o1"/><operand uid="o2"/></operands></fragment>')
    items += item('o1', 'interaction-operand', body='<interaction-operand guard="yes"/>')
    items += item('o2', 'interaction-operand', body='<interaction-operand guard="no"/>')
    items += item('m', 'sequence-action', body='<sequence-action type="call" from="a" to="b" message="go"/>')
    graphics = '''<item id="f"><layout y="10" height="100"/></item>
    <item id="o1"><layout y="10"/></item><item id="o2"><layout y="60"/></item>
    <item id="m"><layout y1="30"/></item>'''
    text, _ = convert('uml-sequence', items, graphics)
    assert 'alt yes\n' in text and 'else no\nend\n' in text
    validate_with_plantuml(text, tmp_path)


def test_unknown_elements_are_visible_and_source_is_archived(tmp_path):
    items = item('x', 'future-widget', 'Special', '<special mode="nonstandard">content</special>')
    text, warnings = convert('new-diagram', items)
    assert 'rectangle "Special"' in text and 'content' in text and 'nonstandard' in text
    assert any('générique' in w for w in warnings)
    records = [json.loads(line.removeprefix("' simp2puml-source ")) for line in text.splitlines() if line.startswith("' simp2puml-source ")]
    assert ET.fromstring(records[0]['xml']).findtext('special') == 'content'
    validate_with_plantuml(text, tmp_path)


@pytest.mark.parametrize('name', ['@enduml\n!include /secret\n@startuml', '%load_json("file")', '<img:/secret>', 'quote"\\end', '[[https://example.com]]'])
def test_data_cannot_inject_plantuml(name, tmp_path):
    text, _ = convert('uml-component', item('x', 'component', name))
    body = text.split("' simp2puml-source:")[0]
    assert body.count('@startuml') == 1 and '@enduml' not in body and '!include' not in body
    assert text.count('@startuml') == 1 and text.count('@enduml') == 1
    assert '%load_json' not in body and '<img:' not in body and '[[' not in body
    validate_with_plantuml(text, tmp_path)


def test_selection_listing_all_strict_and_no_partial_output(tmp_path, capsys):
    root = ET.fromstring(project('uml-component', item('a', 'component', 'A')))
    model = root.find('./models/model')
    second = ET.SubElement(model, 'diagram', id='second', name='States', type='uml-statechart')
    abstract = ET.SubElement(second, 'abstract-items')
    ET.SubElement(abstract, 'item', id='s', type='state', name='Ready')
    text = ET.tostring(root, encoding='unicode')
    assert list_diagrams(text) == [('view', 'Test', 'uml-component'), ('second', 'States', 'uml-statechart')]
    with pytest.raises(ConversionError, match='Sélection'):
        parse_simp(text)
    source = tmp_path / 'source.simp'
    source.write_text(text, encoding='utf-8')
    assert main([str(source), '--list-diagrams']) == 0
    assert len(capsys.readouterr().out.splitlines()) == 2
    assert main([str(source), '--all']) == 0
    output = source.with_suffix('.puml')
    assert output.read_text(encoding='utf-8').count('@startuml') == 2
    assert main([str(source), '--all', '--check']) == 0
    bad = ET.SubElement(abstract, 'item', id='broken', type='dependency', **{'from': 's', 'to': 'missing'})
    source.write_text(ET.tostring(root, encoding='unicode'), encoding='utf-8')
    destination = tmp_path / 'incomplete.puml'
    assert main([str(source), '--all', '-o', str(destination)]) == 1
    assert not destination.exists()
    abstract.remove(bad)
    ET.SubElement(abstract, 'item', id='unknown', type='future-kind', name='X')
    source.write_text(ET.tostring(root, encoding='unicode'), encoding='utf-8')
    assert main([str(source), '--all', '--strict', '-o', str(destination)]) == 1
    assert not destination.exists()


def test_other_targets_refuse_empty_lossy_exports():
    diagram = parse_simp(project('uml-component', item('a', 'component', 'A')))
    for writer in (write_simp, write_atelier):
        with pytest.raises(ConversionError, match='PlantUML|puml'):
            writer(diagram)


def test_metadata_diagram_references_are_not_selectable_diagrams():
    items = item('a', 'component', 'A', '<associated-diagrams><diagram ref="another"/></associated-diagrams>')
    assert list_diagrams(project('uml-component', items)) == [('view', 'Test', 'uml-component')]


def test_repeated_writes_are_deterministic_without_duplicate_warnings():
    diagram = parse_simp(project('future-kind', item('a', 'unknown', 'A')))
    first = write_puml(diagram)
    warnings = list(diagram.warnings)
    assert write_puml(diagram) == first and diagram.warnings == warnings


def test_rich_member_type_references_are_resolved_and_validated():
    members = '<attributes><attribute id="attr" name="value" type-id="Class:b"/></attributes>'
    text, _ = convert('uml-class', item('a', 'class', 'A', members) + item('b', 'class', 'B'))
    assert 'value: B' in text
    with pytest.raises(ConversionError, match='Type référencé introuvable'):
        convert('uml-class', item('a', 'class', 'A', members))


def test_rich_classes_preserve_narrow_orphan_generalization_exception():
    items = item('a', 'class', 'A', '<stereotypes><stereotype name="entity"/></stereotypes>')
    items += item('orphan', 'generalization', **{'to': 'a'})
    output, warnings = convert('uml-class', items, '<item id="a"/>')
    assert any('orpheline' in w for w in warnings)
    assert 'class "A"' in output
    with pytest.raises(ConversionError, match='Extrémité'):
        convert('uml-class', items, '<item id="a"/><item id="orphan"/>')


@pytest.mark.parametrize('version', ['10', '12.75', '99.5'])
def test_version_profiles_are_explicit(version):
    _, warnings = convert('uml-component', item('a', 'component', 'A'), version=version)
    assert any('profil non validé' in w for w in warnings) == (version == '99.5')


def test_xml_nesting_and_duplicate_occurrences_are_rejected():
    text = project('uml-component', item('a', 'component', 'A'), '<item id="a" iid="same"/><item id="a" iid="same"/>')
    with pytest.raises(ConversionError, match='occurrence dupliqué'):
        parse_simp(text)
    with pytest.raises(ConversionError, match='Imbrication XML'):
        parse_simp('<sim-project version="12.75" multi-file="no">' + '<x>' * 110 + '</x>' * 110 + '</sim-project>')


def test_property_references_keep_feature_type_and_group_containment(tmp_path):
    root = ET.fromstring(project('uml-class', item('a', 'class', 'A',
        '<attributes><attribute id="attr" name="value" property-ref="p"/></attributes>')
        + item('g', 'group', 'Group')))
    globals_ = root.find('items')
    globals_.append(ET.fromstring(item('p', 'property', 'value', '<structural-feature type="Integer"/><feature static="true"/>')))
    layer = root.find('./models/model/diagram/layer')
    layer.append(ET.fromstring('<item id="g" type="group"><group-items><item id="a"/></group-items></item>'))
    output = write_puml(parse_simp(ET.tostring(root, encoding='unicode')))
    assert '{static} value: Integer' in output
    assert 'rectangle "Group"' in output and 'as E2 {\nclass "A"' in output
    validate_with_plantuml(output, tmp_path)


def test_association_class_even_when_link_precedes_relationship(tmp_path):
    items = item('a', 'class', 'A') + item('b', 'class', 'B') + item('c', 'class', 'Link')
    items += item('link', 'association-class-link', **{'from': 'c', 'to': 'r'})
    items += item('r', 'relationship', body='<relation from="a" to="b"><start-role/><end-role/></relation>')
    text, _ = convert('uml-class', items)
    assert text.index('E1 -- E2') < text.index('(E1, E2) .. E3')
    validate_with_plantuml(text, tmp_path)


@pytest.mark.parametrize('kind', ['signal', 'data-type', 'uml-n-ary-association', 'collaboration', 'collaboration-use'])
def test_additional_class_element_shapes(kind, tmp_path):
    text, _ = convert('uml-class', item('a', kind, 'Example'))
    assert 'Example' in text
    validate_with_plantuml(text, tmp_path)


def test_nested_group_shapes_remain_nested(tmp_path):
    items = item('a', 'group', 'Outer') + item('b', 'group', 'Inner') + item('c', 'component', 'C')
    graphics = '<item id="a"><group-items><item id="b"><group-items><item id="c"/></group-items></item></group-items></item>'
    text, _ = convert('uml-component', items, graphics)
    assert 'as E1 {\nrectangle "Inner" as E2 {\ncomponent "C"' in text
    validate_with_plantuml(text, tmp_path)


@pytest.mark.parametrize('items', [
    item('a', 'component', owner='b') + item('b', 'component', owner='a'),
    item('a', 'component', owner='a'),
    item('a', 'component') + item('r', 'dependency', **{'from': 'a', 'to': 'missing'}),
    item('a', 'component') + item('r', 'dependency', **{'from': 'a'}),
])
def test_invalid_references_and_container_cycles(items):
    with pytest.raises(ConversionError):
        convert('uml-component', items)


@pytest.mark.skipif(not os.environ.get('SIMP_REFERENCE_FILE'), reason='Exemple réel SIMP non redistribué')
def test_real_project_all_views_with_renderer(tmp_path):
    source = Path(os.environ['SIMP_REFERENCE_FILE'])
    original = source.read_bytes()
    text = original.decode('utf-8-sig')
    views = list_diagrams(text)
    assert views
    for ident, name, _ in views:
        output = write_puml(parse_simp(text, ident))
        try:
            validate_with_plantuml(output, tmp_path)
        except AssertionError as exc:
            raise AssertionError(f'{name}\n{exc}') from exc
    if os.environ.get('PLANTUML_JAR'):
        output = tmp_path / 'reference.puml'
        assert main([str(source), '--all', '-o', str(output)]) == 0
        result = subprocess.run(['java', '-DPLANTUML_SECURITY_PROFILE=SECURE', '-jar', os.environ['PLANTUML_JAR'],
                                 '-charset', 'UTF-8', '-tsvg', str(output)],
                                capture_output=True, text=True, timeout=90)
        assert result.returncode == 0, result.stdout + result.stderr
        assert len(list(tmp_path.glob('reference*.svg'))) == len(views)
    assert source.read_bytes() == original
