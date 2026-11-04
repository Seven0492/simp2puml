from pathlib import Path
import re
import xml.etree.ElementTree as ET

import pytest

from simp2puml.model import (ConversionError, UMLAttribute, UMLClass, UMLDiagram,
                   UMLOperation, UMLParameter, UMLRelationship, UMLNote)
from simp2puml.simp import parse_simp, write_simp


FIXTURE = Path(__file__).resolve().parents[1] / 'Monopoly.simp'
BROKEN = '4db56d1e23f14bb398f93b5f1918504e'


def sample():
    return UMLDiagram('Essai', [
        UMLClass('child', 'Enfant', [UMLAttribute('x', 'Map<A, B>', '-'), UMLAttribute('vide')],
                 [UMLOperation('faire', [UMLParameter('premier', 'T[]'), UMLParameter('second')], 'Résultat', '+')]),
        UMLClass('parent', 'Parent', [UMLAttribute('z', None, '~')]),
    ], [UMLRelationship('child', 'parent', 'inheritance'), UMLRelationship('parent', 'child')])


def normalized(d):
    return (d.name, sorted(d.classes, key=lambda c: c.id), sorted((r.source, r.target, r.kind, r.name or '',
        r.source_multiplicity or '', r.target_multiplicity or '', r.source_role or '',
        r.target_role or '', r.source_visibility or '', r.target_visibility or '') for r in d.relationships),
        sorted((n.id, n.text, tuple(n.targets)) for n in d.notes))


def xml_for(d=None):
    return ET.fromstring(write_simp(d or sample()))


def serialized(root):
    return ET.tostring(root, encoding='unicode')


def required(parent: ET.Element, path: str) -> ET.Element:
    result = parent.find(path)
    assert result is not None, f'Élément XML absent : {path}'
    return result


def abstract(root: ET.Element) -> ET.Element:
    return required(root, './models/model/diagram/abstract-items')


def test_vertical_xml_structure_and_direction():
    d = UMLDiagram('Deux', [UMLClass('c', 'Enfant'), UMLClass('p', 'Parent')],
                   [UMLRelationship('c', 'p', 'inheritance')])
    root = xml_for(d)
    assert root.attrib['version'] == '12.75' and root.attrib['multi-file'] == 'no'
    view = required(root, './models/model/diagram')
    assert view.get('name') == 'Deux' and view.get('type') == 'uml-class'
    generalization = required(view, "./abstract-items/item[@type='generalization']")
    assert (generalization.get('from'), generalization.get('to')) == ('c', 'p')
    layer = required(view, 'layer')
    graphics = {x.get('id'): x for x in layer.findall('item')}
    assert len({x.get('iid') for x in graphics.values()}) == 3
    line = required(graphics[generalization.attrib['id']], 'relation')
    assert (line.get('from-iid'), line.get('to-iid')) == (graphics['c'].get('iid'), graphics['p'].get('iid'))
    assert graphics['c'].find('layout') is not None
    assert normalized(parse_simp(serialized(root))) == normalized(d)


def test_members_and_association_xml_and_roundtrip():
    d = sample()
    root = xml_for(d)
    child = required(abstract(root), "item[@id='child']")
    attr = required(child, './attributes/attribute')
    prop = required(root, "./items/item[@id='%s']" % attr.get('property-ref'))
    assert prop.get('owner') == 'child' and prop.get('visibility') == 'private'
    assert required(prop, 'structural-feature').get('type') == 'Map<A, B>'
    empty = child.findall('./attributes/attribute')[1]
    empty_prop = required(root, "./items/item[@id='%s']" % empty.get('property-ref'))
    assert empty_prop.get('visibility') is None and required(empty_prop, 'structural-feature').get('type') is None
    op = required(child, './operations/operation')
    assert op.get('visibility') == 'public' and op.get('type') == 'Résultat'
    assert [(p.get('name'), p.get('type')) for p in op.findall('./parameters/parameter')] == [('premier', 'T[]'), ('second', None)]
    rel = required(abstract(root), "item[@type='relationship']/relation")
    assert (rel.get('from'), rel.get('to')) == ('parent', 'child')
    assert required(rel, 'start-role').get('association-type') == 'association'
    assert normalized(parse_simp(serialized(root))) == normalized(d)


@pytest.mark.skipif(not FIXTURE.exists(), reason='Fixture réelle Monopoly.simp non redistribuée')
def test_real_original_read_with_orphan_warning():
    d = parse_simp(FIXTURE.read_text(encoding='utf-8-sig'))
    assert len(d.classes) == 32 and len(d.relationships) == 26 and len(d.notes) == 17
    assert any(all(s in w for s in (BROKEN, 'from', 'Service', 'orpheline', 'non représentée', 'omise')) for w in d.warnings)
    by_id = {c.id: c for c in d.classes}
    by_name = {c.name: c for c in d.classes if c.kind != 'interface_symbol'}
    case = by_name['Case']
    assert case.is_abstract and by_name['Propriete'].is_abstract
    assert [(a.name, a.type, a.visibility) for a in case.attributes] == [('numero', 'Integer', '-'), ('nom', 'String', '-')]
    assert next(o for o in case.operations if o.name == 'Action').is_abstract
    assert next(o for o in by_name['Propriete'].operations if o.name == 'CalculCout').is_abstract
    assert next(a for a in by_name['CheminDeFer'].attributes if a.name == 'locationPrix').is_static
    assert next(a for a in by_name['CheminDeFer'].attributes if a.name == 'locationPrix').multiplicity == '4'
    op = next(o for o in by_name['CheminDeFer'].operations if o.name == 'SetLocationPrix')
    assert op.is_static and op.parameters[0].multiplicity == '4'
    assert next(o for o in by_name['CheminDeFer'].operations if o.name == 'GetLocationPrix').multiplicity == '4'
    assert by_name['Couleur'].literals == ['Brun', 'BleuPale', 'Rose', 'Orange', 'Rouge', 'Jaune', 'Vert', 'BleuFonce']
    assert by_name['Pions'].literals == ['DeACoudre', 'FerARepasser', 'Voiture', 'Soulier', 'Chien', 'Chapeau', 'Bateau', 'Brouette']
    assert sum(c.kind == 'interface' for c in d.classes) == 5
    symbols = [c for c in d.classes if c.kind == 'interface_symbol']
    assert len(symbols) == 6 and all(c.id != by_name[c.name].id for c in symbols)
    provided = [r for r in d.relationships if r.kind == 'provided_interface']
    assert len(provided) == 6 and all(by_id[r.target].kind == 'interface_symbol' for r in provided)
    assoc = [r for r in d.relationships if r.kind in ('association', 'aggregation', 'composition')]
    assert len(assoc) == 6
    lookup = {(by_id[r.source].name, by_id[r.target].name, r.kind): r for r in assoc}
    comp = lookup['PlateauMonopoly', 'Case', 'composition']
    assert (comp.source_multiplicity, comp.target_multiplicity, comp.source_visibility, comp.target_visibility) == ('1', '40', '-', '-')
    assert lookup['Joueur', 'Propriete', 'association'].target_multiplicity == '0..40'
    assert lookup['Monopoly', 'Joueur', 'aggregation'].target_multiplicity == '2..8'
    assert any('attribute-ref' in w and 'a0c8b545' in w for w in d.warnings)
    assert len([w for w in d.warnings if 'Propriétés sans attribute' in w]) == 2
    assert any('estDisponible' in w and 'b5a21102' in w for w in d.warnings)
    note = next(n for n in d.notes if n.id == '762fcd3ee2a149559d13af297e6c4456')
    assert note.text.startswith('Quantite:\nLa quantite') and note.targets == [by_name['CheminDeFer'].id]
    assert normalized(parse_simp(write_simp(d))) == normalized(d)


def test_multi_diagram_selection_and_no_merge():
    root = xml_for()
    model = required(root, './models/model')
    other = ET.fromstring(serialized(required(model, 'diagram')))
    other.set('id', 'other'); other.set('name', 'Autre')
    required(other, 'abstract-items').clear()
    required(other, 'layer').clear()
    model.append(other)
    text = serialized(root)
    with pytest.raises(ConversionError, match='Autre'):
        parse_simp(text)
    assert len(parse_simp(text, 'Autre').classes) == 0
    assert len(parse_simp(text, required(model, 'diagram').attrib['id']).classes) == 2
    with pytest.raises(ConversionError, match='choix'):
        parse_simp(text, 'absent')
    other.set('name', 'Essai')
    with pytest.raises(ConversionError, match='ambigu'):
        parse_simp(serialized(root), 'Essai')


@pytest.mark.parametrize('scope', ['model', 'global'])
def test_presented_shared_definitions_are_converted(scope):
    expected = sample()
    root = xml_for(expected)
    destination = required(root, './models/model/abstract-items' if scope == 'model' else './items')
    local = abstract(root)
    for item in list(local):
        local.remove(item)
        destination.append(item)

    assert normalized(parse_simp(serialized(root))) == normalized(expected)


def test_unpresented_model_definitions_are_excluded():
    root = xml_for(UMLDiagram(classes=[UMLClass('visible', 'Visible')]))
    shared = required(root, './models/model/abstract-items')
    ET.SubElement(shared, 'item', {'id': 'unused', 'type': 'class', 'name': 'Hors diagramme'})
    parsed = parse_simp(serialized(root))
    assert [cls.id for cls in parsed.classes] == ['visible']
    assert not parsed.warnings


def test_presentations_do_not_import_definitions_from_other_diagrams():
    root = xml_for(UMLDiagram(classes=[UMLClass('visible', 'Visible')]))
    model = required(root, './models/model')
    other = ET.SubElement(model, 'diagram', {'id': 'other', 'name': 'Autre', 'type': 'uml-class'})
    other_items = ET.SubElement(other, 'abstract-items')
    ET.SubElement(other_items, 'item', {'id': 'remote', 'type': 'class', 'name': 'Distant'})
    ET.SubElement(required(root, './models/model/diagram/layer'), 'item', {'id': 'remote', 'type': 'entity'})

    parsed = parse_simp(serialized(root), 'Diagram')
    assert [cls.id for cls in parsed.classes] == ['visible']


def test_broken_references_and_omitted_endpoint():
    root = xml_for()
    a = abstract(root)
    rel = required(a, "item[@type='generalization']")
    rel.set('to', 'missing')
    with pytest.raises(ConversionError, match='missing'):
        parse_simp(serialized(root))
    rel.set('to', 'parent')
    required(a, "item[@id='parent']").set('type', 'enumeration')
    d = parse_simp(serialized(root))
    assert len(d.relationships) == 2 and next(c for c in d.classes if c.id == 'parent').kind == 'enumeration'
    required(a, "item[@id='child']/attributes/attribute").set('property-ref', 'missing')
    with pytest.raises(ConversionError, match='property-ref'):
        parse_simp(serialized(root))


def test_duplicate_ids_and_missing_from():
    root = xml_for()
    a = abstract(root)
    required(a, "item[@id='parent']").set('id', 'child')
    with pytest.raises(ConversionError, match='dupliqué'):
        parse_simp(serialized(root))
    root = xml_for()
    del required(abstract(root), "item[@type='generalization']").attrib['from']
    with pytest.raises(ConversionError, match='Extrémité'):
        parse_simp(serialized(root))


@pytest.mark.parametrize('missing,value', [('from', None), ('from', ''), ('to', None), ('to', '')])
def test_unreferenced_incomplete_generalization_is_omitted(missing, value):
    root = xml_for()
    rel = required(abstract(root), "item[@type='generalization']")
    ident = rel.attrib['id']
    layer = required(root, './models/model/diagram/layer')
    layer.remove(required(layer, f"item[@id='{ident}']"))
    if value is None:
        del rel.attrib[missing]
    else:
        rel.set(missing, value)
    d = parse_simp(serialized(root))
    assert [r.kind for r in d.relationships] == ['association']
    name = 'Parent' if missing == 'from' else 'Enfant'
    assert any(all(s in w for s in (ident, missing, name, 'orpheline', 'non représentée', 'omise')) for w in d.warnings)


@pytest.mark.parametrize('case', ['referenced', 'invalid_endpoint', 'association', 'no_presentation', 'both_missing'])
def test_incomplete_relation_guardrails(case):
    root = xml_for()
    layer = required(root, './models/model/diagram/layer')
    typ = 'relationship' if case == 'association' else 'generalization'
    item = required(abstract(root), f"item[@type='{typ}']")
    ident = item.attrib['id']
    layer.remove(required(layer, f"item[@id='{ident}']"))
    rel = required(item, 'relation') if typ == 'relationship' else item
    del rel.attrib['from']
    if case == 'referenced':
        ET.SubElement(root, 'reference', {'ref': ident})
    elif case == 'invalid_endpoint':
        rel.set('to', 'missing')
    elif case == 'no_presentation':
        layer.clear()
    elif case == 'both_missing':
        del rel.attrib['to']
    with pytest.raises(ConversionError, match='missing' if case == 'invalid_endpoint' else 'Extrémité'):
        parse_simp(serialized(root))


def test_valid_generalization_without_presentation():
    root = xml_for()
    layer = required(root, './models/model/diagram/layer')
    layer.clear()
    assert any(r.kind == 'inheritance' for r in parse_simp(serialized(root)).relationships)


def test_type_id_resolution_and_unknown():
    root = xml_for()
    op = required(abstract(root), "item[@id='child']/operations/operation")
    op.set('type', ''); op.set('type-id', 'Class:parent')
    p = required(op, './parameters/parameter')
    p.set('type', ''); p.set('type-id', 'Uml.Integer')
    parsed = parse_simp(serialized(root)).classes[0].operations[0]
    assert parsed.return_type == 'Parent' and parsed.parameters[0].type == 'Integer'
    p.set('type-id', 'Class:missing')
    with pytest.raises(ConversionError, match='Type référencé'):
        parse_simp(serialized(root))
    p.set('type', 'Nom apparent')
    with pytest.raises(ConversionError, match='Class:missing'):
        parse_simp(serialized(root))


def test_modifiers_and_composition_preserved():
    root = xml_for()
    cls = required(abstract(root), "item[@id='child']")
    cls.set('is-abstract', 'true')
    required(cls, './operations/operation').set('static', 'true')
    rel = required(abstract(root), "item[@type='relationship']/relation")
    required(rel, 'end-role').set('association-type', 'composition')
    required(rel, 'end-role').set('multiplicity', '5')
    d = parse_simp(serialized(root))
    assert d.classes[0].is_abstract and d.classes[0].operations[0].is_static
    comp = next(r for r in d.relationships if r.kind == 'composition')
    assert (comp.source, comp.target, comp.source_multiplicity) == ('child', 'parent', '5')
    required(rel, 'start-role').set('association-type', 'aggregation')
    with pytest.raises(ConversionError, match='non représentables'):
        parse_simp(serialized(root))


@pytest.mark.parametrize('location, expected', [
    ('attribute', 'Enfant.x'),
    ('operation', 'Enfant.faire'),
    ('parameter', 'Enfant.faire/premier'),
])
def test_member_multiplicity_warning(location, expected):
    root = xml_for()
    cls = required(abstract(root), "item[@id='child']")
    if location == 'attribute':
        ref = required(cls, './attributes/attribute').attrib['property-ref']
        node = required(root, f"./items/item[@id='{ref}']/structural-feature")
    elif location == 'operation':
        node = required(cls, './operations/operation')
    else:
        node = required(cls, './operations/operation/parameters/parameter')
    node.set('multiplicity', '2..*')
    parsed = parse_simp(serialized(root)).classes[0]
    actual = parsed.attributes[0] if location == 'attribute' else parsed.operations[0] if location == 'operation' else parsed.operations[0].parameters[0]
    assert actual.multiplicity == '2..*'
    assert not any('Multiplicité omise' in w for w in parse_simp(serialized(root)).warnings)


def test_select_all_diagram_families_explicitly():
    root = xml_for()
    model = required(root, './models/model')
    other = ET.SubElement(model, 'diagram', {'id': 'sequence', 'name': 'Sequence', 'type': 'uml-sequence'})
    ET.SubElement(other, 'abstract-items')
    with pytest.raises(ConversionError, match='Sélection'):
        parse_simp(serialized(root))
    assert parse_simp(serialized(root), 'Essai').name == 'Essai'
    assert parse_simp(serialized(root), 'Sequence').source_view.kind == 'uml-sequence'


def test_index_all_semantic_locations_and_detect_duplicates():
    root = xml_for()
    model = required(root, './models/model')
    local = abstract(root)
    globals_ = required(root, 'items')
    prop = required(globals_, 'item')
    globals_.remove(prop)
    required(model, 'abstract-items').append(prop)
    other = ET.SubElement(model, 'diagram', {'id': 'other', 'name': 'Other', 'type': 'uml-class'})
    other_items = ET.SubElement(other, 'abstract-items')
    ET.SubElement(other_items, 'item', {'id': 'remote', 'type': 'class', 'name': 'Distant'})
    op = required(local, "item[@id='child']/operations/operation")
    op.set('type', '')
    op.set('type-id', 'Class:remote')
    rel = required(local, "item[@type='generalization']")
    rel.set('to', 'remote')
    d = parse_simp(serialized(root), 'Essai')
    child = next(c for c in d.classes if c.id == 'child')
    assert child.attributes[0].type == 'Map<A, B>'
    assert child.operations[0].return_type == 'Distant'
    assert len(d.classes) == 2 and any('vers élément omis' in w for w in d.warnings)
    ET.SubElement(other_items, 'item', {'id': 'child', 'type': 'class', 'name': 'Collision'})
    with pytest.raises(ConversionError, match='dupliqué'):
        parse_simp(serialized(root), 'Essai')


def test_unsupported_items_report_type_id_and_name():
    root = xml_for()
    ET.SubElement(abstract(root), 'item', {'id': 'u1', 'type': 'universal-connector', 'name': 'Mystere'})
    ET.SubElement(required(root, 'items'), 'item', {'id': 'u2', 'type': 'new-kind', 'name': 'Autre'})
    from simp2puml.puml import write_puml
    parsed = parse_simp(serialized(root))
    output = write_puml(parsed)
    warnings = parsed.warnings
    assert 'Mystere' in output
    assert any(all(s in w for s in ('universal-connector', 'u1')) for w in warnings)
    assert not any('u2' in w for w in warnings)


def test_generated_types_are_observed_or_text_only():
    d = UMLDiagram(classes=[UMLClass('a', 'Alpha', [UMLAttribute('b', 'Beta'), UMLAttribute('i', 'Integer'), UMLAttribute('m', 'Map<X>')]), UMLClass('b', 'Beta')])
    root = xml_for(d)
    assert required(root, './types/type-sets/type-set').get('file') == 'UmlTypes.xml'
    custom = required(root, './types/custom-types/type')
    assert (custom.get('id'), custom.get('name'), custom.get('class-ref')) == ('Class:b', 'Beta', 'b')
    props = {p.get('name'): required(p, 'structural-feature') for p in root.findall('./items/item')}
    assert props['b'].get('type-id') == 'Class:b'
    assert props['i'].get('type-id') == 'Uml.Integer'
    assert props['m'].get('type') == 'Map<X>' and props['m'].get('type-id') is None
    d.classes.append(UMLClass('b2', 'Beta'))
    root = xml_for(d)
    assert root.find('./types/custom-types/type') is None
    assert required(root, "./items/item[@name='b']/structural-feature").get('type-id') is None


def test_geometry_uses_class_boundaries_points_and_member_sizes():
    d = UMLDiagram(classes=[UMLClass('a', 'A', [UMLAttribute('x' + str(i)) for i in range(8)]), UMLClass('b', 'B'), UMLClass('c', 'C'), UMLClass('d', 'D')],
                   relationships=[UMLRelationship('a', 'd'), UMLRelationship('b', 'b')])
    d.classes.append(UMLClass('enum', 'Couleurs', kind='enumeration', literals=['V' + str(i) for i in range(8)]))
    root = xml_for(d)
    layer = required(root, './models/model/diagram/layer')
    boxes = {e.get('id'): required(e, 'layout') for e in layer.findall("item[@type='entity']")}
    assert int(boxes['enum'].attrib['height']) >= 55 + 20 * 8
    assert int(boxes['a'].attrib['height']) > int(boxes['b'].attrib['height'])
    assert int(boxes['d'].attrib['y']) > int(boxes['a'].attrib['y'])
    edges = layer.findall("item[@type='relation']")
    def on_border(point, box):
        x, y = map(float, point)
        bx, by, w, h = (float(box.attrib[k]) for k in ('x', 'y', 'width', 'height'))
        return bx <= x <= bx + w and by <= y <= by + h and (x in (bx, bx + w) or y in (by, by + h))
    for edge, source, target in zip(edges, ('a', 'b'), ('d', 'b')):
        layout = required(edge, 'layout')
        points = [(p.attrib['x'], p.attrib['y']) for p in layout.findall('./points/point')]
        assert len(points) >= 2
        assert points[0] == (layout.get('x1'), layout.get('y1'))
        assert points[-1] == (layout.get('x2'), layout.get('y2'))
        assert on_border(points[0], boxes[source]) and on_border(points[-1], boxes[target])
    assert len(edges[1].findall('./layout/points/point')) > 2


@pytest.mark.parametrize('text', [
    '<!DOCTYPE sim-project><sim-project version="12.75" multi-file="no"/>',
    '<!DOCTYPE sim-project [<!ENTITY x "x">]><sim-project/>',
    '<wrong version="12.75" multi-file="no"/>',
    '<sim-project version="12.74" multi-file="no"/>',
    '<sim-project version="12.75" multi-file="yes"/>',
    '<sim-project',
])
def test_reject_unsafe_or_invalid_xml(text):
    with pytest.raises(ConversionError):
        parse_simp(text)


def test_extended_roundtrip_and_graphics():
    d = UMLDiagram('Complet', [
        UMLClass('c', 'C', [UMLAttribute('x', 'Integer', '-', True, '2..*')],
                 [UMLOperation('f', [UMLParameter('a', 'String', '*'), UMLParameter('b')], 'Boolean', '+', True, True, '1')], is_abstract=True),
        UMLClass('i', 'I', kind='interface'), UMLClass('s', 'I', kind='interface_symbol'),
        UMLClass('e', 'E', kind='enumeration', literals=['A', 'B'])],
        [UMLRelationship('c', 'e', 'composition', 'possède', '1', '*', 'tout', 'partie', '+', '-'),
         UMLRelationship('c', 'e', 'composition', 'possède', '1', '*', 'tout', 'partie', '+', '-'),
         UMLRelationship('c', 's', 'provided_interface')], notes=[UMLNote('n', "Ligne d'abord\n\nFin", ['c', 'e'])])
    root = xml_for(d)
    assert required(abstract(root), "item[@id='i']/stereotypes/stereotype").get('name') == 'interface'
    assert [x.get('name') for x in required(abstract(root), "item[@id='e']/enumeration-items")] == ['A', 'B']
    assert required(root, "./items/item[@id='s']/interface") is not None
    connector = required(root, "./items/item[@type='uml-provided-interface-connector']")
    assert connector.get('to') == 's'
    assert len(root.findall("./items/item[@type='comment-connector']")) == 2
    assert normalized(parse_simp(serialized(root))) == normalized(d)


def test_unknown_layer_reference_rejected():
    root = xml_for()
    ET.SubElement(required(root, './models/model/diagram/layer'), 'item', {'id': 'missing-graphic', 'type': 'entity'})
    with pytest.raises(ConversionError, match='missing-graphic'):
        parse_simp(serialized(root))


def test_visibility_only_association_ends_are_not_drawn_but_kept():
    from simp2puml.puml import parse_puml, write_puml
    root = xml_for()
    rel = required(abstract(root), "item[@type='relationship']/relation")
    for tag, mult in (('start-role', '1'), ('end-role', '40')):
        role = required(rel, tag)
        role.set('visibility', 'private'); role.set('multiplicity', mult); role.set('name', '')
    d = parse_simp(serialized(root))
    assert d.source_view is None  # Lecteur de classes historique.
    text = write_puml(d)
    link = next(line for line in text.splitlines() if ' -- ' in line)
    assert '/' not in link and link.endswith('"40" child')  # Aucun « - » isolé dessiné par PlantUML.
    assert "' simp2puml-visibilite: - -" in text
    again = next(r for r in parse_puml(text).relationships if r.kind == 'association')
    assert (again.source_visibility, again.target_visibility) == ('-', '-')  # Rien n'est perdu.
    assert (again.source_multiplicity, again.target_multiplicity) == ('1', '40')


def test_unsupported_role_fields_and_ambiguous_type_warning():
    root = xml_for()
    rel = required(abstract(root), "item[@type='relationship']/relation")
    rel.set('derived', 'true')
    role = required(rel, 'end-role')
    role.set('navigability', 'true'); role.set('attribute-ref', 'a123')
    from simp2puml.puml import write_puml
    d = parse_simp(serialized(root))
    output = write_puml(d)
    # Navigable vers la cible ; la flèche peut porter une direction de mise en page (-down->).
    assert re.search(r' -(?:-|up-|down-|left-|right-)> ', output) and ' : /' in output
    assert any('attribute-ref' in w for w in d.warnings)
    role.set('navigability', ''); role.set('attribute-ref', '')
    rel.set('derived', 'false')
    ET.SubElement(abstract(root), 'item', {'id': 'dupname', 'type': 'class', 'name': 'Parent'})
    op = required(abstract(root), "item[@id='child']/operations/operation")
    op.set('type', 'Parent'); op.set('type-id', 'Class:parent')
    assert any('référence Class:parent perdue' in w for w in parse_simp(serialized(root)).warnings)
